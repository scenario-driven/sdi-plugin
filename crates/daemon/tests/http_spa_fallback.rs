//! Static SPA fallback contract.
//!
//! When `SDI_WEB_DIST` points at a built bundle, the daemon must serve
//! `index.html` as a fallback for unknown paths (so client-side routes like
//! `/plans/:id` resolve in the browser) and the JSON API must still take
//! precedence on its own routes.

use sdi_daemon::AppState;
use sdi_db::Paths;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn web_bundle_serves_index_html_and_does_not_shadow_api() {
    let tmp = std::env::temp_dir().join(format!(
        "sdid-spa-{}-{}",
        std::process::id(),
        ulid::Ulid::new()
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    let dist = tmp.join("web_dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(
        dist.join("index.html"),
        "<!doctype html><html><body><div id=root></div></body></html>",
    )
    .unwrap();
    std::fs::write(dist.join("favicon.svg"), "<svg/>").unwrap();
    std::env::set_var("SDI_WEB_DIST", &dist);

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
    let _server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    // 1. API still wins on its own routes.
    let health: serde_json::Value = client
        .get(format!("{}/health", base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["ok"], true);

    // 2. Asset hit (file exists on disk under dist/).
    let favicon = client
        .get(format!("{}/favicon.svg", base))
        .send()
        .await
        .unwrap();
    assert_eq!(favicon.status(), 200);
    assert_eq!(favicon.text().await.unwrap(), "<svg/>");

    // 3. SPA fallback — an unknown path returns index.html, not 404.
    let deep = client
        .get(format!("{}/plans/PLAN-abc/scenarios", base))
        .send()
        .await
        .unwrap();
    assert_eq!(deep.status(), 200);
    let body = deep.text().await.unwrap();
    assert!(
        body.contains("<div id=root>"),
        "deep route should fall back to index.html, got body: {body}"
    );

    std::env::remove_var("SDI_WEB_DIST");
}
