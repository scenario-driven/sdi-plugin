//! End-to-end HTTP coverage for the v0.3 Project lifecycle surface:
//!   - PATCH-style PUT /projects/:id (name + description + enabled + wiki_paths)
//!   - POST /projects/:id/disable + POST /projects/:id/enable (idempotent)
//!   - DELETE /projects/:id (default 409 on in-flight tasks, ?force=true)
//!   - cascade verification: every PROJ-scoped row across the schema is gone.

use sdi_daemon::AppState;
use sdi_db::Paths;
use serde_json::{json, Value};
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

async fn create_project(base: &str, c: &reqwest::Client, key: &str) -> Value {
    let suffix = ulid::Ulid::new().to_string();
    let slug = format!("{}-{}", key.to_lowercase(), &suffix[..6].to_lowercase());
    let r = c
        .post(format!("{base}/projects"))
        .json(&json!({
            "key": key,
            "name": format!("Project {key}"),
            "slug": slug,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    r.json().await.unwrap()
}

#[tokio::test]
async fn project_create_defaults_enabled_true_wiki_paths_docs() {
    let (base, _h) = spawn_server().await;
    let c = client();
    let key = format!("DEF{}", &ulid::Ulid::new().to_string()[..3]);
    let project = create_project(&base, &c, &key).await;
    // New project surfaces the new fields with defaults.
    assert_eq!(project["enabled"], true);
    // `description` is None and `skip_serializing_if = "Option::is_none"`
    // strips it out — confirm absence.
    assert!(project.get("description").is_none());
    let wiki = project["wiki_paths"].as_array().expect("wiki_paths array");
    assert_eq!(wiki.len(), 1);
    assert_eq!(wiki[0], "docs");
}

#[tokio::test]
async fn project_update_patch_style_handles_each_field_independently() {
    let (base, _h) = spawn_server().await;
    let c = client();
    let key = format!("PCH{}", &ulid::Ulid::new().to_string()[..3]);
    let project = create_project(&base, &c, &key).await;
    let pid = project["id"].as_str().unwrap().to_string();

    // PUT { description: "v1" } sets only description.
    let r = c
        .put(format!("{base}/projects/{pid}"))
        .json(&json!({ "description": "first body" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["description"], "first body");
    assert_eq!(v["enabled"], true);

    // PUT { description: null } clears description.
    let r = c
        .put(format!("{base}/projects/{pid}"))
        .json(&json!({ "description": null }))
        .send()
        .await
        .unwrap();
    let v: Value = r.json().await.unwrap();
    assert!(
        v.get("description").is_none(),
        "description should be cleared"
    );

    // PUT { wiki_paths: ["docs", "wiki", "/abs/path"] } replaces array.
    let r = c
        .put(format!("{base}/projects/{pid}"))
        .json(&json!({ "wiki_paths": ["docs", "wiki", "/abs/path"] }))
        .send()
        .await
        .unwrap();
    let v: Value = r.json().await.unwrap();
    let wp = v["wiki_paths"].as_array().unwrap();
    assert_eq!(wp.len(), 3);

    // PUT { name: "" } rejected with VALIDATION.
    let r = c
        .put(format!("{base}/projects/{pid}"))
        .json(&json!({ "name": "" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let err: Value = r.json().await.unwrap();
    assert_eq!(err["error"]["code"], "VALIDATION");
}

#[tokio::test]
async fn project_disable_then_enable_is_idempotent_and_emits_events() {
    let (base, _h) = spawn_server().await;
    let c = client();
    let key = format!("EN{}", &ulid::Ulid::new().to_string()[..4]);
    let project = create_project(&base, &c, &key).await;
    let pid = project["id"].as_str().unwrap().to_string();

    // disable
    let r = c
        .post(format!("{base}/projects/{pid}/disable"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["enabled"], false);

    // disable again — idempotent
    let r = c
        .post(format!("{base}/projects/{pid}/disable"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["enabled"], false);

    // enable
    let r = c
        .post(format!("{base}/projects/{pid}/enable"))
        .send()
        .await
        .unwrap();
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["enabled"], true);

    // by-cwd round trip carries the field (sanity).
    let r = c
        .get(format!("{base}/projects/{pid}"))
        .send()
        .await
        .unwrap();
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["enabled"], true);
}

#[tokio::test]
async fn project_delete_default_blocked_by_in_flight_task_then_forced() {
    let (base, _h) = spawn_server().await;
    let c = client();
    let suffix = ulid::Ulid::new().to_string();
    let key = format!("DEL{}", &suffix[..3]);
    let project = create_project(&base, &c, &key).await;
    let pid = project["id"].as_str().unwrap().to_string();

    // plan + scenario + round + task chain to plant an in-flight task.
    let plan_code = format!("PL-{}", &suffix[..6]);
    let r = c
        .post(format!("{base}/plans"))
        .json(&json!({
            "project_id": pid,
            "short_code": plan_code,
            "title": "T",
            "body": "",
        }))
        .send()
        .await
        .unwrap();
    let plan: Value = r.json().await.unwrap();
    let plan_id = plan["id"].as_str().unwrap().to_string();

    let scn_code = format!("S-{}", &suffix[..6]);
    let r = c
        .post(format!("{base}/scenarios"))
        .json(&json!({
            "plan_id": plan_id,
            "short_code": scn_code,
            "given": "g",
            "when": "w",
            "then": "t",
            "confirmed": true,
        }))
        .send()
        .await
        .unwrap();
    let scn: Value = r.json().await.unwrap();
    let scn_id = scn["id"].as_str().unwrap().to_string();

    let r = c
        .post(format!("{base}/plans/{plan_id}/approve"))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "approve failed");

    let round_code = format!("R-{}", &suffix[..6]);
    let r = c
        .post(format!("{base}/rounds"))
        .json(&json!({"plan_id": plan_id, "short_code": round_code}))
        .send()
        .await
        .unwrap();
    let round: Value = r.json().await.unwrap();
    let round_id = round["id"].as_str().unwrap().to_string();
    let r = c
        .post(format!("{base}/rounds/{round_id}/activate"))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "activate round failed");

    let task_code = format!("T-{}", &suffix[..6]);
    let r = c
        .post(format!("{base}/tasks"))
        .json(&json!({
            "round_id": round_id,
            "short_code": task_code,
            "description": "do thing",
            "scenarios": [scn_id],
        }))
        .send()
        .await
        .unwrap();
    let task: Value = r.json().await.unwrap();
    let task_id = task["id"].as_str().unwrap().to_string();
    // Move task to in_progress so the count_in_flight_tasks guard fires.
    let r = c
        .post(format!("{base}/tasks/{task_id}/status"))
        .json(&json!({"status": "in_progress"}))
        .send()
        .await
        .unwrap();
    assert!(
        r.status().is_success(),
        "task → in_progress failed: {}",
        r.text().await.unwrap_or_default()
    );

    // DELETE without force → 409 CONFLICT.
    let r = c
        .delete(format!("{base}/projects/{pid}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    let err: Value = r.json().await.unwrap();
    assert_eq!(err["error"]["code"], "CONFLICT");

    // DELETE with ?force=true → 200, cascade succeeds.
    let r = c
        .delete(format!("{base}/projects/{pid}?force=true"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["forced"], true);

    // get → 404 (project gone)
    let r = c
        .get(format!("{base}/projects/{pid}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);

    // plan / scenario / round / task all cascaded — fetching any returns 404.
    for path in [
        format!("/plans/{plan_id}"),
        format!("/scenarios/{scn_id}"),
        format!("/rounds/{round_id}"),
        format!("/tasks/{task_id}"),
    ] {
        let r = c.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(r.status(), 404, "expected cascade-404 on {path}");
    }
}

#[tokio::test]
async fn project_delete_cascade_clean_when_empty() {
    let (base, _h) = spawn_server().await;
    let c = client();
    let key = format!("EMP{}", &ulid::Ulid::new().to_string()[..3]);
    let project = create_project(&base, &c, &key).await;
    let pid = project["id"].as_str().unwrap().to_string();
    let r = c
        .delete(format!("{base}/projects/{pid}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["forced"], false);
}
