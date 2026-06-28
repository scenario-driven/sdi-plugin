//! `/plans` router. Owns lifecycle transitions:
//! - POST `/plans`               — create (always `draft`)
//! - PUT  `/plans/:id`           — SNAPSHOT body update (bumps `version`)
//! - POST `/plans/:id/approve`   — D8 gate (≥1 confirmed scenario) → `active`
//! - POST `/plans/:id/complete`  — `active` → `completed`
//!
//! All transitions publish a `plan.<state>` SSE event.

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use sdi_core::error::DomainError;
use sdi_core::ids::{now, Id, IdKind};
use sdi_core::plan::{Plan, PlanStatus};
use sdi_core::user_flow::{FlowStatus, UserFlow};
use sdi_db::repo::{
    decision_question as dq_repo, plan as repo, plan_flow as plan_flow_repo, scenario as scn_repo,
    user_flow as uf_repo,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/plans", post(create).get(list))
        .route("/plans/:id", get(get_one).put(update))
        .route("/plans/:id/approve", post(approve))
        .route("/plans/:id/complete", post(complete))
        .route(
            "/projects/:project_id/plans/active",
            get(active_for_project),
        )
}

#[derive(Debug, Deserialize)]
struct CreatePlanBody {
    project_id: String,
    short_code: String,
    title: String,
    #[serde(default)]
    body: String,
}

async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreatePlanBody>,
) -> ApiResult<Json<Value>> {
    let plan = Plan {
        id: Id::new(IdKind::Plan),
        project_id: Id::from(req.project_id),
        short_code: req.short_code,
        title: req.title,
        body: req.body,
        status: PlanStatus::Draft,
        version: 0,
        // D23 — the plan shell is the root, with no work yet. Minting a
        // `direct` sentinel at plan birth would mark every plan an
        // anti-pattern before any work runs. Provenance is resolved lazily by
        // the first solo-produced work entity (scenario/round/…); migration
        // 009 back-fills this column for plans that already had solo work.
        produced_via_pattern_id: None,
        approved_at: None,
        completed_at: None,
        created_at: now(),
        updated_at: now(),
    };
    let conn = state.conn()?;
    repo::insert(&conn, &plan)?;
    let fresh = repo::get(&conn, &plan.id)?;
    state.publish(EventEnvelope {
        kind: "plan.created".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct ListPlansQuery {
    project_id: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListPlansQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = match q.project_id {
        Some(pid) => repo::list_by_project(&conn, &Id::from(pid))?,
        None => {
            return Err(
                DomainError::Validation("project_id query parameter required".into()).into(),
            )
        }
    };
    Ok(Json(json!({ "plans": rows })))
}

async fn get_one(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    Ok(Json(json!(repo::get(&conn, &Id::from(id))?)))
}

#[derive(Debug, Deserialize)]
struct UpdatePlanBody {
    title: String,
    body: String,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<UpdatePlanBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let pid = Id::from(id);
    let version = repo::update_body(&conn, &pid, &b.title, &b.body)?;
    let fresh = repo::get(&conn, &pid)?;
    state.publish(EventEnvelope {
        kind: "plan.updated".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: json!({ "version": version, "plan": fresh }),
    });
    Ok(Json(json!(fresh)))
}

async fn approve(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let pid = Id::from(id);
    let plan = repo::get(&conn, &pid)?;

    // D34 (supersedes D8) — once a plan targets UserFlows it is "oracle-scoped"
    // and approve enforces L2 step coverage. A plan with no targeted flow falls
    // back to the legacy D8 gate during the v1→v2 transition; D8 becomes a dead
    // path when every plan is flow-scoped.
    let flow_ids = plan_flow_repo::list_flow_ids_for_plan(&conn, &pid)?;
    if flow_ids.is_empty() {
        let confirmed = repo::count_confirmed_scenarios(&conn, &pid)?;
        plan.check_can_approve(confirmed >= 1)?;
    } else {
        let mut uncovered: Vec<String> = Vec::new();
        let mut scoped_refs: Vec<String> = vec![pid.as_str().to_string()];
        for fid in &flow_ids {
            scoped_refs.push(fid.as_str().to_string());
            let flow = uf_repo::get(&conn, fid)?;
            if flow.status != FlowStatus::Confirmed {
                uncovered.push(format!("flow {} not confirmed", flow.short_code));
                continue;
            }
            let covered: HashSet<String> = scn_repo::covered_flow_steps(&conn, &pid, fid)?
                .into_iter()
                .collect();
            for step in UserFlow::parse_steps(&flow.steps_json).unwrap_or_default() {
                if !covered.contains(&step.idx.to_string()) {
                    uncovered.push(format!(
                        "flow {} step {} uncovered",
                        flow.short_code, step.idx
                    ));
                }
            }
        }
        let open_scoped = dq_repo::count_open_scoped(&conn, &scoped_refs)?;
        if !uncovered.is_empty() || open_scoped > 0 {
            return Err(DomainError::Validation(format!(
                "plan approve blocked (D34): {} L2 gap(s), {} open decision question(s) — {}",
                uncovered.len(),
                open_scoped,
                uncovered.join("; ")
            ))
            .into());
        }
        // Retain the draft-status transition guard.
        plan.check_can_approve(true)?;
    }

    let approved_at = now();
    repo::set_status(&conn, &pid, PlanStatus::Active, Some(approved_at), None)?;
    let fresh = repo::get(&conn, &pid)?;
    state.publish(EventEnvelope {
        kind: "plan.approved".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

async fn complete(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let pid = Id::from(id);
    let plan = repo::get(&conn, &pid)?;
    plan.check_can_complete()?;
    let completed_at = now();
    repo::set_status(&conn, &pid, PlanStatus::Completed, None, Some(completed_at))?;
    let fresh = repo::get(&conn, &pid)?;
    state.publish(EventEnvelope {
        kind: "plan.completed".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

async fn active_for_project(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let found = repo::find_active_for_project(&conn, &Id::from(project_id))?;
    Ok(Json(json!({ "plan": found })))
}
