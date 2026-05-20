//! `/decisions` router. Append-only ADR log (D12). Body is never edited — a
//! follow-up decision uses `supersedes_id` to chain a replacement.

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use sdi_core::decision::{Decision, DecisionStatus};
use sdi_core::error::DomainError;
use sdi_core::ids::{now, Id, IdKind};
use sdi_db::repo::decision as repo;
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/decisions", post(create).get(list))
        .route("/decisions/:id", get(get_one))
        .route("/decisions/:id/status", post(set_status))
}

#[derive(Debug, Deserialize)]
struct CreateDecisionBody {
    plan_id: String,
    short_code: String,
    title: String,
    body: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    supersedes_id: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    Json(b): Json<CreateDecisionBody>,
) -> ApiResult<Json<Value>> {
    let status = match b.status.as_deref() {
        Some(s) => DecisionStatus::from_str(s)?,
        None => DecisionStatus::Accepted,
    };
    let decision = Decision {
        id: Id::new(IdKind::Decision),
        plan_id: Id::from(b.plan_id),
        short_code: b.short_code,
        title: b.title,
        body: b.body,
        status,
        supersedes_id: b.supersedes_id.map(Id::from),
        created_at: now(),
    };
    let conn = state.conn()?;
    repo::insert(&conn, &decision)?;
    let fresh = repo::get(&conn, &decision.id)?;
    state.publish(EventEnvelope {
        kind: "decision.created".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct ListDecisionQuery {
    plan_id: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListDecisionQuery>,
) -> ApiResult<Json<Value>> {
    let plan_id = q
        .plan_id
        .ok_or_else(|| DomainError::Validation("plan_id query parameter required".into()))?;
    let conn = state.conn()?;
    let rows = repo::list_by_plan(&conn, &Id::from(plan_id))?;
    Ok(Json(json!({ "decisions": rows })))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    Ok(Json(json!(repo::get(&conn, &Id::from(id))?)))
}

#[derive(Debug, Deserialize)]
struct SetStatusBody {
    status: String,
}

async fn set_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<SetStatusBody>,
) -> ApiResult<Json<Value>> {
    let status = DecisionStatus::from_str(&b.status)?;
    let conn = state.conn()?;
    let did = Id::from(id);
    repo::set_status(&conn, &did, status)?;
    let fresh = repo::get(&conn, &did)?;
    state.publish(EventEnvelope {
        kind: "decision.status-changed".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}
