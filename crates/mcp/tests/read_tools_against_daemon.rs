//! Integration: drive the MCP read tools against a real in-process daemon.
//!
//! Boots `sdi_daemon::router::build` on a random port, seeds a project / plan
//! (active) / scenario / decisions / knowledge in both scopes, then invokes
//! each read tool through its `Tool::call` surface. Asserts the LM invariant:
//! `search_knowledge` MUST NOT surface scope=reference entries even if they
//! match the FTS query, because the tool forces scope=rag at the URL layer.

use sdi_daemon::AppState;
use sdi_db::Paths;
use sdi_mcp::tools::read::{GetPlanContext, GetRecentDecisions, SearchKnowledge};
use sdi_mcp::tools::{DaemonClient, Tool};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

async fn spawn_daemon() -> String {
    let tmp = std::env::temp_dir().join(format!(
        "sdi-mcp-it-{}-{}",
        std::process::id(),
        ulid::Ulid::new()
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    let paths = Arc::new(Paths {
        data: tmp.clone(),
        cache: tmp.clone(),
        config: tmp.clone(),
        state: tmp.clone(),
        db_file: tmp.join("sdi.db"),
        pid_file: tmp.join("sdid.pid"),
        port_file: tmp.join("sdid.port"),
        socket_file: tmp.join("sdid.sock"),
        log_file: tmp.join("sdid.log"),
    });
    let pool = sdi_db::open(&paths).expect("open db");
    let state = AppState::new(pool, paths);
    let app = sdi_daemon::router::build(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{}", addr);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    base
}

async fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

async fn post(c: &reqwest::Client, base: &str, path: &str, body: &Value) -> Value {
    let resp = c
        .post(format!("{}{}", base, path))
        .json(body)
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let text = resp.text().await.unwrap();
    assert!(status.is_success(), "{path} → {status}: {text}");
    serde_json::from_str(&text).unwrap()
}

/// Seeds a project + active plan + one confirmed scenario + two decisions +
/// two knowledge entries (one rag, one reference) so each read tool has
/// something to find and the scope-leak assertion has a control case.
async fn seed(base: &str) -> Seeded {
    let c = http().await;
    let suffix = ulid::Ulid::new().to_string();
    let key = format!("M{}", &suffix[..4]);
    let slug = format!("mcp-{}", suffix[..6].to_lowercase());

    let project = post(
        &c,
        base,
        "/projects",
        &json!({ "key": key, "name": "MCP E2", "slug": slug }),
    )
    .await;
    let project_id = project["id"].as_str().unwrap().to_string();

    let plan = post(
        &c,
        base,
        "/plans",
        &json!({
            "project_id": project_id,
            "short_code": format!("MCP-{}", &suffix[..6]),
            "title": "MCP plan",
            "body": "p"
        }),
    )
    .await;
    let plan_id = plan["id"].as_str().unwrap().to_string();

    let _scn = post(
        &c,
        base,
        "/scenarios",
        &json!({
            "plan_id": plan_id,
            "short_code": format!("SCN-{}", &suffix[..6]),
            "given": "a user",
            "when": "they login",
            "then": "they get a token",
            "confirmed": true
        }),
    )
    .await;

    // Activate the plan so plan-level queries see a real `active` row.
    let _ = post(&c, base, &format!("/plans/{}/approve", plan_id), &json!({})).await;

    // Two decisions (most-recent first per repo ordering).
    let _d1 = post(
        &c,
        base,
        "/decisions",
        &json!({
            "plan_id": plan_id,
            "short_code": format!("DEC-{}-A", &suffix[..6]),
            "title": "Use JWT",
            "body": "first"
        }),
    )
    .await;
    let _d2 = post(
        &c,
        base,
        "/decisions",
        &json!({
            "plan_id": plan_id,
            "short_code": format!("DEC-{}-B", &suffix[..6]),
            "title": "Switch to opaque",
            "body": "second"
        }),
    )
    .await;

    // Two knowledge entries with the SAME body keyword so the scope=rag
    // filter is what differentiates them at search time.
    let _k_rag = post(
        &c,
        base,
        "/knowledge",
        &json!({
            "project_id": project_id,
            "scope": "rag",
            "kind": "decision",
            "title": "RAG-side token note",
            "body": "uniqkeyword opaque tokens are preferred over JWT",
            "tags": ["auth"]
        }),
    )
    .await;
    let _k_ref = post(
        &c,
        base,
        "/knowledge",
        &json!({
            "project_id": project_id,
            "scope": "reference",
            "kind": "runbook",
            "title": "Reference runbook",
            "body": "uniqkeyword internal procedure should never reach the LLM",
            "tags": ["ops"]
        }),
    )
    .await;

    Seeded {
        project_id,
        plan_id,
    }
}

struct Seeded {
    project_id: String,
    plan_id: String,
}

#[tokio::test]
async fn search_knowledge_never_leaks_reference_scope() {
    let base = spawn_daemon().await;
    let seeded = seed(&base).await;
    let client = Arc::new(DaemonClient::new(base.clone()));

    let tool = SearchKnowledge {
        client: client.clone(),
    };
    // The query matches BOTH the rag and reference entries by body text.
    // If the tool weren't forcing scope=rag, we'd see two hits — and the
    // LLM would learn the reference content. We assert exactly one hit,
    // and that its scope is "rag".
    let res = tool
        .call(json!({ "project_id": seeded.project_id, "q": "uniqkeyword" }))
        .await;
    assert!(
        !res.is_error,
        "search_knowledge surfaced error: {:?}",
        res.content
    );
    let text = match &res.content[0] {
        sdi_mcp::protocol::ContentBlock::Text { text } => text.clone(),
    };
    let payload: Value = serde_json::from_str(&text).unwrap();
    let hits = payload["knowledge"].as_array().expect("knowledge array");
    assert_eq!(hits.len(), 1, "expected exactly 1 rag hit, got {hits:?}");
    assert_eq!(hits[0]["scope"], "rag");
    assert!(
        hits[0]["body"]
            .as_str()
            .unwrap_or("")
            .contains("opaque tokens"),
        "expected rag body, got {:?}",
        hits[0]
    );
}

#[tokio::test]
async fn get_plan_context_returns_composite_snapshot() {
    let base = spawn_daemon().await;
    let seeded = seed(&base).await;
    let client = Arc::new(DaemonClient::new(base));
    let tool = GetPlanContext { client };

    let res = tool.call(json!({ "plan_id": seeded.plan_id })).await;
    assert!(!res.is_error, "get_plan_context errored: {:?}", res.content);
    let text = match &res.content[0] {
        sdi_mcp::protocol::ContentBlock::Text { text } => text.clone(),
    };
    let payload: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(payload["plan"]["id"], seeded.plan_id);
    assert_eq!(payload["plan"]["status"], "active");
    assert_eq!(payload["scenarios"].as_array().unwrap().len(), 1);
    // No tasks yet, so in_flight is empty — but the key must exist.
    assert!(payload["tasks_in_flight"].is_array());
    assert_eq!(payload["decisions"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn get_recent_decisions_respects_limit() {
    let base = spawn_daemon().await;
    let seeded = seed(&base).await;
    let client = Arc::new(DaemonClient::new(base));
    let tool = GetRecentDecisions { client };

    let res = tool
        .call(json!({ "plan_id": seeded.plan_id, "limit": 1 }))
        .await;
    assert!(!res.is_error);
    let text = match &res.content[0] {
        sdi_mcp::protocol::ContentBlock::Text { text } => text.clone(),
    };
    let payload: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(payload["decisions"].as_array().unwrap().len(), 1);
}
