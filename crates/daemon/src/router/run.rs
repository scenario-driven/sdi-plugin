//! `/runs` + task hierarchy + task lease routers.
//!
//! Endpoints:
//!   POST /runs                          start a run
//!   POST /runs/:id/finish               record terminal state
//!   GET  /runs/:id, /runs               read
//!   POST /tasks/:id/decompose           create subtask(s) under a parent
//!   POST /tasks/:id/subtasks            same shape (alias used by Clawket CLI)
//!   GET  /tasks/:id/ancestors|descendants|subtree
//!   POST /tasks/:id/relations           add a non-parent relation
//!   DELETE /relations/:id
//!   POST /tasks/:id/lease  + /lease/heartbeat + /lease/release
//!   GET  /tasks/stats                   status histogram

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post},
    Json, Router,
};
use sdi_core::error::DomainError;
use sdi_core::ids::{now, Id, IdKind};
use sdi_core::run::{RelationKind, Run, RunResult, TaskRelation};
use sdi_core::task::{Task, TaskStatus};
use crate::router::provenance;
use sdi_db::repo::round as round_repo;
use sdi_db::repo::run as repo;
use sdi_db::repo::task as task_repo;
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;

pub fn router() -> Router<AppState> {
    Router::new()
        // runs
        .route("/runs", post(start_run).get(list_runs))
        .route("/runs/:id", get(get_run))
        .route("/runs/:id/finish", post(finish_run))
        // hierarchy
        .route("/tasks/:id/decompose", post(decompose))
        .route("/tasks/:id/subtasks", post(decompose))
        .route("/tasks/:id/ancestors", get(ancestors))
        .route("/tasks/:id/descendants", get(descendants))
        .route("/tasks/:id/subtree", get(subtree))
        .route(
            "/tasks/:id/relations",
            post(create_relation).get(list_relations),
        )
        .route("/relations/:id", delete(delete_relation))
        // lease
        .route("/tasks/:id/lease", post(acquire_lease).get(get_lease))
        .route("/tasks/:id/lease/heartbeat", post(heartbeat_lease))
        .route("/tasks/:id/lease/release", post(release_lease))
        // stats
        .route("/tasks/stats", get(stats))
}

// ---------------------------------------------------------------------------
// runs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct StartRunBody {
    task_id: String,
    #[serde(default = "default_actor")]
    actor: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
}

fn default_actor() -> String {
    "agent".into()
}

async fn start_run(
    State(state): State<AppState>,
    Json(b): Json<StartRunBody>,
) -> ApiResult<Json<Value>> {
    let run = Run {
        id: Id::new(IdKind::Run),
        task_id: Id::from(b.task_id),
        actor: b.actor,
        session_id: b.session_id,
        started_at: now(),
        finished_at: None,
        result: None,
        notes: None,
        payload: b.payload.unwrap_or(Value::Null),
    };
    let conn = state.conn()?;
    repo::insert_run(&conn, &run)?;
    let fresh = repo::get_run(&conn, &run.id)?;
    state.publish(EventEnvelope {
        kind: "run.started".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct FinishRunBody {
    result: String,
    #[serde(default)]
    notes: Option<String>,
}

async fn finish_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<FinishRunBody>,
) -> ApiResult<Json<Value>> {
    let result = RunResult::from_str(&b.result)?;
    let conn = state.conn()?;
    let rid = Id::from(id);
    repo::finish_run(&conn, &rid, result, b.notes.as_deref())?;
    let fresh = repo::get_run(&conn, &rid)?;
    state.publish(EventEnvelope {
        kind: "run.finished".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

async fn get_run(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    Ok(Json(json!(repo::get_run(&conn, &Id::from(id))?)))
}

#[derive(Debug, Deserialize)]
struct ListRunsQuery {
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

async fn list_runs(
    State(state): State<AppState>,
    Query(q): Query<ListRunsQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = if let Some(t) = q.task_id {
        repo::list_runs_by_task(&conn, &Id::from(t))?
    } else if let Some(s) = q.session_id {
        repo::list_runs_by_session(&conn, &s)?
    } else {
        return Err(DomainError::Validation("task_id or session_id required".into()).into());
    };
    Ok(Json(json!({ "runs": rows })))
}

// ---------------------------------------------------------------------------
// hierarchy
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DecomposeBody {
    /// Optional pre-allocated round id. If omitted, subtasks land on the
    /// parent's round (the common case for inline decomposition).
    #[serde(default)]
    round_id: Option<String>,
    /// New subtasks to insert. Each carries its own short_code/description.
    subtasks: Vec<DecomposeChild>,
}

#[derive(Debug, Deserialize)]
struct DecomposeChild {
    short_code: String,
    description: String,
    #[serde(default)]
    parent_scenario_ids: Vec<String>,
    #[serde(default)]
    parent_requirement_ids: Vec<String>,
}

async fn decompose(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<DecomposeBody>,
) -> ApiResult<Json<Value>> {
    if b.subtasks.is_empty() {
        return Err(DomainError::Validation("subtasks list cannot be empty".into()).into());
    }
    let conn = state.conn()?;
    let parent_tid = Id::from(id);
    let parent = task_repo::get(&conn, &parent_tid)?;
    let round_id = b
        .round_id
        .map(Id::from)
        .unwrap_or_else(|| parent.round_id.clone());

    let plan_id = round_repo::get(&conn, &round_id)?.plan_id;

    // D23 — decomposed children stay within the parent's collaboration
    // context: inherit its pattern. A legacy parent with no provenance falls
    // back to the plan's `direct` sentinel.
    let child_provenance = match &parent.produced_via_pattern_id {
        Some(p) => p.clone(),
        None => provenance::ensure_direct_pattern(&conn, &plan_id)?,
    };

    let mut created: Vec<Task> = Vec::with_capacity(b.subtasks.len());
    for child in b.subtasks {
        let t = Task {
            id: Id::new(IdKind::Task),
            round_id: round_id.clone(),
            plan_id: plan_id.clone(),
            short_code: child.short_code,
            description: child.description,
            status: TaskStatus::Todo,
            parent_scenario_ids: child
                .parent_scenario_ids
                .into_iter()
                .map(Id::from)
                .collect(),
            parent_requirement_ids: child
                .parent_requirement_ids
                .into_iter()
                .map(Id::from)
                .collect(),
            evidence: None,
            produced_via_pattern_id: Some(child_provenance.clone()),
            evidence_at: None,
            created_at: now(),
            updated_at: now(),
        };
        task_repo::insert(&conn, &t)?;
        let rel = TaskRelation {
            id: Id::new(IdKind::TaskRelation),
            parent_id: parent_tid.clone(),
            child_id: t.id.clone(),
            kind: RelationKind::Parent,
            created_at: now(),
        };
        repo::insert_relation(&conn, &rel)?;
        let fresh = task_repo::get(&conn, &t.id)?;
        state.publish(EventEnvelope {
            kind: "task.created".into(),
            entity_id: Some(fresh.id.to_string()),
            payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
        });
        created.push(fresh);
    }
    state.publish(EventEnvelope {
        kind: "task.decomposed".into(),
        entity_id: Some(parent_tid.to_string()),
        payload: json!({
            "parent_id": parent_tid.to_string(),
            "subtask_ids": created.iter().map(|t| t.id.to_string()).collect::<Vec<_>>(),
        }),
    });
    Ok(Json(json!({ "subtasks": created })))
}

async fn ancestors(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let ids = repo::ancestors(&conn, &Id::from(id))?;
    let tasks: Vec<Task> = ids
        .iter()
        .filter_map(|i| task_repo::get(&conn, i).ok())
        .collect();
    Ok(Json(json!({ "ancestors": tasks })))
}

async fn descendants(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let ids = repo::descendants(&conn, &Id::from(id))?;
    let tasks: Vec<Task> = ids
        .iter()
        .filter_map(|i| task_repo::get(&conn, i).ok())
        .collect();
    Ok(Json(json!({ "descendants": tasks })))
}

async fn subtree(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let root = task_repo::get(&conn, &Id::from(id.clone()))?;
    let ids = repo::descendants(&conn, &Id::from(id))?;
    let descendants_tasks: Vec<Task> = ids
        .iter()
        .filter_map(|i| task_repo::get(&conn, i).ok())
        .collect();
    Ok(Json(
        json!({ "root": root, "descendants": descendants_tasks }),
    ))
}

#[derive(Debug, Deserialize)]
struct CreateRelationBody {
    child_id: String,
    #[serde(default = "default_relation_kind")]
    kind: String,
}

fn default_relation_kind() -> String {
    "related".into()
}

async fn create_relation(
    State(state): State<AppState>,
    Path(parent_id): Path<String>,
    Json(b): Json<CreateRelationBody>,
) -> ApiResult<Json<Value>> {
    let kind = RelationKind::from_str(&b.kind)?;
    if matches!(kind, RelationKind::Parent) {
        return Err(DomainError::Validation(
            "use POST /tasks/:id/decompose to create parent relations".into(),
        )
        .into());
    }
    let rel = TaskRelation {
        id: Id::new(IdKind::TaskRelation),
        parent_id: Id::from(parent_id),
        child_id: Id::from(b.child_id),
        kind,
        created_at: now(),
    };
    let conn = state.conn()?;
    repo::insert_relation(&conn, &rel)?;
    state.publish(EventEnvelope {
        kind: "task.relation.created".into(),
        entity_id: Some(rel.id.to_string()),
        payload: serde_json::to_value(&rel).unwrap_or(Value::Null),
    });
    Ok(Json(json!(rel)))
}

async fn list_relations(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = repo::list_relations_for_task(&conn, &Id::from(task_id))?;
    Ok(Json(json!({ "relations": rows })))
}

async fn delete_relation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rid = Id::from(id);
    repo::delete_relation(&conn, &rid)?;
    state.publish(EventEnvelope {
        kind: "task.relation.deleted".into(),
        entity_id: Some(rid.to_string()),
        payload: json!({ "id": rid.to_string() }),
    });
    Ok(Json(json!({ "deleted": rid.to_string() })))
}

// ---------------------------------------------------------------------------
// lease
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LeaseBody {
    holder: String,
    #[serde(default = "default_ttl")]
    ttl_seconds: i64,
}

fn default_ttl() -> i64 {
    120
}

async fn acquire_lease(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<LeaseBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let tid = Id::from(id);
    let ok = repo::acquire_lease(&conn, &tid, &b.holder, b.ttl_seconds)?;
    let lease = repo::get_lease(&conn, &tid)?;
    Ok(Json(json!({ "acquired": ok, "lease": lease })))
}

async fn heartbeat_lease(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<LeaseBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let tid = Id::from(id);
    let ok = repo::heartbeat_lease(&conn, &tid, &b.holder, b.ttl_seconds)?;
    let lease = repo::get_lease(&conn, &tid)?;
    if !ok {
        return Err(DomainError::Validation("not the lease holder".into()).into());
    }
    Ok(Json(json!({ "lease": lease })))
}

#[derive(Debug, Deserialize)]
struct ReleaseBody {
    holder: String,
}

async fn release_lease(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<ReleaseBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let tid = Id::from(id);
    let ok = repo::release_lease(&conn, &tid, &b.holder)?;
    Ok(Json(json!({ "released": ok })))
}

async fn get_lease(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let lease = repo::get_lease(&conn, &Id::from(id))?;
    Ok(Json(json!({ "lease": lease })))
}

// ---------------------------------------------------------------------------
// stats
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct StatsQuery {
    #[serde(default)]
    project_id: Option<String>,
}

async fn stats(
    State(state): State<AppState>,
    Query(q): Query<StatsQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = match q.project_id {
        Some(pid) => repo::status_counts_for_project(&conn, &Id::from(pid))?,
        None => repo::status_counts(&conn)?,
    };
    let items: Vec<_> = rows
        .into_iter()
        .map(|(s, n)| json!({ "status": s, "count": n }))
        .collect();
    Ok(Json(json!({ "by_status": items })))
}
