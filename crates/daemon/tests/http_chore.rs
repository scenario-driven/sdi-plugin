//! HTTP integration: the chore maintenance lane (#18).
//!
//! Covers:
//! - POST /projects/:id/chores with NO active plan/round → creates an
//!   in_progress kind='chore' task under the per-project CHORE container.
//! - GET /projects/:id/chores exposes the in-flight chore.
//! - POST /chores/:id/done with a note flips it to done.
//! - The CHORE container does NOT register as the project's active plan
//!   (GET /projects/:id/plans/active stays empty), so D8's single-active-plan
//!   invariant is untouched and a real plan can still go active alongside it.

use sdi_daemon::AppState;
use sdi_db::Paths;
use std::sync::Arc;
use std::time::Duration;

async fn spawn_server() -> (String, tokio::task::JoinHandle<()>) {
    let tmp = std::env::temp_dir().join(format!(
        "sdid-chore-{}-{}",
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

async fn mk_project(base: &str) -> String {
    let cli = c();
    let suffix = ulid::Ulid::new().to_string();
    let project: serde_json::Value = cli
        .post(format!("{}/projects", base))
        .json(&serde_json::json!({
            "key": format!("C{}", &suffix[..3]),
            "name": "Chore",
            "slug": format!("c-{}", &suffix[..6].to_lowercase()),
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    project["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn chore_lifecycle_without_active_plan() {
    let (base, handle) = spawn_server().await;
    let cli = c();
    let project_id = mk_project(&base).await;

    // Sanity: no active plan to begin with.
    let active: serde_json::Value = cli
        .get(format!("{}/projects/{}/plans/active", base, project_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        active["plan"].is_null(),
        "expected no active plan initially"
    );

    // Create a chore with no scenario/round. It comes back already in_progress.
    let chore: serde_json::Value = cli
        .post(format!("{}/projects/{}/chores", base, project_id))
        .json(&serde_json::json!({ "description": "tidy imports in foo.rs" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let chore_id = chore["id"].as_str().unwrap().to_string();
    assert_eq!(chore["kind"].as_str(), Some("chore"));
    assert_eq!(chore["status"].as_str(), Some("in_progress"));
    assert!(chore["short_code"].as_str().unwrap().starts_with("CHORE-"));

    // It surfaces in the in-flight chore list.
    let listed: serde_json::Value = cli
        .get(format!("{}/projects/{}/chores", base, project_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let tasks = listed["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["id"].as_str(), Some(chore_id.as_str()));

    // The CHORE container must NOT register as the project's active plan.
    let active2: serde_json::Value = cli
        .get(format!("{}/projects/{}/plans/active", base, project_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        active2["plan"].is_null(),
        "CHORE container must not count as the active plan, got {active2:?}"
    );

    // Complete it with a free-text note (no scenario evidence).
    let done: serde_json::Value = cli
        .post(format!("{}/chores/{}/done", base, chore_id))
        .json(&serde_json::json!({ "note": "removed 3 unused imports" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(done["status"].as_str(), Some("done"));
    assert_eq!(
        done["evidence"]["summary"].as_str(),
        Some("removed 3 unused imports")
    );

    // After completion the in-flight list is empty again.
    let listed2: serde_json::Value = cli
        .get(format!("{}/projects/{}/chores", base, project_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed2["tasks"].as_array().unwrap().len(), 0);

    handle.abort();
}

#[tokio::test]
async fn chore_container_is_idempotent_and_coexists_with_a_real_active_plan() {
    let (base, handle) = spawn_server().await;
    let cli = c();
    let project_id = mk_project(&base).await;

    // Two chores reuse one CHORE container (same plan_id + round_id).
    let mk_chore = |desc: &'static str| {
        let cli = cli.clone();
        let base = base.clone();
        let project_id = project_id.clone();
        async move {
            cli.post(format!("{}/projects/{}/chores", base, project_id))
                .json(&serde_json::json!({ "description": desc }))
                .send()
                .await
                .unwrap()
                .json::<serde_json::Value>()
                .await
                .unwrap()
        }
    };
    let a = mk_chore("a").await;
    let b = mk_chore("b").await;
    assert_eq!(a["plan_id"], b["plan_id"], "chores share one CHORE plan");
    assert_eq!(a["round_id"], b["round_id"], "chores share one CHORE round");

    // A real plan can still be created and approved → active, alongside the
    // permanently-active CHORE container (the partial unique index excludes it).
    let plan: serde_json::Value = cli
        .post(format!("{}/plans", base))
        .json(&serde_json::json!({
            "project_id": project_id,
            "short_code": "P-1",
            "title": "real work",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let plan_id = plan["id"].as_str().unwrap().to_string();

    // Give it a confirmed scenario so the D8 approve gate passes.
    let scn: serde_json::Value = cli
        .post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": "SC-1",
            "given": "g",
            "when": "w",
            "then": "t",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let scn_id = scn["id"].as_str().unwrap().to_string();
    cli.post(format!("{}/scenarios/{}/confirm", base, scn_id))
        .send()
        .await
        .unwrap();

    let approved = cli
        .post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();
    assert!(
        approved.status().is_success(),
        "real plan must approve even though the CHORE container is active: {}",
        approved.text().await.unwrap()
    );

    // The real plan is now THE active plan (not the CHORE container).
    let active: serde_json::Value = cli
        .get(format!("{}/projects/{}/plans/active", base, project_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(active["plan"]["id"].as_str(), Some(plan_id.as_str()));

    handle.abort();
}
