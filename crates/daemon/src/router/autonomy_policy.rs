//! `/autonomy_policies` router. D14 entity; D17 forced-L4 invariants are
//! enforced inside the repo's `upsert`. v0.5 (D24/D25/D28/D29) added the
//! pattern_kind scope, l5_threshold, pattern_depth_cap,
//! plan_single_session_lock, external_surface, timeout_ms, forced columns.
//! Emits `autonomy.changed` on every mutation and `circuit_breaker.triggered`
//! for the D18 panic switch.

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Query, State},
    routing::{get, post},
    Json, Router,
};
use sdi_core::autonomy_policy::{AutonomyMode, AutonomyPolicy, AutonomyScopeKind};
use sdi_core::error::DomainError;
use sdi_core::ids::{now, Id, IdKind};
use sdi_db::repo::autonomy_policy as repo;
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/autonomy_policies", post(upsert).get(list))
        .route("/autonomy_policies/resolve", get(resolve))
        .route("/autonomy_policies/circuit_breaker", post(circuit_breaker))
}

#[derive(Debug, Deserialize)]
struct UpsertBody {
    project_id: String,
    #[serde(default)]
    plan_id: Option<String>,
    scope_kind: String,
    #[serde(default)]
    decision_kind: Option<String>,
    /// D25 — required when scope_kind = pattern_kind.
    #[serde(default)]
    pattern_kind: Option<String>,
    mode: String,
    /// D28 — L5 unlock threshold (0..=10); defaults to 5.
    #[serde(default)]
    l5_threshold: Option<i32>,
    /// D24 — pattern DAG depth cap (1..=10); defaults to 3.
    #[serde(default)]
    pattern_depth_cap: Option<i32>,
    /// D29 — single-session lock on the plan's active scenarios.
    #[serde(default)]
    plan_single_session_lock: Option<bool>,
    /// D17 — marks plans with external (publish/deploy/API) surface.
    #[serde(default)]
    external_surface: Option<bool>,
    /// L4 timed-gate auto-apply delay in milliseconds.
    #[serde(default)]
    timeout_ms: Option<i64>,
    /// D17 — true when this row was set by a forced-L4 invariant.
    #[serde(default)]
    forced: Option<bool>,
    #[serde(default = "default_actor")]
    set_by: String,
    #[serde(default)]
    reason: Option<String>,
}

fn default_actor() -> String {
    "agent".into()
}

async fn upsert(
    State(state): State<AppState>,
    Json(b): Json<UpsertBody>,
) -> ApiResult<Json<Value>> {
    let scope_kind = AutonomyScopeKind::from_str(&b.scope_kind)?;
    let mode = AutonomyMode::from_str(&b.mode)?;
    let policy = AutonomyPolicy {
        id: Id::new(IdKind::AutonomyPolicy),
        project_id: Id::from(b.project_id),
        plan_id: b.plan_id.map(Id::from),
        scope_kind,
        decision_kind: b.decision_kind,
        pattern_kind: b.pattern_kind.clone(),
        mode,
        l5_threshold: b.l5_threshold.unwrap_or(5),
        pattern_depth_cap: b.pattern_depth_cap.unwrap_or(3),
        plan_single_session_lock: b.plan_single_session_lock.unwrap_or(false),
        external_surface: b.external_surface.unwrap_or(false),
        timeout_ms: b.timeout_ms,
        forced: b.forced.unwrap_or(false),
        set_at: now(),
        set_by: b.set_by,
        reason: b.reason,
        created_at: now(),
        updated_at: now(),
    };
    let conn = state.conn()?;
    repo::upsert(&conn, &policy)?;
    let fresh = repo::resolve(
        &conn,
        &policy.project_id,
        policy.plan_id.as_ref(),
        policy.decision_kind.as_deref(),
        policy.pattern_kind.as_deref(),
    )?;
    state.publish(EventEnvelope {
        kind: "autonomy.changed".into(),
        entity_id: Some(policy.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    project_id: String,
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = repo::list_by_project(&conn, &Id::from(q.project_id))?;
    Ok(Json(json!({ "policies": rows })))
}

#[derive(Debug, Deserialize)]
struct ResolveQuery {
    project_id: String,
    #[serde(default)]
    plan_id: Option<String>,
    #[serde(default)]
    decision_kind: Option<String>,
    /// D25 — let callers resolve a pattern-kind-scoped policy directly.
    #[serde(default)]
    pattern_kind: Option<String>,
}

async fn resolve(
    State(state): State<AppState>,
    Query(q): Query<ResolveQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let plan_id = q.plan_id.map(Id::from);
    let policy = repo::resolve(
        &conn,
        &Id::from(q.project_id),
        plan_id.as_ref(),
        q.decision_kind.as_deref(),
        q.pattern_kind.as_deref(),
    )?;
    Ok(Json(json!({ "policy": policy })))
}

#[derive(Debug, Deserialize)]
struct CircuitBreakerBody {
    project_id: String,
    #[serde(default = "default_actor")]
    actor: String,
    reason: String,
}

async fn circuit_breaker(
    State(state): State<AppState>,
    Json(b): Json<CircuitBreakerBody>,
) -> ApiResult<Json<Value>> {
    if b.reason.trim().is_empty() {
        return Err(DomainError::Validation("circuit_breaker reason required".into()).into());
    }
    let conn = state.conn()?;
    let pid = Id::from(b.project_id);
    let demoted = repo::circuit_breaker(&conn, &pid, &b.actor, &b.reason, now())?;
    state.publish(EventEnvelope {
        kind: "circuit_breaker.triggered".into(),
        entity_id: Some(pid.to_string()),
        payload: json!({
            "project_id": pid.to_string(),
            "actor": b.actor,
            "reason": b.reason,
            "demoted": demoted,
        }),
    });
    Ok(Json(json!({ "demoted": demoted })))
}
