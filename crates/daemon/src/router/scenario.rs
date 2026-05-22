//! `/scenarios` router. D5 GWT-strict authoring path; FTS5 search; status
//! transition (draft → confirmed) is what unlocks the D8 approve gate.

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use sdi_core::error::DomainError;
use sdi_core::ids::{now, Id, IdKind};
use sdi_core::scenario::{Scenario, ScenarioStatus};
use sdi_db::repo::scenario as repo;
use serde::Deserialize;
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scenarios", post(create).get(list))
        .route("/scenarios/search", get(search))
        .route("/scenarios/:id", get(get_one).put(update))
        .route("/scenarios/:id/confirm", post(confirm))
}

#[derive(Debug, Deserialize)]
struct CreateScenarioBody {
    plan_id: String,
    short_code: String,
    given: String,
    #[serde(rename = "when")]
    when_clause: String,
    #[serde(rename = "then")]
    then_clause: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    origin_round_id: Option<String>,
    #[serde(default)]
    confirmed: bool,
    /// M4 contract — DAG predecessor edges (Scenario short_codes).
    #[serde(default)]
    depends_on: Vec<String>,
    /// M4 contract — producing agent.
    #[serde(default)]
    produced_by: Option<String>,
    /// M4 contract — verifying agent.
    #[serde(default)]
    verified_by: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    Json(b): Json<CreateScenarioBody>,
) -> ApiResult<Json<Value>> {
    let scenario = Scenario {
        id: Id::new(IdKind::Scenario),
        plan_id: Id::from(b.plan_id),
        short_code: b.short_code,
        given: b.given,
        when_clause: b.when_clause,
        then_clause: b.then_clause,
        tags: b.tags,
        origin_round_id: b.origin_round_id.map(Id::from),
        status: if b.confirmed {
            ScenarioStatus::Confirmed
        } else {
            ScenarioStatus::Draft
        },
        depends_on: b.depends_on,
        produced_by: b.produced_by,
        verified_by: b.verified_by,
        created_at: now(),
        updated_at: now(),
    };
    let conn = state.conn()?;
    repo::insert(&conn, &scenario)?;
    let fresh = repo::get(&conn, &scenario.id)?;
    state.publish(EventEnvelope {
        kind: "scenario.created".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct ListScenarioQuery {
    plan_id: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListScenarioQuery>,
) -> ApiResult<Json<Value>> {
    let plan_id = q
        .plan_id
        .ok_or_else(|| DomainError::Validation("plan_id query parameter required".into()))?;
    let conn = state.conn()?;
    let rows = repo::list_by_plan(&conn, &Id::from(plan_id))?;
    Ok(Json(json!({ "scenarios": rows })))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    Ok(Json(json!(repo::get(&conn, &Id::from(id))?)))
}

#[derive(Debug, Deserialize)]
struct UpdateScenarioBody {
    given: String,
    #[serde(rename = "when")]
    when_clause: String,
    #[serde(rename = "then")]
    then_clause: String,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<UpdateScenarioBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let sid = Id::from(id);
    repo::update_gwt(&conn, &sid, &b.given, &b.when_clause, &b.then_clause)?;
    let fresh = repo::get(&conn, &sid)?;
    state.publish(EventEnvelope {
        kind: "scenario.updated".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

async fn confirm(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let sid = Id::from(id);
    repo::set_status(&conn, &sid, ScenarioStatus::Confirmed)?;
    let fresh = repo::get(&conn, &sid)?;
    state.publish(EventEnvelope {
        kind: "scenario.confirmed".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct SearchScenarioQuery {
    plan_id: String,
    q: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    20
}

async fn search(
    State(state): State<AppState>,
    Query(qq): Query<SearchScenarioQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let hits = repo::search(&conn, &Id::from(qq.plan_id), &qq.q, qq.limit)?;
    Ok(Json(json!({ "scenarios": hits })))
}
