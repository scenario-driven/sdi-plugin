//! Axum router assembly. Each entity router lives in its own module so that
//! adding/removing endpoints stays diff-localized.

use crate::events::sse_handler;
use crate::state::AppState;
use axum::{routing::get, Json, Router};
use serde_json::json;
use std::path::PathBuf;
use tower_http::services::{ServeDir, ServeFile};

pub mod project;
pub mod plan;
pub mod requirement;
pub mod scenario;
pub mod round;
pub mod task;
pub mod knowledge;
pub mod decision;
pub mod disruption;
pub mod collab;
pub mod run;
pub mod aggregate;

pub fn build(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/events", get(sse_handler))
        .merge(project::router())
        .merge(plan::router())
        .merge(requirement::router())
        .merge(scenario::router())
        .merge(round::router())
        .merge(task::router())
        .merge(knowledge::router())
        .merge(decision::router())
        .merge(disruption::router())
        .merge(collab::router())
        .merge(run::router())
        .merge(aggregate::router())
        .with_state(state);

    match locate_web_bundle() {
        Some(bundle) => {
            let index = bundle.join("index.html");
            let serve = ServeDir::new(&bundle).fallback(ServeFile::new(&index));
            api.fallback_service(serve)
        }
        None => api,
    }
}

/// Resolve the static SPA bundle. The daemon serves it as a fallback so the
/// dashboard is reachable on the same port as the HTTP API.
///
/// Lookup order:
/// 1. `SDI_WEB_DIST` env var (CI / dev pinning).
/// 2. `<plugin_root>/web/dist` derived from the daemon binary location
///    (release layout where `sdid` lives under `<plugin_root>/daemon/bin/`).
///
/// Returns `None` if neither resolves to an existing directory; callers then
/// skip static serving entirely and only expose the JSON API.
fn locate_web_bundle() -> Option<PathBuf> {
    if let Ok(env_dir) = std::env::var("SDI_WEB_DIST") {
        let p = PathBuf::from(env_dir);
        if p.is_dir() {
            return Some(p);
        }
    }
    let exe = std::env::current_exe().ok()?;
    // <plugin_root>/daemon/bin/sdid  ->  <plugin_root>/web/dist
    let candidate = exe
        .parent()? // bin
        .parent()? // daemon
        .parent()? // plugin_root
        .join("web/dist");
    if candidate.is_dir() {
        Some(candidate)
    } else {
        None
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "service": "sdid",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
