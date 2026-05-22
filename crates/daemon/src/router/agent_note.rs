//! `/agent_notes` router. M1 blackboard append-only journal, M2 hand-off
//! receipts, retire-as-non-destructive-delete. Emits `agent_note.created`
//! on append.

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use sdi_core::agent_note::{AgentNote, AgentNoteKind, AgentNoteScope};
use sdi_core::error::DomainError;
use sdi_core::ids::{now, Id, IdKind};
use sdi_db::repo::agent_note as repo;
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/agent_notes", post(create).get(list))
        .route("/agent_notes/handoffs", get(list_handoffs))
        .route("/agent_notes/:id/ack", post(ack))
        .route("/agent_notes/:id/retire", post(retire))
}

#[derive(Debug, Deserialize)]
struct CreateBody {
    project_id: String,
    #[serde(default)]
    plan_id: Option<String>,
    #[serde(default)]
    round_id: Option<String>,
    #[serde(default)]
    scenario_id: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    scope_kind: String,
    kind: String,
    from_agent: String,
    #[serde(default)]
    to_agent: Option<String>,
    body: String,
    #[serde(default = "default_payload")]
    payload: serde_json::Value,
}

fn default_payload() -> serde_json::Value {
    serde_json::json!({})
}

async fn create(
    State(state): State<AppState>,
    Json(b): Json<CreateBody>,
) -> ApiResult<Json<Value>> {
    let scope_kind = AgentNoteScope::from_str(&b.scope_kind)?;
    let kind = AgentNoteKind::from_str(&b.kind)?;
    let note = AgentNote {
        id: Id::new(IdKind::AgentNote),
        project_id: Id::from(b.project_id),
        plan_id: b.plan_id.map(Id::from),
        round_id: b.round_id.map(Id::from),
        scenario_id: b.scenario_id.map(Id::from),
        task_id: b.task_id.map(Id::from),
        scope_kind,
        kind,
        from_agent: b.from_agent,
        to_agent: b.to_agent,
        body: b.body,
        payload: b.payload,
        receipt_acknowledged_at: None,
        retired_at: None,
        retired_reason: None,
        created_at: now(),
    };
    let conn = state.conn()?;
    repo::insert(&conn, &note)?;
    let fresh = repo::get(&conn, &note.id)?;
    state.publish(EventEnvelope {
        kind: "agent_note.created".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    scope_kind: String,
    anchor_id: String,
    #[serde(default)]
    kind: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let scope = AgentNoteScope::from_str(&q.scope_kind)?;
    let kind = q.kind.as_deref().map(AgentNoteKind::from_str).transpose()?;
    let conn = state.conn()?;
    let rows = repo::list_active(&conn, scope, &Id::from(q.anchor_id), kind)?;
    Ok(Json(json!({ "notes": rows })))
}

#[derive(Debug, Deserialize)]
struct HandoffQuery {
    to_agent: String,
}

async fn list_handoffs(
    State(state): State<AppState>,
    Query(q): Query<HandoffQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = repo::list_pending_handoffs(&conn, &q.to_agent)?;
    Ok(Json(json!({ "notes": rows })))
}

async fn ack(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let nid = Id::from(id);
    repo::ack_handoff(&conn, &nid)?;
    let fresh = repo::get(&conn, &nid)?;
    state.publish(EventEnvelope {
        kind: "agent_note.acknowledged".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct RetireBody {
    reason: String,
}

async fn retire(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<RetireBody>,
) -> ApiResult<Json<Value>> {
    if b.reason.trim().is_empty() {
        return Err(DomainError::Validation("retire reason required".into()).into());
    }
    let conn = state.conn()?;
    let nid = Id::from(id);
    repo::retire(&conn, &nid, &b.reason)?;
    let fresh = repo::get(&conn, &nid)?;
    state.publish(EventEnvelope {
        kind: "agent_note.retired".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}
