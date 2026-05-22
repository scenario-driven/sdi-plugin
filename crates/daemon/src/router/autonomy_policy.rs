//! `/autonomy_policies` router. D14 entity; D17 forced-L4 invariants are
//! enforced inside the repo's `upsert`. Emits `autonomy.changed` on every
//! mutation and `circuit_breaker.triggered` for the D18 panic switch.

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
    mode: String,
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
        mode,
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
