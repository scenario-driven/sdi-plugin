//! End-to-end-ish HTTP test: spins the daemon's router with an in-memory
//! SQLite, drives `/projects`, `/plans`, `/requirements` over a real TCP
//! listener with `reqwest`, and asserts both the happy path and the D8 approve
//! gate (≥1 confirmed scenario required).
//!
//! Keeps in-process so the test does not depend on a built `sdid` binary or
//! XDG paths — instead it constructs `AppState` directly.

use sdi_daemon::AppState;
use sdi_db::Paths;
use std::sync::Arc;
use std::time::Duration;

async fn spawn_server() -> (String, tokio::task::JoinHandle<()>) {
    let tmp = std::env::temp_dir().join(format!(
        "sdid-it-{}-{}",
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

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

#[tokio::test]
async fn health_endpoint_responds() {
    let (base, _h) = spawn_server().await;
    let r = client()
        .get(format!("{}/health", base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["service"], "sdid");
}

#[tokio::test]
async fn project_crud_happy_path() {
    let (base, _h) = spawn_server().await;
    let c = client();
    let suffix = ulid::Ulid::new().to_string();
    let key = format!("SDI{}", &suffix[..4]);
    let slug = format!("sdi-{}", suffix[..6].to_lowercase());

    // create
    let r = c
        .post(format!("{}/projects", base))
        .json(&serde_json::json!({
            "key": key,
            "name": "SDI Test",
            "slug": slug,
            "cwds": ["/tmp/sdi-test-1"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let created: serde_json::Value = r.json().await.unwrap();
    let pid = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["key"], key);

    // get
    let r = c
        .get(format!("{}/projects/{}", base, pid))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // by-cwd
    let r = c
        .get(format!("{}/projects/by-cwd", base))
        .query(&[("cwd", "/tmp/sdi-test-1")])
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["project"]["id"], pid);

    // attach cwd
    let r = c
        .post(format!("{}/projects/{}/cwds", base, pid))
        .json(&serde_json::json!({"cwd": "/tmp/sdi-test-2"}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = r.json().await.unwrap();
    let cwds = body["cwds"].as_array().unwrap();
    assert_eq!(cwds.len(), 2);

    // duplicate key conflicts -> 409
    let r = c
        .post(format!("{}/projects", base))
        .json(&serde_json::json!({
            "key": key,
            "name": "duplicate",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "CONFLICT");
}

#[tokio::test]
async fn plan_approve_requires_scenarios_d8() {
    let (base, _h) = spawn_server().await;
    let c = client();

    let suffix = ulid::Ulid::new().to_string();
    let key = format!("DG{}", &suffix[..4]);
    let slug = format!("dg-{}", suffix[..6].to_lowercase());

    // create project
    let r = c
        .post(format!("{}/projects", base))
        .json(&serde_json::json!({"key": key, "name": "Gate", "slug": slug}))
        .send()
        .await
        .unwrap();
    let project: serde_json::Value = r.json().await.unwrap();
    let project_id = project["id"].as_str().unwrap().to_string();

    // create plan (draft)
    let plan_code = format!("DG-{}", &suffix[..6]);
    let r = c
        .post(format!("{}/plans", base))
        .json(&serde_json::json!({
            "project_id": project_id,
            "short_code": plan_code,
            "title": "v0.1",
            "body": "body"
        }))
        .send()
        .await
        .unwrap();
    let plan: serde_json::Value = r.json().await.unwrap();
    let plan_id = plan["id"].as_str().unwrap().to_string();
    assert_eq!(plan["status"], "draft");

    // approve without scenarios -> 400 SCENARIOS_REQUIRED (D8)
    let r = c
        .post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "SCENARIOS_REQUIRED");
}

#[tokio::test]
async fn requirement_snapshot_overwrite() {
    let (base, _h) = spawn_server().await;
    let c = client();

    let suffix = ulid::Ulid::new().to_string();
    let key = format!("RQ{}", &suffix[..4]);
    let slug = format!("rq-{}", suffix[..6].to_lowercase());

    let project: serde_json::Value = c
        .post(format!("{}/projects", base))
        .json(&serde_json::json!({"key": key, "name": "Req", "slug": slug}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let project_id = project["id"].as_str().unwrap().to_string();

    let plan_code = format!("RQ-{}", &suffix[..6]);
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

    let req_code = format!("REQ-{}", &suffix[..6]);
    let req: serde_json::Value = c
        .post(format!("{}/requirements", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": req_code,
            "title": "Login",
            "body": "v1"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let req_id = req["id"].as_str().unwrap().to_string();
    assert_eq!(req["body"], "v1");
    assert_eq!(req["source"], "snapshot");

    // snapshot rewrite — should overwrite, not append history
    let r = c
        .put(format!("{}/requirements/{}", base, req_id))
        .json(&serde_json::json!({
            "title": "Login",
            "body": "v2 — replaced",
            "source": "decision-rewrite"
        }))
        .send()
        .await
        .unwrap();
    let updated: serde_json::Value = r.json().await.unwrap();
    assert_eq!(updated["body"], "v2 — replaced");
    assert_eq!(updated["source"], "decision-rewrite");
}

#[tokio::test]
async fn requirement_rejects_history_traces() {
    let (base, _h) = spawn_server().await;
    let c = client();

    let suffix = ulid::Ulid::new().to_string();
    let key = format!("RS{}", &suffix[..4]);
    let slug = format!("rs-{}", suffix[..6].to_lowercase());

    let project: serde_json::Value = c
        .post(format!("{}/projects", base))
        .json(&serde_json::json!({"key": key, "name": "Snap", "slug": slug}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let project_id = project["id"].as_str().unwrap().to_string();

    let plan_code = format!("RS-{}", &suffix[..6]);
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

    // create — strikethrough history trace must be rejected
    let strike_code = format!("REQ-STK-{}", &suffix[..4]);
    let r = c
        .post(format!("{}/requirements", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": strike_code,
            "title": "T",
            "body": "The body shall be ~~mutable~~ snapshot-only."
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "SNAPSHOT_VIOLATION");

    // create the snapshot body
    let req_code = format!("REQ-OK-{}", &suffix[..4]);
    let created: serde_json::Value = c
        .post(format!("{}/requirements", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": req_code,
            "title": "T",
            "body": "Snapshot. Endpoint exposes /scenarios."
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let req_id = created["id"].as_str().unwrap().to_string();

    // update with version marker history trace -> rejected
    let r = c
        .put(format!("{}/requirements/{}", base, req_id))
        .json(&serde_json::json!({
            "title": "T",
            "body": "v1: cli only. v2: cli + daemon.",
            "source": "snapshot"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], "SNAPSHOT_VIOLATION");

    // update with Korean history phrase -> rejected
    let r = c
        .put(format!("{}/requirements/{}", base, req_id))
        .json(&serde_json::json!({
            "title": "T",
            "body": "이전에는 CLI 만 있었지만 이제는 daemon 도 포함한다.",
            "source": "snapshot"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);

    // clean rewrite -> accepted
    let r = c
        .put(format!("{}/requirements/{}", base, req_id))
        .json(&serde_json::json!({
            "title": "T",
            "body": "Snapshot v3. New spec.",
            "source": "decision-rewrite"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
}
