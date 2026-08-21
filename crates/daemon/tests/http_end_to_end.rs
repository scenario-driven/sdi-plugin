//! End-to-end HTTP smoke test: exercises the full SDI happy path against a
//! live daemon over reqwest, plus the C4 surfaces (decisions, knowledge).
//!
//! Flow:
//! 1. POST  /projects                       — create project
//! 2. POST  /plans                          — create plan (draft)
//! 3. POST  /requirements                   — author a requirement (SNAPSHOT)
//! 4. POST  /scenarios  (confirmed=true)    — author a confirmed GWT scenario
//! 5. POST  /plans/:id/approve              — D8 gate satisfied → active
//! 6. POST  /rounds                         — create R1 (planning)
//! 7. POST  /rounds/:id/activate            — R1 active
//! 8. POST  /tasks                          — create a task tied to the scenario
//! 9. POST  /tasks/:id/status (in_progress) — pick it up
//! 10. POST /tasks/:id/complete (evidence)  — PRD §6.6 evidence-required path
//! 11. POST /rounds/:id/results             — record per-scenario verdict
//! 12. POST /rounds/:id/complete            — R1 done
//! 13. POST /plans/:id/complete             — plan done
//! 14. POST /decisions                      — capture an ADR
//! 15. POST /decisions (supersedes prior)   — chained supersession
//! 16. POST /knowledge (rag + reference)    — both scopes, MCP isolation contract
//! 17. GET  /knowledge?project_id&scope=rag — verify scope filter

use sdi_daemon::AppState;
use sdi_db::Paths;
use std::sync::Arc;
use std::time::Duration;

async fn spawn_server() -> (String, tokio::task::JoinHandle<()>) {
    let tmp = std::env::temp_dir().join(format!(
        "sdid-e2e-{}-{}",
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
        lock_file: tmp.join("sdid.lock"),
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

#[tokio::test]
async fn full_sdi_flow_end_to_end() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let key = format!("E{}", &suffix[..4]);
    let slug = format!("e2e-{}", suffix[..6].to_lowercase());

    // 1. project
    let project: serde_json::Value = c
        .post(format!("{}/projects", base))
        .json(&serde_json::json!({
            "key": key,
            "name": "End-to-End",
            "slug": slug,
            "cwds": ["/tmp/e2e"]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let project_id = project["id"].as_str().unwrap().to_string();

    // 2. plan
    let plan: serde_json::Value = c
        .post(format!("{}/plans", base))
        .json(&serde_json::json!({
            "project_id": project_id,
            "short_code": format!("E2E-{}", &suffix[..6]),
            "title": "v0.1 ship",
            "body": "ship the MVP"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let plan_id = plan["id"].as_str().unwrap().to_string();
    assert_eq!(plan["status"], "draft");

    // 3. requirement
    let req: serde_json::Value = c
        .post(format!("{}/requirements", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("REQ-{}", &suffix[..6]),
            "title": "User can log in",
            "body": "Email+password auth"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let _req_id = req["id"].as_str().unwrap().to_string();

    // 4. scenario (confirmed)
    let scn: serde_json::Value = c
        .post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("SCN-{}", &suffix[..6]),
            "given": "a registered user",
            "when": "they submit valid credentials",
            "then": "they receive a session token",
            "confirmed": true
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let scn_id = scn["id"].as_str().unwrap().to_string();

    // 5. approve plan
    let plan: serde_json::Value = c
        .post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(plan["status"], "active");
    assert!(plan["approved_at"].is_string());

    // 6 + 7. round 1 create + activate
    let r1: serde_json::Value = c
        .post(format!("{}/rounds", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("R1-{}", &suffix[..6])
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let r1_id = r1["id"].as_str().unwrap().to_string();
    assert_eq!(r1["status"], "planning");
    assert_eq!(r1["mode"], "strict-regression");
    assert_eq!(r1["disruption_policy"], "needs-review");
    let r = c
        .post(format!("{}/rounds/{}/activate", base, r1_id))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // 8 + 9. task create + in_progress
    let task: serde_json::Value = c
        .post(format!("{}/tasks", base))
        .json(&serde_json::json!({
            "round_id": r1_id,
            "short_code": format!("T-{}", &suffix[..6]),
            "description": "implement /auth/login handler",
            "parent_scenario_ids": [scn_id.clone()]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let task_id = task["id"].as_str().unwrap().to_string();
    let _ = c
        .post(format!("{}/tasks/{}/status", base, task_id))
        .json(&serde_json::json!({"status": "in_progress"}))
        .send()
        .await
        .unwrap();

    // 10. complete with evidence
    let done: serde_json::Value = c
        .post(format!("{}/tasks/{}/complete", base, task_id))
        .json(&serde_json::json!({
            "evidence": {
                "scenarios": [{
                    "scenario_id": scn_id,
                    "result": "passing",
                    "evidence_ref": "auth.rs:42"
                }],
                "summary": "endpoint live, login returns 200 + jwt"
            }
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(done["status"], "done");
    assert!(done["evidence_at"].is_string());

    // 11. record per-scenario verdict on round 1
    let r = c
        .post(format!("{}/rounds/{}/results", base, r1_id))
        .json(&serde_json::json!({
            "scenario_id": scn_id,
            "result": "passing",
            "evidence_ref": "auth.rs:42"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // 12 + 13. round + plan complete
    let r = c
        .post(format!("{}/rounds/{}/complete", base, r1_id))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let plan_done: serde_json::Value = c
        .post(format!("{}/plans/{}/complete", base, plan_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(plan_done["status"], "completed");

    // 14. decision (accepted)
    let d1: serde_json::Value = c
        .post(format!("{}/decisions", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("DEC-{}-A", &suffix[..6]),
            "title": "Use jwt for session",
            "body": "we picked JWT over server sessions for v0.1"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let d1_id = d1["id"].as_str().unwrap().to_string();
    assert_eq!(d1["status"], "accepted");

    // 15. supersede d1
    let d2: serde_json::Value = c
        .post(format!("{}/decisions", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("DEC-{}-B", &suffix[..6]),
            "title": "Migrate to opaque session tokens",
            "body": "JWT replay risk; switch to opaque tokens + Redis store",
            "supersedes_id": d1_id
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let _d2_id = d2["id"].as_str().unwrap().to_string();
    // prior decision should be flipped to superseded
    let d1_fresh: serde_json::Value = c
        .get(format!("{}/decisions/{}", base, d1_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(d1_fresh["status"], "superseded");

    // 16. knowledge — rag + reference
    let _k_rag: serde_json::Value = c
        .post(format!("{}/knowledge", base))
        .json(&serde_json::json!({
            "project_id": project_id,
            "scope": "rag",
            "kind": "decision",
            "title": "Token strategy",
            "body": "Decided to use opaque tokens after JWT replay risk surfaced.",
            "tags": ["auth","tokens"]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let _k_ref: serde_json::Value = c
        .post(format!("{}/knowledge", base))
        .json(&serde_json::json!({
            "project_id": project_id,
            "scope": "reference",
            "kind": "runbook",
            "title": "Auth runbook",
            "body": "Manual procedures for the on-call.",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // 17. scope filter — listing with scope=rag should NOT include the reference row.
    let rag_only: serde_json::Value = c
        .get(format!("{}/knowledge", base))
        .query(&[("project_id", project_id.as_str()), ("scope", "rag")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let arr = rag_only["knowledge"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["scope"], "rag");

    let all: serde_json::Value = c
        .get(format!("{}/knowledge", base))
        .query(&[("project_id", project_id.as_str())])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(all["knowledge"].as_array().unwrap().len(), 2);
}
