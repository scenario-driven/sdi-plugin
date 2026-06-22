//! `/chores` router — the lightweight maintenance lane (#18).
//!
//! A chore is a `kind='chore'` Task that bypasses the scenario/round ceremony:
//! it is created already `in_progress` under a per-project `CHORE` container
//! (`ensure_chore_container`), so a trivial consistency edit satisfies the
//! active-task PreToolUse gate even when no real plan is active. Completion
//! takes a free-text note instead of scenario evidence (#12/#13's scenario
//! mapping is skipped — a chore has no GWT scenario to evidence).

use crate::router::provenance;
use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, State},
    routing::post,
    Json, Router,
};
use sdi_core::ids::{now, Id, IdKind};
use sdi_core::task::{Task, TaskEvidence, TaskStatus};
use sdi_db::repo::task as repo;
use serde::Deserialize;
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/:id/chores", post(create).get(list))
        .route("/chores/:id/done", post(done))
}

#[derive(Debug, Deserialize)]
struct CreateChoreBody {
    description: String,
}

/// `POST /projects/:id/chores` — create a chore already `in_progress`.
async fn create(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(b): Json<CreateChoreBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let project_id = Id::from(project_id);
    let (plan_id, round_id) = provenance::ensure_chore_container(&conn, &project_id)?;
    // Chores are solo-flow by definition; bind the container plan's `direct`
    // sentinel exactly as an unbound task create would.
    let produced_via_pattern_id = Some(provenance::ensure_direct_pattern(&conn, &plan_id)?);
    let short_code = format!("CHORE-{}", short_ulid());
    let task = Task {
        id: Id::new(IdKind::Task),
        round_id,
        plan_id,
        short_code,
        description: b.description,
        // Created already in flight — that is the whole point of the lane: one
        // call satisfies the active-task gate, no separate `start` step.
        status: TaskStatus::InProgress,
        kind: "chore".into(),
        parent_scenario_ids: vec![],
        parent_requirement_ids: vec![],
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

/// `GET /projects/:id/chores` — in-flight chores for the project.
async fn list(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = repo::list_in_flight_chores_for_project(&conn, &Id::from(project_id))?;
    Ok(Json(json!({ "tasks": rows })))
}

#[derive(Debug, Deserialize)]
struct DoneChoreBody {
    #[serde(default)]
    note: Option<String>,
}

/// `POST /chores/:id/done` — complete a chore with a free-text note.
///
/// Unlike `/tasks/:id/complete`, there is no scenario evidence to validate
/// (#12/#13 are scenario-only); the note becomes the evidence `summary`. The
/// summary alone satisfies `TaskEvidence::validate_for_done`, so the done
/// transition gate still holds — a chore cannot complete with no record at all.
async fn done(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<DoneChoreBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let tid = Id::from(id);
    let task = repo::get(&conn, &tid)?;
    let summary = b
        .note
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "chore completed".to_string());
    let evidence = TaskEvidence {
        scenarios: vec![],
        summary: Some(summary),
        extras: Default::default(),
    };
    // Same double-gate as a normal done: domain check first, then the repo
    // re-validates at the SQL boundary. No scenario mapping / round-result
    // mirror — a chore has no parent scenarios.
    task.check_transition(TaskStatus::Done, Some(&evidence))?;
    repo::complete_with_evidence(&conn, &tid, &evidence)?;
    let fresh = repo::get(&conn, &tid)?;
    state.publish(EventEnvelope {
        kind: "task.completed".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

/// Short, readable suffix for a chore's human short_code. Full uniqueness lives
/// in the task id; this is only the label tail. ULIDs are monotonic + 6 base32
/// chars give ~1B values, so `(plan_id, short_code)` collisions are negligible.
fn short_ulid() -> String {
    let ulid = ulid::Ulid::new().to_string();
    ulid[ulid.len() - 6..].to_string()
}
