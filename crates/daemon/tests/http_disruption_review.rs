//! PRD §6 #4 — Disruption needs-review enforcement (HTTP integration).
//!
//! Asserts:
//! 1. POST /disruption-reviews with empty impacted_scenario_ids -> 400.
//! 2. A pending review on a plan blocks /rounds/:id/activate with
//!    `DISRUPTION_PENDING`.
//! 3. After POST /disruption-reviews/:id/resolve, activation succeeds.
//! 4. The list endpoint filters by status.

use sdi_daemon::AppState;
use sdi_db::Paths;
use std::sync::Arc;
use std::time::Duration;

async fn spawn_server() -> (String, tokio::task::JoinHandle<()>) {
    let tmp = std::env::temp_dir().join(format!(
        "sdid-drv-{}-{}",
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

fn c() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

/// Build a project + plan + 1 confirmed scenario + approve the plan + create
/// a fresh round in `planning`. Returns (plan_id, scenario_id, round_id).
async fn bootstrap(base: &str, suffix: &str) -> (String, String, String) {
    let cli = c();
    let key = format!("D{}", &suffix[..4]);
    let slug = format!("d-{}", suffix[..6].to_lowercase());
    let proj: serde_json::Value = cli
        .post(format!("{}/projects", base))
        .json(&serde_json::json!({"key": key, "name": "DR", "slug": slug}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let project_id = proj["id"].as_str().unwrap().to_string();
    let plan_code = format!("D-{}", &suffix[..6]);
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
    (plan_id, scn_id, r1_id)
}

#[tokio::test]
async fn create_rejects_empty_impacted_list() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (plan_id, _scn_id, _r1_id) = bootstrap(&base, &suffix).await;

    let r = cli
        .post(format!("{}/disruption-reviews", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "source_kind": "scenario",
            "source_id": "SCN-new",
            "impacted_scenario_ids": []
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "VALIDATION");
}

#[tokio::test]
async fn pending_review_blocks_round_activation_prd_6_4() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (plan_id, scn_id, r1_id) = bootstrap(&base, &suffix).await;

    // Record a disruption: a hypothetical new scenario impacts the existing one.
    let drv: serde_json::Value = cli
        .post(format!("{}/disruption-reviews", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "source_kind": "scenario",
            "source_id": "SCN-hypothetical-new",
            "impacted_scenario_ids": [scn_id],
            "note": "new flow shadows existing checkout"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let drv_id = drv["id"].as_str().unwrap().to_string();
    assert_eq!(drv["status"], "pending");

    // Activation is blocked.
    let r = cli
        .post(format!("{}/rounds/{}/activate", base, r1_id))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "DISRUPTION_PENDING");

    // Resolve with retire verdict.
    let r = cli
        .post(format!("{}/disruption-reviews/{}/resolve", base, drv_id))
        .json(&serde_json::json!({
            "resolution": "retire",
            "note": "scenario superseded by new flow"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let after: serde_json::Value = r.json().await.unwrap();
    assert_eq!(after["status"], "resolved");
    assert_eq!(after["resolution"], "retire");

    // Activation now succeeds.
    let r = cli
        .post(format!("{}/rounds/{}/activate", base, r1_id))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
}

#[tokio::test]
async fn list_filters_pending_and_all() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let (plan_id, scn_id, _r1_id) = bootstrap(&base, &suffix).await;

    let mk = |source: &str, source_id: &str| {
        let plan_id = plan_id.clone();
        let scn_id = scn_id.clone();
        let cli = cli.clone();
        let base = base.clone();
        let source = source.to_string();
        let source_id = source_id.to_string();
        async move {
            cli.post(format!("{}/disruption-reviews", base))
                .json(&serde_json::json!({
                    "plan_id": plan_id,
                    "source_kind": source,
                    "source_id": source_id,
                    "impacted_scenario_ids": [scn_id]
                }))
                .send()
                .await
                .unwrap()
                .json::<serde_json::Value>()
                .await
                .unwrap()
        }
    };
    let a = mk("scenario", "SCN-a").await;
    let _b = mk("requirement", "REQ-b").await;
    let a_id = a["id"].as_str().unwrap().to_string();

    cli.post(format!("{}/disruption-reviews/{}/resolve", base, a_id))
        .json(&serde_json::json!({"resolution": "keep"}))
        .send()
        .await
        .unwrap();

    let pending: serde_json::Value = cli
        .get(format!(
            "{}/plans/{}/disruption-reviews?status=pending",
            base, plan_id
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let pending_arr = pending["reviews"].as_array().unwrap();
    assert_eq!(pending_arr.len(), 1);
    assert_eq!(pending_arr[0]["source_kind"], "requirement");

    let all: serde_json::Value = cli
        .get(format!("{}/plans/{}/disruption-reviews", base, plan_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(all["reviews"].as_array().unwrap().len(), 2);
}
