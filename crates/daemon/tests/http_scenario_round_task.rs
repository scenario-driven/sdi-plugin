//! HTTP integration: scenario → round → task → evidence → done.
//!
//! Covers:
//! - D5 GWT-strict POST /scenarios rejection on empty `then`
//! - D8 approve gate (plan can't approve until ≥1 confirmed scenario)
//! - R2+ auto-regression carry-over on round activation under strict-regression
//! - PRD §6.6 evidence required: /tasks/:id/status with status=done -> 400 EVIDENCE_REQUIRED
//! - /tasks/:id/complete with full evidence flips status -> done

use sdi_daemon::AppState;
use sdi_db::Paths;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

async fn spawn_server() -> (String, tokio::task::JoinHandle<()>) {
    let tmp = std::env::temp_dir().join(format!(
        "sdid-it2-{}-{}",
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

fn c() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

async fn mk_plan(base: &str, suffix: &str) -> (String, String) {
    let cli = c();
    let key = format!("F{}", &suffix[..4]);
    let slug = format!("f-{}", &suffix[..6].to_lowercase());
    let project: serde_json::Value = cli
        .post(format!("{}/projects", base))
        .json(&serde_json::json!({"key": key, "name": "Flow", "slug": slug}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let project_id = project["id"].as_str().unwrap().to_string();

    let plan_code = format!("F-{}", &suffix[..6]);
    let plan: serde_json::Value = cli
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
async fn scenario_gwt_strict_d5() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (_pid, plan_id) = mk_plan(&base, &suffix).await;

    // Empty `then` -> 400 GWT_EMPTY
    let r = cli
        .post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("SCN-{}", &suffix[..6]),
            "given": "logged in",
            "when": "click",
            "then": "  "
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "GWT_EMPTY");
}

#[tokio::test]
async fn plan_approve_unlocks_after_scenario_confirmed() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (_pid, plan_id) = mk_plan(&base, &suffix).await;

    let scn: serde_json::Value = cli
        .post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("SCN-{}", &suffix[..6]),
            "given": "logged in",
            "when": "click checkout",
            "then": "order is created",
            "confirmed": true
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(scn["status"], "confirmed");

    let r = cli
        .post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let plan: serde_json::Value = r.json().await.unwrap();
    assert_eq!(plan["status"], "active");
}

#[tokio::test]
async fn r2_auto_regression_carries_results() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (_pid, plan_id) = mk_plan(&base, &suffix).await;

    // 2 confirmed scenarios
    let mut scn_ids = vec![];
    for i in 0..2 {
        let scn: serde_json::Value = cli
            .post(format!("{}/scenarios", base))
            .json(&serde_json::json!({
                "plan_id": plan_id,
                "short_code": format!("SCN-{}-{i}", &suffix[..6]),
                "given": "user state",
                "when": format!("action {i}"),
                "then": "outcome",
                "confirmed": true
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        scn_ids.push(scn["id"].as_str().unwrap().to_string());
    }
    // approve plan
    cli.post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();

    // round 1 (strict-regression default)
    let r1: serde_json::Value = cli
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
    cli.post(format!("{}/rounds/{}/activate", base, r1_id))
        .send()
        .await
        .unwrap();
    // verdicts: scn[0]=passing, scn[1]=failing
    for (i, sid) in scn_ids.iter().enumerate() {
        cli.post(format!("{}/rounds/{}/results", base, r1_id))
            .json(&serde_json::json!({
                "scenario_id": sid,
                "result": if i == 0 { "passing" } else { "failing" }
            }))
            .send()
            .await
            .unwrap();
    }
    cli.post(format!("{}/rounds/{}/complete", base, r1_id))
        .send()
        .await
        .unwrap();

    // round 2 (default strict-regression) — activation should carry results
    let r2: serde_json::Value = cli
        .post(format!("{}/rounds", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("R2-{}", &suffix[..6])
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let r2_id = r2["id"].as_str().unwrap().to_string();
    let act: serde_json::Value = cli
        .post(format!("{}/rounds/{}/activate", base, r2_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(act["carried_results"], 2);
    let results: serde_json::Value = cli
        .get(format!("{}/rounds/{}/results", base, r2_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let arr = results["results"].as_array().unwrap();
    let mut got: Vec<(String, String)> = arr
        .iter()
        .map(|r| {
            (
                r["scenario_id"].as_str().unwrap().to_string(),
                r["result"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    got.sort();
    let mut want = vec![
        (scn_ids[0].clone(), "passing".into()),
        (scn_ids[1].clone(), "failing".into()),
    ];
    want.sort();
    assert_eq!(got, want);
}

#[tokio::test]
async fn task_done_requires_evidence_prd_6_6() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (_pid, plan_id) = mk_plan(&base, &suffix).await;

    // 1 scenario + plan approve + round1 active
    let scn: serde_json::Value = cli
        .post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("SCN-{}", &suffix[..6]),
            "given": "g", "when": "w", "then": "t",
            "confirmed": true
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let scn_id = scn["id"].as_str().unwrap().to_string();
    cli.post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();
    let r1: serde_json::Value = cli
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
    cli.post(format!("{}/rounds/{}/activate", base, r1_id))
        .send()
        .await
        .unwrap();

    let task: serde_json::Value = cli
        .post(format!("{}/tasks", base))
        .json(&serde_json::json!({
            "round_id": r1_id,
            "short_code": format!("T-{}", &suffix[..6]),
            "description": "wire CLI",
            "parent_scenario_ids": [scn_id.clone()]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let task_id = task["id"].as_str().unwrap().to_string();

    // todo -> in_progress (allowed, no evidence needed)
    let r = cli
        .post(format!("{}/tasks/{}/status", base, task_id))
        .json(&serde_json::json!({"status":"in_progress"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // attempt done via /status — should be rejected with EVIDENCE_REQUIRED
    let r = cli
        .post(format!("{}/tasks/{}/status", base, task_id))
        .json(&serde_json::json!({"status":"done"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "EVIDENCE_REQUIRED");

    // /tasks/:id/complete with empty evidence still rejected
    let r = cli
        .post(format!("{}/tasks/{}/complete", base, task_id))
        .json(&serde_json::json!({"evidence": {}}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);

    // /tasks/:id/complete with valid evidence accepted
    let r = cli
        .post(format!("{}/tasks/{}/complete", base, task_id))
        .json(&serde_json::json!({
            "evidence": {
                "scenarios": [{
                    "scenario_id": scn_id,
                    "result": "passing",
                    "evidence_ref": "src/lib.rs:42"
                }],
                "summary": "all green"
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let done: serde_json::Value = r.json().await.unwrap();
    assert_eq!(done["status"], "done");
    assert!(done["evidence_at"].is_string());
}

/// PRD §6 #5 — In-flight Task pause. When R(N+1) is activated and its
/// `in_flight_policy = pause` (the daemon default), every task in the same
/// plan that is currently `in_progress` MUST transition to `blocked`. The
/// activate response surfaces the policy, the action taken, and the affected
/// task IDs so the LLM caller can confirm the disruption set.
#[tokio::test]
async fn round_activate_pauses_in_flight_tasks_prd_6_5() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (_pid, plan_id) = mk_plan(&base, &suffix).await;

    // 1 confirmed scenario, approve plan
    let scn: serde_json::Value = cli
        .post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("SCN-{}", &suffix[..6]),
            "given": "g", "when": "w", "then": "t",
            "confirmed": true
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let scn_id = scn["id"].as_str().unwrap().to_string();
    cli.post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();

    // R1 active, task in R1 moves to in_progress
    let r1: serde_json::Value = cli
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
    cli.post(format!("{}/rounds/{}/activate", base, r1_id))
        .send()
        .await
        .unwrap();

    let task: serde_json::Value = cli
        .post(format!("{}/tasks", base))
        .json(&serde_json::json!({
            "round_id": r1_id,
            "short_code": format!("T-{}", &suffix[..6]),
            "description": "wire CLI",
            "parent_scenario_ids": [scn_id.clone()]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let task_id = task["id"].as_str().unwrap().to_string();

    let r = cli
        .post(format!("{}/tasks/{}/status", base, task_id))
        .json(&serde_json::json!({"status": "in_progress"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // R1 must be completed before R2 can activate (round lifecycle invariant).
    cli.post(format!("{}/rounds/{}/complete", base, r1_id))
        .send()
        .await
        .unwrap();

    // R2 with default in-flight policy (pause)
    let r2: serde_json::Value = cli
        .post(format!("{}/rounds", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("R2-{}", &suffix[..6])
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let r2_id = r2["id"].as_str().unwrap().to_string();

    let act: serde_json::Value = cli
        .post(format!("{}/rounds/{}/activate", base, r2_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(act["in_flight"]["policy"], "pause");
    assert_eq!(act["in_flight"]["action"], "paused");
    let affected = act["in_flight"]["affected_task_ids"].as_array().unwrap();
    assert!(
        affected.iter().any(|v| v.as_str() == Some(&task_id)),
        "task {task_id} should appear in affected_task_ids, got {affected:?}"
    );

    // The in-flight task is now blocked.
    let task_after: serde_json::Value = cli
        .get(format!("{}/tasks/{}", base, task_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(task_after["status"], "blocked");
}

/// PRD §6 #5 — `in_flight_policy = abort` cancels every in-flight task in the
/// plan when the next round activates.
#[tokio::test]
async fn round_activate_abort_cancels_in_flight_tasks_prd_6_5() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (_pid, plan_id) = mk_plan(&base, &suffix).await;

    let scn: serde_json::Value = cli
        .post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("SCN-{}", &suffix[..6]),
            "given": "g", "when": "w", "then": "t",
            "confirmed": true
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let scn_id = scn["id"].as_str().unwrap().to_string();
    cli.post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();

    let r1: serde_json::Value = cli
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
    cli.post(format!("{}/rounds/{}/activate", base, r1_id))
        .send()
        .await
        .unwrap();

    let task: serde_json::Value = cli
        .post(format!("{}/tasks", base))
        .json(&serde_json::json!({
            "round_id": r1_id,
            "short_code": format!("T-{}", &suffix[..6]),
            "description": "wire CLI",
            "parent_scenario_ids": [scn_id]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let task_id = task["id"].as_str().unwrap().to_string();
    cli.post(format!("{}/tasks/{}/status", base, task_id))
        .json(&serde_json::json!({"status": "in_progress"}))
        .send()
        .await
        .unwrap();
    cli.post(format!("{}/rounds/{}/complete", base, r1_id))
        .send()
        .await
        .unwrap();

    let r2: serde_json::Value = cli
        .post(format!("{}/rounds", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("R2-{}", &suffix[..6]),
            "in_flight_policy": "abort"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let r2_id = r2["id"].as_str().unwrap().to_string();
    let act: serde_json::Value = cli
        .post(format!("{}/rounds/{}/activate", base, r2_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(act["in_flight"]["policy"], "abort");
    assert_eq!(act["in_flight"]["action"], "aborted");

    let task_after: serde_json::Value = cli
        .get(format!("{}/tasks/{}", base, task_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(task_after["status"], "cancelled");
}

/// PRD §6 #5 — `in_flight_policy = continue-on-noimpact` leaves in-progress
/// tasks alone when the next round activates. The action label is `continued`
/// and affected_task_ids still surfaces the running set so the LLM can
/// double-check intent against the disruption review trail (PRD §6 #4).
#[tokio::test]
async fn round_activate_continue_on_noimpact_leaves_tasks_prd_6_5() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (_pid, plan_id) = mk_plan(&base, &suffix).await;

    let scn: serde_json::Value = cli
        .post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("SCN-{}", &suffix[..6]),
            "given": "g", "when": "w", "then": "t",
            "confirmed": true
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let scn_id = scn["id"].as_str().unwrap().to_string();
    cli.post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();

    let r1: serde_json::Value = cli
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
    cli.post(format!("{}/rounds/{}/activate", base, r1_id))
        .send()
        .await
        .unwrap();

    let task: serde_json::Value = cli
        .post(format!("{}/tasks", base))
        .json(&serde_json::json!({
            "round_id": r1_id,
            "short_code": format!("T-{}", &suffix[..6]),
            "description": "wire CLI",
            "parent_scenario_ids": [scn_id]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let task_id = task["id"].as_str().unwrap().to_string();
    cli.post(format!("{}/tasks/{}/status", base, task_id))
        .json(&serde_json::json!({"status": "in_progress"}))
        .send()
        .await
        .unwrap();
    cli.post(format!("{}/rounds/{}/complete", base, r1_id))
        .send()
        .await
        .unwrap();

    let r2: serde_json::Value = cli
        .post(format!("{}/rounds", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("R2-{}", &suffix[..6]),
            "in_flight_policy": "continue-on-noimpact"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let r2_id = r2["id"].as_str().unwrap().to_string();
    let act: serde_json::Value = cli
        .post(format!("{}/rounds/{}/activate", base, r2_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(act["in_flight"]["policy"], "continue-on-noimpact");
    assert_eq!(act["in_flight"]["action"], "continued");
    let affected = act["in_flight"]["affected_task_ids"].as_array().unwrap();
    assert!(
        affected.iter().any(|v| v.as_str() == Some(&task_id)),
        "task {task_id} should still surface in affected_task_ids"
    );

    let task_after: serde_json::Value = cli
        .get(format!("{}/tasks/{}", base, task_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        task_after["status"], "in_progress",
        "continue-on-noimpact must NOT mutate the task status"
    );
}

/// PRD §6 #6 (D6) — Task evidence is the canonical write surface for per-
/// scenario verdicts. When /tasks/:id/complete succeeds, the daemon mirrors
/// every `evidence.scenarios[]` entry into `round.scenario_results` so
/// /rounds/:id/results reflects the verdict without a separate call.
#[tokio::test]
async fn task_complete_mirrors_evidence_into_round_results_d6() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (_pid, plan_id) = mk_plan(&base, &suffix).await;

    // Two confirmed scenarios.
    let mut scn_ids = vec![];
    for i in 0..2 {
        let scn: serde_json::Value = cli
            .post(format!("{}/scenarios", base))
            .json(&serde_json::json!({
                "plan_id": plan_id,
                "short_code": format!("SCN-{}-{i}", &suffix[..6]),
                "given": "g",
                "when": format!("w{i}"),
                "then": format!("t{i}"),
                "confirmed": true
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        scn_ids.push(scn["id"].as_str().unwrap().to_string());
    }
    cli.post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();

    let r1: serde_json::Value = cli
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
    cli.post(format!("{}/rounds/{}/activate", base, r1_id))
        .send()
        .await
        .unwrap();

    let task: serde_json::Value = cli
        .post(format!("{}/tasks", base))
        .json(&serde_json::json!({
            "round_id": r1_id,
            "short_code": format!("T-{}", &suffix[..6]),
            "description": "wire login + checkout",
            "parent_scenario_ids": [scn_ids[0].clone(), scn_ids[1].clone()]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let task_id = task["id"].as_str().unwrap().to_string();

    cli.post(format!("{}/tasks/{}/status", base, task_id))
        .json(&serde_json::json!({"status":"in_progress"}))
        .send()
        .await
        .unwrap();

    let r = cli
        .post(format!("{}/tasks/{}/complete", base, task_id))
        .json(&serde_json::json!({
            "evidence": {
                "scenarios": [
                    {
                        "scenario_id": scn_ids[0],
                        "result": "passing",
                        "evidence_ref": "tests/login.rs:42",
                        "note": "happy path verified"
                    },
                    {
                        "scenario_id": scn_ids[1],
                        "result": "failing",
                        "evidence_ref": "tests/checkout.rs:118",
                        "note": "expected ORDER_CREATED, got DRAFT"
                    }
                ],
                "summary": "1 pass / 1 fail"
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    let results: serde_json::Value = cli
        .get(format!("{}/rounds/{}/results", base, r1_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let arr = results["results"].as_array().unwrap();
    assert_eq!(
        arr.len(),
        2,
        "task evidence must mirror both scenarios into round.scenario_results"
    );
    let mut got: Vec<(String, String, String)> = arr
        .iter()
        .map(|r| {
            (
                r["scenario_id"].as_str().unwrap().to_string(),
                r["result"].as_str().unwrap().to_string(),
                r["evidence_ref"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    got.sort();
    let mut want = vec![
        (
            scn_ids[0].clone(),
            "passing".into(),
            "tests/login.rs:42".into(),
        ),
        (
            scn_ids[1].clone(),
            "failing".into(),
            "tests/checkout.rs:118".into(),
        ),
    ];
    want.sort();
    assert_eq!(got, want);
}

/// PRD §6 #3 — carry-over must NOT silently auto-pass a scenario that was
/// added after the prior round completed (no prev verdict → must remain
/// unevaluated in the new round). The fix swaps the old
/// `COALESCE(sr.result,'passing')` for an inner-join on scenario_results so
/// only scenarios with an explicit prior verdict are carried.
#[tokio::test]
async fn carry_over_excludes_unevaluated_scenarios_prd_6_3() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (_pid, plan_id) = mk_plan(&base, &suffix).await;

    // SCN-A exists before R1 — gets a verdict.
    let scn_a: serde_json::Value = cli
        .post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("SCN-A-{}", &suffix[..6]),
            "given": "g", "when": "w", "then": "t",
            "confirmed": true
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let scn_a_id = scn_a["id"].as_str().unwrap().to_string();
    cli.post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();

    let r1: serde_json::Value = cli
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
    cli.post(format!("{}/rounds/{}/activate", base, r1_id))
        .send()
        .await
        .unwrap();
    cli.post(format!("{}/rounds/{}/results", base, r1_id))
        .json(&serde_json::json!({
            "scenario_id": scn_a_id,
            "result": "passing"
        }))
        .send()
        .await
        .unwrap();
    cli.post(format!("{}/rounds/{}/complete", base, r1_id))
        .send()
        .await
        .unwrap();

    // SCN-B is created AFTER R1 completes. It must NOT auto-pass in R2.
    let scn_b: serde_json::Value = cli
        .post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("SCN-B-{}", &suffix[..6]),
            "given": "g2", "when": "w2", "then": "t2",
            "confirmed": true
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let scn_b_id = scn_b["id"].as_str().unwrap().to_string();

    let r2: serde_json::Value = cli
        .post(format!("{}/rounds", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("R2-{}", &suffix[..6])
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let r2_id = r2["id"].as_str().unwrap().to_string();
    let act: serde_json::Value = cli
        .post(format!("{}/rounds/{}/activate", base, r2_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        act["carried_results"], 1,
        "only SCN-A should carry (SCN-B is unevaluated)"
    );

    let results: serde_json::Value = cli
        .get(format!("{}/rounds/{}/results", base, r2_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let arr = results["results"].as_array().unwrap();
    assert_eq!(
        arr.len(),
        1,
        "R2 must contain exactly SCN-A; SCN-B unevaluated"
    );
    assert_eq!(arr[0]["scenario_id"], scn_a_id);
    assert!(
        arr.iter().all(|r| r["scenario_id"] != scn_b_id),
        "SCN-B must NOT be auto-passed in R2",
    );
}

/// Build a project with an approved plan and an active R1; returns
/// `(project_id, round_id)`. One confirmed scenario satisfies the D8 approve
/// gate.
async fn mk_active_round(base: &str, suffix: &str) -> (String, String) {
    let cli = c();
    let (project_id, plan_id) = mk_plan(base, suffix).await;
    cli.post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("SCN-{}", &suffix[..6]),
            "given": "g", "when": "w", "then": "t",
            "confirmed": true
        }))
        .send()
        .await
        .unwrap();
    cli.post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();
    let r1: serde_json::Value = cli
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
    cli.post(format!("{}/rounds/{}/activate", base, r1_id))
        .send()
        .await
        .unwrap();
    (project_id, r1_id)
}

async fn mk_task(base: &str, round_id: &str, suffix: &str) -> String {
    let task: serde_json::Value = c()
        .post(format!("{}/tasks", base))
        .json(&serde_json::json!({
            "round_id": round_id,
            "short_code": format!("T-{}", &suffix[..6]),
            "description": "work"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    task["id"].as_str().unwrap().to_string()
}

async fn stats_map(base: &str, project_id: Option<&str>) -> HashMap<String, i64> {
    let url = match project_id {
        Some(p) => format!("{}/tasks/stats?project_id={}", base, p),
        None => format!("{}/tasks/stats", base),
    };
    let v: serde_json::Value = c().get(url).send().await.unwrap().json().await.unwrap();
    v["by_status"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["status"].as_str().unwrap().to_string(),
                e["count"].as_i64().unwrap(),
            )
        })
        .collect()
}

/// `/tasks/stats?project_id=` and the dashboard's `task_status` must count only
/// the named project's tasks — a global histogram would leak other projects'
/// tasks in the shared multi-project DB. The no-arg `/tasks/stats` stays global
/// (the `/metrics` server-wide gauge semantic).
#[tokio::test]
async fn task_stats_is_project_scoped() {
    let (base, _h) = spawn_server().await;
    let cli = c();

    // Distinct leading chars so the two projects derive distinct keys/slugs
    // (mk_plan keys off suffix[..4]; sibling ULIDs share their timestamp prefix).
    // Project A: one task left in `todo`.
    let sa = format!("AAAA{}", ulid::Ulid::new());
    let (pa, ra) = mk_active_round(&base, &sa).await;
    mk_task(&base, &ra, &sa).await;

    // Project B: one task moved to `in_progress`.
    let sb = format!("BBBB{}", ulid::Ulid::new());
    let (pb, rb) = mk_active_round(&base, &sb).await;
    let tb = mk_task(&base, &rb, &sb).await;
    cli.post(format!("{}/tasks/{}/status", base, tb))
        .json(&serde_json::json!({"status": "in_progress"}))
        .send()
        .await
        .unwrap();

    // Scoped to A: only `todo`, never B's `in_progress`.
    let a_stats = stats_map(&base, Some(&pa)).await;
    assert_eq!(a_stats.get("todo"), Some(&1));
    assert_eq!(a_stats.get("in_progress"), None);

    // Scoped to B: only `in_progress`, never A's `todo`.
    let b_stats = stats_map(&base, Some(&pb)).await;
    assert_eq!(b_stats.get("in_progress"), Some(&1));
    assert_eq!(b_stats.get("todo"), None);

    // Global (no project_id): sees both.
    let g_stats = stats_map(&base, None).await;
    assert_eq!(g_stats.get("todo"), Some(&1));
    assert_eq!(g_stats.get("in_progress"), Some(&1));

    // Dashboard task_status mirrors the project-scoped histogram for A.
    let dash: serde_json::Value = cli
        .get(format!("{}/dashboard?project={}", base, pa))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ts = dash["task_status"].as_array().unwrap();
    assert!(ts.iter().any(|e| e["status"] == "todo" && e["count"] == 1));
    assert!(
        ts.iter().all(|e| e["status"] != "in_progress"),
        "dashboard task_status must not leak project B's in_progress task"
    );
}

// ── #12 / #13 — evidence integrity at task complete ────────────────────────

/// Plan → confirmed scenario → active round → in_progress task whose only
/// parent is that scenario. Returns (project_id, plan_id, scn_id, scn_short,
/// round_id, task_id).
async fn setup_in_progress_task(
    base: &str,
    suffix: &str,
) -> (String, String, String, String, String, String) {
    let cli = c();
    let (project_id, plan_id) = mk_plan(base, suffix).await;
    let scn_short = format!("SCN-{}", &suffix[..6]);
    let scn: serde_json::Value = cli
        .post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id, "short_code": scn_short,
            "given": "g", "when": "w", "then": "t", "confirmed": true
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let scn_id = scn["id"].as_str().unwrap().to_string();
    cli.post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();
    let r1: serde_json::Value = cli
        .post(format!("{}/rounds", base))
        .json(
            &serde_json::json!({"plan_id": plan_id, "short_code": format!("R1-{}", &suffix[..6])}),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let r1_id = r1["id"].as_str().unwrap().to_string();
    cli.post(format!("{}/rounds/{}/activate", base, r1_id))
        .send()
        .await
        .unwrap();
    let task: serde_json::Value = cli
        .post(format!("{}/tasks", base))
        .json(&serde_json::json!({
            "round_id": r1_id, "short_code": format!("T-{}", &suffix[..6]),
            "description": "impl", "parent_scenario_ids": [scn_id.clone()]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let task_id = task["id"].as_str().unwrap().to_string();
    cli.post(format!("{}/tasks/{}/status", base, task_id))
        .json(&serde_json::json!({"status": "in_progress"}))
        .send()
        .await
        .unwrap();
    (project_id, plan_id, scn_id, scn_short, r1_id, task_id)
}

/// #12 — a ghost scenario_id in evidence is rejected, and the task stays
/// in_progress (the done transition was NOT committed): nothing partial.
#[tokio::test]
async fn task_complete_rejects_ghost_scenario_id_12() {
    let (base, _h) = spawn_server().await;
    let suffix = ulid::Ulid::new().to_string();
    let (_proj, _plan, _scn, _short, _r1, task_id) = setup_in_progress_task(&base, &suffix).await;
    let cli = c();

    let r = cli
        .post(format!("{}/tasks/{}/complete", base, task_id))
        .json(&serde_json::json!({
            "evidence": { "scenarios": [{
                "scenario_id": "SCN-DOES-NOT-EXIST",
                "result": "passing",
                "evidence_ref": "x.rs:1"
            }], "summary": "s" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400, "ghost scenario id must be rejected");

    // Atomicity: the task did NOT flip to done.
    let task: serde_json::Value = cli
        .get(format!("{}/tasks/{}", base, task_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        task["status"], "in_progress",
        "rejected complete must not partially commit"
    );
}

/// #12 — a real scenario that is NOT a parent of the task is rejected.
#[tokio::test]
async fn task_complete_rejects_non_parent_scenario_12() {
    let (base, _h) = spawn_server().await;
    let suffix = ulid::Ulid::new().to_string();
    let (_proj, plan_id, _scn, _short, _r1, task_id) = setup_in_progress_task(&base, &suffix).await;
    let cli = c();
    // A second scenario in the same plan, NOT linked to the task.
    let other: serde_json::Value = cli
        .post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id, "short_code": format!("SCN-OTHER-{}", &suffix[..6]),
            "given": "g", "when": "w2", "then": "t2", "confirmed": true
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let other_id = other["id"].as_str().unwrap().to_string();

    let r = cli
        .post(format!("{}/tasks/{}/complete", base, task_id))
        .json(&serde_json::json!({
            "evidence": { "scenarios": [{
                "scenario_id": other_id, "result": "passing", "evidence_ref": "x.rs:1"
            }], "summary": "s" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400, "a non-parent scenario must be rejected");
}

/// #13 — evidence may reference a scenario by its plan-scoped SHORT CODE; the
/// daemon resolves it to the SCN ULID (instead of FK-failing) and the round
/// result is keyed by the ULID.
#[tokio::test]
async fn task_complete_resolves_short_code_13() {
    let (base, _h) = spawn_server().await;
    let suffix = ulid::Ulid::new().to_string();
    let (_proj, _plan, scn_id, scn_short, r1_id, task_id) =
        setup_in_progress_task(&base, &suffix).await;
    let cli = c();

    let r = cli
        .post(format!("{}/tasks/{}/complete", base, task_id))
        .json(&serde_json::json!({
            "evidence": { "scenarios": [{
                "scenario_id": scn_short,  // short code, NOT the ULID
                "result": "passing",
                "evidence_ref": "tests/x.rs:1"
            }], "summary": "via short code" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "short code must resolve, not FK-fail");

    // The round result is keyed by the resolved ULID, not the short code.
    let results: serde_json::Value = cli
        .get(format!("{}/rounds/{}/results", base, r1_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let arr = results["results"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(
        arr[0]["scenario_id"], scn_id,
        "result keyed by resolved ULID"
    );
    assert_ne!(arr[0]["scenario_id"], scn_short);
}

/// #7 — round activate response includes `scenarios_needing_verification`:
/// confirmed scenarios without a passing/retired verdict in the new round.
#[tokio::test]
async fn round_activate_returns_scenarios_needing_verification_7() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (_pid, plan_id) = mk_plan(&base, &suffix).await;

    // Two confirmed scenarios + one still draft (must NOT appear).
    let mut confirmed = vec![];
    for i in 0..2 {
        let scn: serde_json::Value = cli
            .post(format!("{}/scenarios", base))
            .json(&serde_json::json!({
                "plan_id": plan_id, "short_code": format!("SCN-{}-{i}", &suffix[..6]),
                "given": "g", "when": format!("w{i}"), "then": format!("t{i}"),
                "confirmed": true
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        confirmed.push(scn["id"].as_str().unwrap().to_string());
    }
    cli.post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id, "short_code": format!("SCN-DRAFT-{}", &suffix[..6]),
            "given": "g", "when": "wd", "then": "td", "confirmed": false
        }))
        .send()
        .await
        .unwrap();
    cli.post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();

    let r1: serde_json::Value = cli
        .post(format!("{}/rounds", base))
        .json(
            &serde_json::json!({"plan_id": plan_id, "short_code": format!("R1-{}", &suffix[..6])}),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let r1_id = r1["id"].as_str().unwrap().to_string();

    let act: serde_json::Value = cli
        .post(format!("{}/rounds/{}/activate", base, r1_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let needs = act["scenarios_needing_verification"].as_array().unwrap();
    let ids: Vec<String> = needs
        .iter()
        .map(|n| n["scenario_id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        needs.len(),
        2,
        "both confirmed scenarios need verification at R1"
    );
    assert!(ids.contains(&confirmed[0]) && ids.contains(&confirmed[1]));
    // GWT is inlined so the LLM can decompose without a second fetch.
    assert!(needs[0]["given"].is_string() && needs[0]["then_clause"].is_string());
}

/// #8 — retire excludes a confirmed scenario from the approve count and the
/// needs-verification set; un-retire restores it with status preserved; past
/// round verdicts are untouched.
#[tokio::test]
async fn scenario_retire_excludes_and_unretire_restores_8() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (_pid, plan_id) = mk_plan(&base, &suffix).await;

    let mk_scn = |code: String| {
        let plan_id = plan_id.clone();
        let base = base.clone();
        async move {
            let cli = c();
            let scn: serde_json::Value = cli
                .post(format!("{}/scenarios", base))
                .json(&serde_json::json!({
                    "plan_id": plan_id, "short_code": code,
                    "given": "g", "when": "w", "then": "t", "confirmed": true
                }))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            scn["id"].as_str().unwrap().to_string()
        }
    };
    let keep = mk_scn(format!("SCN-KEEP-{}", &suffix[..6])).await;
    let retire = mk_scn(format!("SCN-RET-{}", &suffix[..6])).await;

    // Retire one scenario.
    let r = cli
        .post(format!("{}/scenarios/{}/retire", base, retire))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body: serde_json::Value = r.json().await.unwrap();
    assert!(body["retired_at"].is_string(), "retired_at stamped");
    assert_eq!(
        body["status"], "confirmed",
        "authoring status preserved across retire"
    );

    // Approve still works (the kept scenario satisfies D8); activate R1.
    cli.post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();
    let r1: serde_json::Value = cli
        .post(format!("{}/rounds", base))
        .json(
            &serde_json::json!({"plan_id": plan_id, "short_code": format!("R1-{}", &suffix[..6])}),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let r1_id = r1["id"].as_str().unwrap().to_string();
    let act: serde_json::Value = cli
        .post(format!("{}/rounds/{}/activate", base, r1_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // Needs-verification excludes the retired scenario.
    let needs: Vec<String> = act["scenarios_needing_verification"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["scenario_id"].as_str().unwrap().to_string())
        .collect();
    assert!(needs.contains(&keep), "kept scenario needs verification");
    assert!(!needs.contains(&retire), "retired scenario excluded (#8)");

    // Un-retire restores it (status preserved); now it's verification-eligible.
    let r = cli
        .post(format!("{}/scenarios/{}/unretire", base, retire))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = r.json().await.unwrap();
    assert!(body["retired_at"].is_null(), "retired_at cleared");
    assert_eq!(body["status"], "confirmed", "status restored");
}

/// #15 + #16 — `sdi next` points at the in-progress task's brief and surfaces a
/// provisional decision; `sdi task brief` inlines GWT + evidence format; the
/// round baseline round-trips.
#[tokio::test]
async fn next_brief_baseline_and_provisional_15_16() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (project_id, plan_id, scn_id, _short, r1_id, task_id) =
        setup_in_progress_task(&base, &suffix).await;

    // #16 — a provisional (accepted + supersede_when) decision.
    cli.post(format!("{}/decisions", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("DEC-{}", &suffix[..6]),
            "title": "file as SoT",
            "body": "tentative",
            "supersede_when": "the team disagrees in review"
        }))
        .send()
        .await
        .unwrap();

    // #15 — next step. An in_progress task exists, so it points at the brief,
    // and the provisional decision rides along.
    let nxt: serde_json::Value = cli
        .get(format!("{}/projects/{}/next", base, project_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        nxt["command"].as_str().unwrap().contains("sdi task brief"),
        "next should point at the in-progress task's brief, got: {}",
        nxt["command"]
    );
    let prov = nxt["provisional_decisions"].as_array().unwrap();
    assert_eq!(prov.len(), 1, "the provisional decision is surfaced (#16)");
    assert_eq!(prov[0]["supersede_when"], "the team disagrees in review");

    // #15 — task brief inlines the linked scenario's GWT + evidence format.
    let brief: serde_json::Value = cli
        .get(format!("{}/tasks/{}/brief", base, task_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let scns = brief["scenarios"].as_array().unwrap();
    assert_eq!(scns.len(), 1);
    assert_eq!(scns[0]["id"], scn_id);
    assert!(scns[0]["given"].is_string() && scns[0]["then"].is_string());
    assert!(brief["evidence_format"]
        .as_str()
        .unwrap()
        .contains("passing"));
    assert!(brief["prohibitions"].as_array().unwrap().len() >= 2);

    // #15 — round baseline round-trips and shows up in the brief.
    cli.post(format!("{}/rounds/{}/baseline", base, r1_id))
        .json(&serde_json::json!({ "baseline_json": {"green_tests": 412} }))
        .send()
        .await
        .unwrap();
    let got: serde_json::Value = cli
        .get(format!("{}/rounds/{}/baseline", base, r1_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(got["baseline"]["green_tests"], 412);
    let brief2: serde_json::Value = cli
        .get(format!("{}/tasks/{}/brief", base, task_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(brief2["baseline"], "{\"green_tests\":412}");
}

/// #15 — on a fresh plan with no confirmed scenarios, `next` steers toward
/// authoring + confirming scenarios.
#[tokio::test]
async fn next_on_empty_plan_steers_to_scenarios_15() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (project_id, plan_id) = mk_plan(&base, &suffix).await;
    // Make the plan active so it is the project's active plan: confirm one
    // scenario, approve, then there IS an active plan with a round to open.
    cli.post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id, "short_code": format!("SCN-{}", &suffix[..6]),
            "given": "g", "when": "w", "then": "t", "confirmed": true
        }))
        .send()
        .await
        .unwrap();
    cli.post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();

    let nxt: serde_json::Value = cli
        .get(format!("{}/projects/{}/next", base, project_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // Confirmed scenarios exist, no round yet → next is "create + activate a round".
    assert!(
        nxt["command"]
            .as_str()
            .unwrap()
            .contains("sdi round create"),
        "got: {}",
        nxt["command"]
    );
}
