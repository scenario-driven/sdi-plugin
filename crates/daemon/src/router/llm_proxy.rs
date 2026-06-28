//! `/v1/agent/stream` proxy → llm-bridge sidecar (PRD-v2 D37).
//!
//! sdid (Rust) cannot host the Node provider stack, so subscription-based LLM
//! traffic is served by a Node `llm-bridge` sidecar (`plugin/llm-bridge/`).
//! This module is the daemon's thin SSE pass-through: the browser SPA posts to
//! sdid (same origin as the dashboard) and sdid streams the request body to the
//! bridge and the bridge's SSE response back to the browser, byte for byte.
//!
//! Why a proxy rather than a browser→bridge direct call:
//!   - same-origin: the SPA already talks to sdid; one origin, no extra CORS.
//!   - the bridge is loopback-bound and tokenless (its only client is sdid);
//!     keeping it un-exposed to the browser preserves that boundary.
//!
//! Route ownership note (axum 0.7 build-time panic on param-name divergence at
//! a shared prefix): `/v1/agent/stream` is a fresh prefix — collab owns
//! `/questions`, plan owns `/plans/:id`, oracle owns `/projects/:project_id/…`,
//! `/decision-questions/…`, `/ssot-nodes/…`, `/user-flows/…`. No overlap.

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};

use crate::state::AppState;

/// Default bridge URL when `SDI_LLM_BRIDGE_URL` is unset. The bridge defaults to
/// port 19501 (see `plugin/llm-bridge/src/index.mjs`).
const DEFAULT_BRIDGE_URL: &str = "http://127.0.0.1:19501";

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/agent/stream", post(proxy_stream))
}

fn bridge_url() -> String {
    std::env::var("SDI_LLM_BRIDGE_URL").unwrap_or_else(|_| DEFAULT_BRIDGE_URL.to_string())
}

/// `POST /v1/agent/stream` — forward the request body to the llm-bridge and
/// stream its SSE response back unbuffered.
///
/// The bridge selects the provider from the request body's `provider` field
/// ("acp" | "sdk"); sdid is provider-agnostic and just relays bytes.
async fn proxy_stream(State(_state): State<AppState>, headers: HeaderMap, body: Body) -> Response {
    let target = format!("{}/v1/agent/stream", bridge_url());

    // Buffer the request body. Agent prompts are small JSON payloads (the
    // upstream bridge caps bodies at 1 MiB), so a full read keeps the proxy
    // simple and avoids a streaming-request type mismatch between axum's Body
    // and reqwest's body. The *response* is what must stream (SSE tokens).
    let bytes = match axum::body::to_bytes(body, 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "LLM_BRIDGE_BAD_REQUEST",
                &format!("failed to read request body: {e}"),
            );
        }
    };

    let client = reqwest::Client::new();
    let mut req = client
        .post(&target)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "text/event-stream")
        .body(bytes.to_vec());

    // Forward an inbound Authorization header if present (future-proofs a
    // bridge that opts into a pairing token); harmless when the bridge is
    // tokenless.
    if let Some(auth) = headers.get(header::AUTHORIZATION) {
        req = req.header(header::AUTHORIZATION, auth);
    }

    let upstream = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            // Connection refused / DNS / timeout → bridge is down. 502 so the
            // SPA can surface "llm-bridge unreachable" distinctly from a 4xx.
            return error_response(
                StatusCode::BAD_GATEWAY,
                "LLM_BRIDGE_UNREACHABLE",
                &format!("llm-bridge at {target} unreachable: {e}"),
            );
        }
    };

    let status = upstream.status();
    let content_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| header::HeaderValue::from_static("text/event-stream"));

    // Stream the upstream body straight through as the axum response body.
    // `bytes_stream` yields `reqwest::Error`; axum's Body wants an error that
    // is `Into<BoxError>`, which reqwest::Error satisfies.
    let stream = upstream.bytes_stream();
    let body = Body::from_stream(stream);

    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        // SSE hygiene: disable proxy buffering and keep the connection from
        // being treated as cacheable.
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());

    // Carry the connection header through so SSE keep-alive survives.
    response.headers_mut().insert(
        header::CONNECTION,
        header::HeaderValue::from_static("keep-alive"),
    );
    response
}

/// Build a JSON error body matching the daemon's stable error contract
/// (`{ "error": { code, message, status } }`) so the SPA parses bridge proxy
/// failures the same way it parses domain errors.
fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    let body = serde_json::json!({
        "error": { "code": code, "message": message, "status": status.as_u16() }
    });
    (status, axum::Json(body)).into_response()
}
