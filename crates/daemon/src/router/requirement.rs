//! `/requirements` router. Body updates are SNAPSHOT-ONLY (D12) — they replace
//! `body` in place; history lives in the Decision append-only log.

use crate::router::provenance;
use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use sdi_core::error::DomainError;
use sdi_core::ids::{now, Id, IdKind};
use sdi_core::requirement::{validate_snapshot_body, Requirement};
use sdi_db::repo::requirement as repo;
use serde::Deserialize;
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/requirements", post(create).get(list))
        .route(
            "/requirements/:id",
            get(get_one).put(update).delete(delete_one),
        )
}

#[derive(Debug, Deserialize)]
struct CreateReqBody {
    plan_id: String,
    short_code: String,
    title: String,
    body: String,
    #[serde(default = "default_source")]
    source: String,
}

fn default_source() -> String {
    "snapshot".into()
}

async fn create(
    State(state): State<AppState>,
    Json(b): Json<CreateReqBody>,
) -> ApiResult<Json<Value>> {
    validate_snapshot_body(&b.body)?;
    let conn = state.conn()?;
    let plan_id = Id::from(b.plan_id);
    // D23 — solo authoring resolves the plan's `direct` sentinel.
    let produced_via_pattern_id = Some(provenance::ensure_direct_pattern(&conn, &plan_id)?);
    let req = Requirement {
        id: Id::new(IdKind::Requirement),
        plan_id,
        short_code: b.short_code,
        title: b.title,
        body: b.body,
        source: b.source,
        produced_via_pattern_id,
        created_at: now(),
        updated_at: now(),
    };
    repo::insert(&conn, &req)?;
    let fresh = repo::get(&conn, &req.id)?;
    state.publish(EventEnvelope {
        kind: "requirement.created".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct ListReqQuery {
    plan_id: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListReqQuery>,
) -> ApiResult<Json<Value>> {
    let plan_id = q
        .plan_id
        .ok_or_else(|| DomainError::Validation("plan_id query parameter required".into()))?;
    let conn = state.conn()?;
    let rows = repo::list_by_plan(&conn, &Id::from(plan_id))?;
    Ok(Json(json!({ "requirements": rows })))
}

async fn get_one(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    Ok(Json(json!(repo::get(&conn, &Id::from(id))?)))
}

#[derive(Debug, Deserialize)]
struct UpdateReqBody {
    title: String,
    body: String,
    #[serde(default = "default_source")]
    source: String,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<UpdateReqBody>,
) -> ApiResult<Json<Value>> {
    validate_snapshot_body(&b.body)?;
    let conn = state.conn()?;
    let rid = Id::from(id);
    repo::update_body(&conn, &rid, &b.title, &b.body, &b.source)?;
    let fresh = repo::get(&conn, &rid)?;
    state.publish(EventEnvelope {
        kind: "requirement.updated".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

async fn delete_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rid = Id::from(id);
    repo::delete(&conn, &rid)?;
    state.publish(EventEnvelope {
        kind: "requirement.deleted".into(),
        entity_id: Some(rid.to_string()),
        payload: json!({ "id": rid.to_string() }),
    });
    Ok(Json(json!({ "deleted": true, "id": rid.to_string() })))
}
