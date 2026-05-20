//! Integration: drive the MCP write tools against a real in-process daemon.
//!
//! Each tool exercises a different daemon mutation path. We also pin the
//! error-surface contract: when the daemon refuses (D5 GWT_EMPTY,
//! EVIDENCE_REQUIRED, etc.), the tool result must be `is_error: true` and
//! the error code MUST be present in the text — that's the signal the LLM
//! reads to decide retry vs abort.

use sdi_daemon::AppState;
use sdi_db::Paths;
use sdi_mcp::tools::write::{
    AddDecision, AddRequirement, AddScenario, StartRound, UpdateTaskEvidence,
};
use sdi_mcp::tools::{DaemonClient, Tool};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

async fn spawn_daemon() -> String {
    let tmp = std::env::temp_dir().join(format!(
        "sdi-mcp-it-w-{}-{}",
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
    let resp = c.post(format!("{}{}", base, path)).json(body).send().await.unwrap();
    let status = resp.status();
    let text = resp.text().await.unwrap();
    assert!(status.is_success(), "{path} → {status}: {text}");
    serde_json::from_str(&text).unwrap()
}

fn text_of(r: &sdi_mcp::protocol::ToolCallResult) -> String {
    match &r.content[0] {
        sdi_mcp::protocol::ContentBlock::Text { text } => text.clone(),
    }
}

struct Bootstrap {
    plan_id: String,
    round_id: String,
    task_id: String,
    scenario_id: String,
}

/// Seeds the minimum graph each write-tool test needs.
async fn bootstrap(base: &str) -> Bootstrap {
    let c = http().await;
    let suffix = ulid::Ulid::new().to_string();
    let key = format!("W{}", &suffix[..4]);
    let slug = format!("w-{}", suffix[..6].to_lowercase());

    let project = post(
        &c,
        base,
        "/projects",
        &json!({ "key": key, "name": "MCP Write", "slug": slug }),
    )
    .await;
    let project_id = project["id"].as_str().unwrap().to_string();

    let plan = post(
        &c,
        base,
        "/plans",
        &json!({
            "project_id": project_id,
            "short_code": format!("W-{}", &suffix[..6]),
            "title": "Write plan",
            "body": "p"
        }),
    )
    .await;
    let plan_id = plan["id"].as_str().unwrap().to_string();

    // Confirmed scenario so plan can be approved (D8 gate).
    let scn = post(
        &c,
        base,
        "/scenarios",
        &json!({
            "plan_id": plan_id,
            "short_code": format!("SCN-{}", &suffix[..6]),
            "given": "x",
            "when": "y",
            "then": "z",
            "confirmed": true
        }),
    )
    .await;
    let scenario_id = scn["id"].as_str().unwrap().to_string();
    post(&c, base, &format!("/plans/{}/approve", plan_id), &json!({})).await;

    let round = post(
        &c,
        base,
        "/rounds",
        &json!({ "plan_id": plan_id, "short_code": format!("R-{}", &suffix[..6]) }),
    )
    .await;
    let round_id = round["id"].as_str().unwrap().to_string();
    post(
        &c,
        base,
        &format!("/rounds/{}/activate", round_id),
        &json!({}),
    )
    .await;

    let task = post(
        &c,
        base,
        "/tasks",
        &json!({
            "round_id": round_id,
            "short_code": format!("T-{}", &suffix[..6]),
            "description": "impl",
            "parent_scenario_ids": [scenario_id]
        }),
    )
    .await;
    let task_id = task["id"].as_str().unwrap().to_string();
    post(
        &c,
        base,
        &format!("/tasks/{}/status", task_id),
        &json!({ "status": "in_progress" }),
    )
    .await;

    Bootstrap {
        plan_id,
        round_id,
        task_id,
        scenario_id,
    }
}

#[tokio::test]
async fn add_scenario_round_trip_creates_a_row() {
    let base = spawn_daemon().await;
    let b = bootstrap(&base).await;
    let client = Arc::new(DaemonClient::new(base));

    let tool = AddScenario { client };
    let res = tool
        .call(json!({
            "plan_id": b.plan_id,
            "short_code": "SCN-NEW",
            "given": "a registered user",
            "when": "they call /auth/login",
            "then": "they get a 200 with a token"
        }))
        .await;
    assert!(!res.is_error, "{}", text_of(&res));
    let v: Value = serde_json::from_str(&text_of(&res)).unwrap();
    assert_eq!(v["status"], "draft");
    assert_eq!(v["short_code"], "SCN-NEW");
}

#[tokio::test]
async fn add_scenario_surfaces_d5_gwt_empty_from_daemon() {
    let base = spawn_daemon().await;
    let b = bootstrap(&base).await;
    let client = Arc::new(DaemonClient::new(base));

    let tool = AddScenario { client };
    // Whitespace-only `then` triggers D5 GWT_EMPTY at the daemon. We assert
    // the tool propagates the daemon code so the LLM can branch on it.
    let res = tool
        .call(json!({
            "plan_id": b.plan_id,
            "short_code": "SCN-BAD",
            "given": "a",
            "when": "b",
            "then": "   "
        }))
        .await;
    assert!(res.is_error);
    assert!(text_of(&res).contains("GWT_EMPTY"), "got: {}", text_of(&res));
}

#[tokio::test]
async fn add_requirement_round_trip() {
    let base = spawn_daemon().await;
    let b = bootstrap(&base).await;
    let client = Arc::new(DaemonClient::new(base));
    let tool = AddRequirement { client };

    let res = tool
        .call(json!({
            "plan_id": b.plan_id,
            "short_code": "REQ-LOGIN",
            "title": "Login",
            "body": "user must log in",
            "source": "interview"
        }))
        .await;
    assert!(!res.is_error, "{}", text_of(&res));
    let v: Value = serde_json::from_str(&text_of(&res)).unwrap();
    assert_eq!(v["short_code"], "REQ-LOGIN");
    assert_eq!(v["source"], "interview");
}

#[tokio::test]
async fn add_decision_then_supersede() {
    let base = spawn_daemon().await;
    let b = bootstrap(&base).await;
    let client = Arc::new(DaemonClient::new(base));
    let tool = AddDecision { client: client.clone() };

    let r1 = tool
        .call(json!({
            "plan_id": b.plan_id,
            "short_code": "DEC-A",
            "title": "Use JWT",
            "body": "v0.1"
        }))
        .await;
    assert!(!r1.is_error, "{}", text_of(&r1));
    let d1: Value = serde_json::from_str(&text_of(&r1)).unwrap();
    let d1_id = d1["id"].as_str().unwrap();

    let r2 = tool
        .call(json!({
            "plan_id": b.plan_id,
            "short_code": "DEC-B",
            "title": "Opaque tokens",
            "body": "replay risk",
            "supersedes_id": d1_id
        }))
        .await;
    assert!(!r2.is_error, "{}", text_of(&r2));

    // Daemon should have auto-flipped d1 → superseded.
    let c = http().await;
    let fresh: Value = c
        .get(format!(
            "{}/decisions/{}",
            client.url(""),
            d1_id
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fresh["status"], "superseded");
}

#[tokio::test]
async fn update_task_evidence_completes_with_prd_evidence_required_satisfied() {
    let base = spawn_daemon().await;
    let b = bootstrap(&base).await;
    let client = Arc::new(DaemonClient::new(base));
    let tool = UpdateTaskEvidence { client };

    let res = tool
        .call(json!({
            "task_id": b.task_id,
            "summary": "endpoint live",
            "scenarios": [{
                "scenario_id": b.scenario_id,
                "result": "passing",
                "evidence_ref": "auth.rs:42"
            }]
        }))
        .await;
    assert!(!res.is_error, "{}", text_of(&res));
    let v: Value = serde_json::from_str(&text_of(&res)).unwrap();
    assert_eq!(v["status"], "done");
    assert!(v["evidence_at"].is_string());
}

#[tokio::test]
async fn start_round_is_idempotent_on_already_active_round() {
    let base = spawn_daemon().await;
    let b = bootstrap(&base).await;
    let client = Arc::new(DaemonClient::new(base));
    let tool = StartRound { client };

    // The round is already active from bootstrap — the daemon should
    // surface a structured error (not a tool panic), and the tool result
    // should be is_error=true with the code visible.
    let res = tool.call(json!({ "round_id": b.round_id })).await;
    assert!(res.is_error, "expected error on double-activate");
    let txt = text_of(&res);
    assert!(
        txt.contains("400") || txt.contains("ROUND"),
        "expected daemon error visible, got: {txt}"
    );
}
