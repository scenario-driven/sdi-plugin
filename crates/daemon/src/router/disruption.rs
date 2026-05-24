//! `/disruption-reviews` router (PRD §6 #4). The LLM records detected impacts
//! here; round activation reads from this queue to enforce the needs-review
//! gate. Resolution is the human's explicit acknowledgement that the impacted
//! scenarios have been triaged.

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use sdi_core::disruption::{
    DisruptionResolution, DisruptionReview, DisruptionReviewStatus, DisruptionSource,
};
use sdi_core::ids::{now, Id, IdKind};
use sdi_db::repo::disruption as repo;
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/disruption-reviews", post(create))
        .route("/disruption-reviews/:id", get(get_one))
        .route("/disruption-reviews/:id/resolve", post(resolve))
        .route("/plans/:plan_id/disruption-reviews", get(list_for_plan))
}

#[derive(Debug, Deserialize)]
struct CreateBody {
    plan_id: String,
    source_kind: String,
    source_id: String,
    impacted_scenario_ids: Vec<String>,
    #[serde(default)]
    note: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    Json(b): Json<CreateBody>,
) -> ApiResult<Json<Value>> {
    let source_kind = DisruptionSource::from_str(&b.source_kind)?;
    let review = DisruptionReview {
        id: Id::new(IdKind::DisruptionReview),
        plan_id: Id::from(b.plan_id),
        source_kind,
        source_id: Id::from(b.source_id),
        impacted_scenario_ids: b.impacted_scenario_ids.into_iter().map(Id::from).collect(),
        status: DisruptionReviewStatus::Pending,
        resolution: None,
        note: b.note,
        created_at: now(),
        resolved_at: None,
    };
    let conn = state.conn()?;
    repo::insert(&conn, &review)?;
    let fresh = repo::get(&conn, &review.id)?;
    state.publish(EventEnvelope {
        kind: "disruption.created".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

async fn get_one(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    Ok(Json(json!(repo::get(&conn, &Id::from(id))?)))
}

#[derive(Debug, Deserialize)]
struct ResolveBody {
    #[serde(default)]
    resolution: Option<String>,
    #[serde(default)]
    note: Option<String>,
}

async fn resolve(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<ResolveBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rid = Id::from(id);
    let resolution = match b.resolution.as_deref() {
        Some(r) => Some(DisruptionResolution::from_str(r)?),
        None => None,
    };
    repo::resolve(&conn, &rid, resolution, b.note.as_deref())?;
    let fresh = repo::get(&conn, &rid)?;
    state.publish(EventEnvelope {
        kind: "disruption.resolved".into(),
        entity_id: Some(rid.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    status: Option<String>,
}

async fn list_for_plan(
    State(state): State<AppState>,
    Path(plan_id): Path<String>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let status = match q.status.as_deref() {
        Some(s) => Some(DisruptionReviewStatus::from_str(s)?),
        None => None,
    };
    let conn = state.conn()?;
    let rows = repo::list_by_plan(&conn, &Id::from(plan_id), status)?;
    Ok(Json(json!({ "reviews": rows })))
}
