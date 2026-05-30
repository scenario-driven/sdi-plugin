//! `/tasks` router. Task is D3-runtime artifact: LLM-decomposed work items
//! bound to a round and (optionally) parent scenarios/requirements. The
//! `done` transition is the only one that requires evidence (PRD §6.6) and is
//! double-gated: domain `Task::check_transition` + repo `complete_with_evidence`.

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
use sdi_core::task::{Task, TaskEvidence, TaskStatus};
use sdi_db::repo::round as round_repo;
use sdi_db::repo::task as repo;
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tasks", post(create).get(list))
        .route("/tasks/:id", get(get_one))
        .route("/tasks/:id/status", post(set_status))
        .route("/tasks/:id/complete", post(complete))
        .route("/plans/:plan_id/tasks/in-flight", get(list_in_flight))
        .route("/plans/:plan_id/tasks/backlog", get(list_backlog))
}

#[derive(Debug, Deserialize)]
struct CreateTaskBody {
    round_id: String,
    short_code: String,
    description: String,
    #[serde(default)]
    parent_scenario_ids: Vec<String>,
    #[serde(default)]
    parent_requirement_ids: Vec<String>,
}

async fn create(
    State(state): State<AppState>,
    Json(b): Json<CreateTaskBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let round_id = Id::from(b.round_id);
    // D23 — the task's plan is resolved through its round; a task decomposed
    // outside any pattern resolves that plan's `direct` sentinel.
    let plan_id = round_repo::get(&conn, &round_id)?.plan_id;
    let produced_via_pattern_id = Some(provenance::ensure_direct_pattern(&conn, &plan_id)?);
    let task = Task {
        id: Id::new(IdKind::Task),
        round_id,
        short_code: b.short_code,
        description: b.description,
        status: TaskStatus::Todo,
        parent_scenario_ids: b.parent_scenario_ids.into_iter().map(Id::from).collect(),
        parent_requirement_ids: b.parent_requirement_ids.into_iter().map(Id::from).collect(),
        evidence: None,
        produced_via_pattern_id,
        evidence_at: None,
        created_at: now(),
        updated_at: now(),
    };
    repo::insert(&conn, &task)?;
    let fresh = repo::get(&conn, &task.id)?;
    state.publish(EventEnvelope {
        kind: "task.created".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct ListTaskQuery {
    round_id: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListTaskQuery>,
) -> ApiResult<Json<Value>> {
    let round_id = q
        .round_id
        .ok_or_else(|| DomainError::Validation("round_id query parameter required".into()))?;
    let conn = state.conn()?;
    let rows = repo::list_by_round(&conn, &Id::from(round_id))?;
    Ok(Json(json!({ "tasks": rows })))
}

async fn get_one(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
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
    let target = TaskStatus::from_str(&b.status)?;
    if matches!(target, TaskStatus::Done) {
        return Err(DomainError::EvidenceRequired.into());
    }
    let conn = state.conn()?;
    let tid = Id::from(id);
    let task = repo::get(&conn, &tid)?;
    task.check_transition(target, None)?;
    repo::set_status(&conn, &tid, target)?;
    let fresh = repo::get(&conn, &tid)?;
    state.publish(EventEnvelope {
        kind: "task.updated".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct CompleteTaskBody {
    evidence: TaskEvidence,
}

async fn complete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<CompleteTaskBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let tid = Id::from(id);
    let task = repo::get(&conn, &tid)?;
    task.check_transition(TaskStatus::Done, Some(&b.evidence))?;
    repo::complete_with_evidence(&conn, &tid, &b.evidence)?;
    // D6: mirror per-scenario evidence into round.scenario_results so
    // /rounds/<id>/results reflects the verdict the task carried.
    for se in &b.evidence.scenarios {
        round_repo::upsert_result(
            &conn,
            &task.round_id,
            &se.scenario_id,
            se.result,
            se.note.as_deref(),
            Some(se.evidence_ref.as_str()),
        )?;
        state.publish(EventEnvelope {
            kind: "round.result.updated".into(),
            entity_id: Some(task.round_id.to_string()),
            payload: json!({
                "round_id": task.round_id.to_string(),
                "scenario_id": se.scenario_id.to_string(),
                "result": se.result.to_string(),
                "evidence_ref": se.evidence_ref,
            }),
        });
    }
    let fresh = repo::get(&conn, &tid)?;
    state.publish(EventEnvelope {
        kind: "task.completed".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

async fn list_in_flight(
    State(state): State<AppState>,
    Path(plan_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = repo::list_in_flight_for_plan(&conn, &Id::from(plan_id))?;
    Ok(Json(json!({ "tasks": rows })))
}

async fn list_backlog(
    State(state): State<AppState>,
    Path(plan_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = repo::list_backlog_for_plan(&conn, &Id::from(plan_id))?;
    Ok(Json(json!({ "tasks": rows })))
}
