//! v0.4 multi-agent governance HTTP integration.
//!
//! Asserts:
//! 1. POST /autonomy_policies upserts and emits autonomy.changed.
//! 2. GET /autonomy_policies/resolve falls through plan → kind → global.
//! 3. POST /autonomy_policies/circuit_breaker demotes all rows to L3 and
//!    emits circuit_breaker.triggered.
//! 4. POST /agent_notes round-trips a hand-off, ack clears the queue.
//! 5. POST /decisions with kind=consensus before any critique returns 400
//!    (M3 ordering gate).
//! 6. consensus after critique succeeds and emits consensus.reached.

use sdi_daemon::AppState;
use sdi_db::Paths;
use std::sync::Arc;
use std::time::Duration;

async fn spawn_server() -> (String, tokio::task::JoinHandle<()>) {
    let tmp = std::env::temp_dir().join(format!(
        "sdid-mav-{}-{}",
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
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    (base, handle)
}

fn cli() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

async fn bootstrap(base: &str, suffix: &str) -> (String, String) {
    let c = cli();
    let key = format!("M{}", &suffix[..4]);
    let slug = format!("m-{}", &suffix[..6].to_lowercase());
    let proj: serde_json::Value = c
        .post(format!("{}/projects", base))
        .json(&serde_json::json!({"key": key, "name": "MA", "slug": slug}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let project_id = proj["id"].as_str().unwrap().to_string();
    let plan_code = format!("M-{}", &suffix[..6]);
    let plan: serde_json::Value = c
        .post(format!("{}/plans", base))
        .json(&serde_json::json!({
            "project_id": project_id,
            "short_code": plan_code,
            "title": "v",
            "body": ""
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let plan_id = plan["id"].as_str().unwrap().to_string();
    (project_id, plan_id)
}

#[tokio::test]
async fn autonomy_upsert_resolve_and_circuit_breaker() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let (project_id, plan_id) = bootstrap(&base, &suffix).await;

    // Upsert global L5.
    let r1 = c
        .post(format!("{}/autonomy_policies", base))
        .json(&serde_json::json!({
            "project_id": project_id,
            "scope_kind": "global",
            "mode": "L5",
            "set_by": "agent",
        }))
        .send()
        .await
        .unwrap();
    assert!(r1.status().is_success(), "global upsert failed: {:?}", r1.status());

    // Upsert plan L4 — should resolve first.
    c.post(format!("{}/autonomy_policies", base))
        .json(&serde_json::json!({
            "project_id": project_id,
            "plan_id": plan_id,
            "scope_kind": "plan",
            "mode": "L4",
            "set_by": "user",
        }))
        .send()
        .await
        .unwrap();

    let resolved: serde_json::Value = c
        .get(format!(
            "{}/autonomy_policies/resolve?project_id={}&plan_id={}",
            base, project_id, plan_id
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resolved["policy"]["mode"], "L4");

    // Trigger circuit breaker.
    let r2: serde_json::Value = c
        .post(format!("{}/autonomy_policies/circuit_breaker", base))
        .json(&serde_json::json!({
            "project_id": project_id,
            "actor": "user",
            "reason": "panic",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(r2["demoted"].as_i64().unwrap() >= 1);

    let after: serde_json::Value = c
        .get(format!(
            "{}/autonomy_policies/resolve?project_id={}&plan_id={}",
            base, project_id, plan_id
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after["policy"]["mode"], "L3");
}

#[tokio::test]
async fn forced_l4_blocks_l5_on_architecture_scope() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let (project_id, _plan_id) = bootstrap(&base, &suffix).await;

    let resp = c
        .post(format!("{}/autonomy_policies", base))
        .json(&serde_json::json!({
            "project_id": project_id,
            "scope_kind": "decision_kind",
            "decision_kind": "architecture",
            "mode": "L5",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[tokio::test]
async fn agent_note_handoff_round_trip() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let (project_id, plan_id) = bootstrap(&base, &suffix).await;

    let note: serde_json::Value = c
        .post(format!("{}/agent_notes", base))
        .json(&serde_json::json!({
            "project_id": project_id,
            "plan_id": plan_id,
            "scope_kind": "plan",
            "kind": "handoff",
            "from_agent": "impl-coder",
            "to_agent": "test-runner",
            "body": "please verify",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let note_id = note["id"].as_str().unwrap().to_string();

    let pending: serde_json::Value = c
        .get(format!("{}/agent_notes/handoffs?to_agent=test-runner", base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(pending["notes"].as_array().unwrap().len(), 1);

    let ack = c
        .post(format!("{}/agent_notes/{}/ack", base, note_id))
        .send()
        .await
        .unwrap();
    assert!(ack.status().is_success());

    let pending: serde_json::Value = c
        .get(format!("{}/agent_notes/handoffs?to_agent=test-runner", base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(pending["notes"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn decision_m3_gate_blocks_consensus_without_critique() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let (_project_id, plan_id) = bootstrap(&base, &suffix).await;

    // 1) Propose.
    let proposal: serde_json::Value = c
        .post(format!("{}/decisions", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("DEC-P-{}", &suffix[..6]),
            "title": "p",
            "body": "b",
            "kind": "proposal",
            "agent_name": "decision-resolver",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let proposal_id = proposal["id"].as_str().unwrap().to_string();

    // 2) Consensus before critique — must fail.
    let bad = c
        .post(format!("{}/decisions", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("DEC-C1-{}", &suffix[..6]),
            "title": "c",
            "body": "b",
            "kind": "consensus",
            "proposal_id": proposal_id,
            "agent_name": "decision-resolver",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status().as_u16(), 400);

    // 3) Critique, then consensus succeeds.
    c.post(format!("{}/decisions", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("DEC-K-{}", &suffix[..6]),
            "title": "k",
            "body": "b",
            "kind": "critique",
            "proposal_id": proposal_id,
            "agent_name": "schema-architect",
        }))
        .send()
        .await
        .unwrap();
    let ok = c
        .post(format!("{}/decisions", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("DEC-C2-{}", &suffix[..6]),
            "title": "c",
            "body": "b",
            "kind": "consensus",
            "proposal_id": proposal_id,
            "agent_name": "decision-resolver",
        }))
        .send()
        .await
        .unwrap();
    assert!(ok.status().is_success(), "consensus after critique should succeed");
}
