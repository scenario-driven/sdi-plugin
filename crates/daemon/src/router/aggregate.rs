//! Aggregate views + observability + import/export + events replay.
//!
//! Each handler is read-mostly; only `/usage` POST mutates. The router
//! is mounted alongside the per-entity routers in `router::build`.
//!
//! Endpoints (Phase C surface):
//!   GET  /dashboard?cwd=… | ?project=…   active plan + counts + recent activity
//!   GET  /projects/:id/handoff           session pickup bundle
//!   GET  /projects/:id/timeline (alias)  → mirrors collab activity stream
//!   GET  /metrics                        Prometheus text format
//!   POST /usage                          insert a usage record
//!   GET  /usage?project_id=|plan_id=|task_id=
//!   GET  /plans/:id/usage                rollup
//!   GET  /tasks/:id/usage/preflight      heuristic cost estimate
//!   GET  /plans/export?project_id=       JSON bundle of all plan-scoped state
//!   POST /plans/import                   reverse of export (best-effort upsert)
//!   GET  /knowledge/export?project_id=
//!   POST /knowledge/import
//!   GET  /events/replay?project_id=&since=&limit=

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::DateTime;
use sdi_core::error::DomainError;
use sdi_core::ids::{now, Id, IdKind};
use sdi_core::knowledge::{Knowledge, KnowledgeScope};
use sdi_core::usage::{UsageRecord, UsageRollup, UsageTier};
use sdi_db::repo::{
    collab as collab_repo, decision as decision_repo, event as event_repo,
    knowledge as knowledge_repo, plan as plan_repo, project as project_repo,
    requirement as requirement_repo, round as round_repo, run as run_repo,
    scenario as scenario_repo, task as task_repo, usage as usage_repo,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/dashboard", get(dashboard))
        .route("/projects/:id/handoff", get(handoff))
        .route("/metrics", get(metrics))
        .route("/usage", post(record_usage).get(list_usage))
        .route("/plans/:id/usage", get(plan_usage))
        .route("/tasks/:id/usage/preflight", get(task_preflight))
        .route("/plans/export", get(export_plans))
        .route("/plans/import", post(import_plans))
        .route("/knowledge/export", get(export_knowledge))
        .route("/knowledge/import", post(import_knowledge))
        .route("/events/replay", get(events_replay))
}

// ---------------------------------------------------------------------------
// dashboard
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DashboardQuery {
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    project: Option<String>,
}

async fn dashboard(
    State(state): State<AppState>,
    Query(q): Query<DashboardQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let project = if let Some(p) = q.project.as_deref() {
        // accept either id ("PROJ-…") or key ("LM")
        if p.starts_with("PROJ-") {
            Some(project_repo::get(&conn, &Id::from(p))?)
        } else {
            project_repo::find_by_key(&conn, p)?
        }
    } else if let Some(cwd) = q.cwd.as_deref() {
        project_repo::find_by_cwd(&conn, cwd)?
    } else {
        // first project, or none
        project_repo::list(&conn)?.into_iter().next()
    };

    let project = match project {
        Some(p) => p,
        None => {
            return Ok(Json(json!({
                "project": null,
                "active_plan": null,
                "counts": {},
                "recent_activity": [],
            })));
        }
    };

    let active_plan = plan_repo::find_active_for_project(&conn, &project.id)?;
    let plans = plan_repo::list_by_project(&conn, &project.id)?;

    let mut requirements = 0i64;
    let mut decisions = 0i64;
    let mut scenarios = 0i64;
    let mut rounds = 0i64;
    let mut in_flight_tasks: Vec<sdi_core::task::Task> = Vec::new();
    let mut backlog_tasks: Vec<sdi_core::task::Task> = Vec::new();
    for p in &plans {
        requirements += requirement_repo::list_by_plan(&conn, &p.id)?.len() as i64;
        decisions += decision_repo::list_by_plan(&conn, &p.id)?.len() as i64;
        scenarios += scenario_repo::list_by_plan(&conn, &p.id)?.len() as i64;
        rounds += round_repo::list_by_plan(&conn, &p.id)?.len() as i64;
        in_flight_tasks.extend(task_repo::list_in_flight_for_plan(&conn, &p.id)?);
        backlog_tasks.extend(task_repo::list_backlog_for_plan(&conn, &p.id)?);
    }

    let recent_activity = collab_repo::list_activity_by_project(&conn, &project.id, 20)?;
    let usage = usage_repo::rollup_for_project(&conn, &project.id)?;
    let task_status = run_repo::status_counts(&conn)?;

    Ok(Json(json!({
        "project": project,
        "active_plan": active_plan,
        "plans": plans,
        "counts": {
            "plans": plans.len(),
            "requirements": requirements,
            "decisions": decisions,
            "scenarios": scenarios,
            "rounds": rounds,
            "in_flight_tasks": in_flight_tasks.len(),
            "backlog_tasks": backlog_tasks.len(),
        },
        "task_status": task_status.into_iter()
            .map(|(s, n)| json!({"status": s, "count": n}))
            .collect::<Vec<_>>(),
        "in_flight_tasks": in_flight_tasks,
        "backlog_tasks": backlog_tasks,
        "recent_activity": recent_activity,
        "usage": usage,
    })))
}

// ---------------------------------------------------------------------------
// handoff
// ---------------------------------------------------------------------------

async fn handoff(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let project = project_repo::get(&conn, &Id::from(id))?;
    let active_plan = plan_repo::find_active_for_project(&conn, &project.id)?;

    // Last 24h of activity is enough for session pickup.
    let activity = collab_repo::list_activity_by_project(&conn, &project.id, 50)?;

    let mut scenarios = Vec::new();
    let mut in_flight = Vec::new();
    let mut backlog = Vec::new();
    let mut recent_decisions = Vec::new();
    if let Some(p) = active_plan.as_ref() {
        scenarios = scenario_repo::list_by_plan(&conn, &p.id)?;
        in_flight = task_repo::list_in_flight_for_plan(&conn, &p.id)?;
        backlog = task_repo::list_backlog_for_plan(&conn, &p.id)?;
        recent_decisions = decision_repo::list_by_plan(&conn, &p.id)?;
    }

    Ok(Json(json!({
        "project": project,
        "active_plan": active_plan,
        "scenarios": scenarios,
        "in_flight_tasks": in_flight,
        "backlog_tasks": backlog,
        "recent_decisions": recent_decisions,
        "recent_activity": activity,
    })))
}

// ---------------------------------------------------------------------------
// metrics (Prometheus text)
// ---------------------------------------------------------------------------

async fn metrics(State(state): State<AppState>) -> ApiResult<axum::response::Response> {
    let conn = state.conn()?;
    let status = run_repo::status_counts(&conn)?;
    let projects = project_repo::list(&conn)?;

    let mut buf = String::new();
    buf.push_str("# HELP sdi_tasks_total Number of tasks by status\n");
    buf.push_str("# TYPE sdi_tasks_total gauge\n");
    for (s, n) in &status {
        buf.push_str(&format!("sdi_tasks_total{{status=\"{s}\"}} {n}\n"));
    }

    buf.push_str("# HELP sdi_projects_total Total projects registered\n");
    buf.push_str("# TYPE sdi_projects_total gauge\n");
    buf.push_str(&format!("sdi_projects_total {}\n", projects.len()));

    let mut total_cost = 0.0f64;
    let mut total_records = 0i64;
    let mut total_tokens_in = 0i64;
    let mut total_tokens_out = 0i64;
    for p in &projects {
        let r = usage_repo::rollup_for_project(&conn, &p.id)?;
        total_cost += r.cost_usd;
        total_records += r.records;
        total_tokens_in += r.input_tokens;
        total_tokens_out += r.output_tokens;
    }
    buf.push_str("# HELP sdi_usage_cost_usd_total Total token cost in USD\n");
    buf.push_str("# TYPE sdi_usage_cost_usd_total counter\n");
    buf.push_str(&format!("sdi_usage_cost_usd_total {total_cost}\n"));
    buf.push_str("# HELP sdi_usage_records_total Total usage records emitted\n");
    buf.push_str("# TYPE sdi_usage_records_total counter\n");
    buf.push_str(&format!("sdi_usage_records_total {total_records}\n"));
    buf.push_str("# HELP sdi_usage_input_tokens_total Input tokens across all runs\n");
    buf.push_str("# TYPE sdi_usage_input_tokens_total counter\n");
    buf.push_str(&format!("sdi_usage_input_tokens_total {total_tokens_in}\n"));
    buf.push_str("# HELP sdi_usage_output_tokens_total Output tokens across all runs\n");
    buf.push_str("# TYPE sdi_usage_output_tokens_total counter\n");
    buf.push_str(&format!("sdi_usage_output_tokens_total {total_tokens_out}\n"));

    Ok(axum::response::Response::builder()
        .header("content-type", "text/plain; version=0.0.4")
        .body(axum::body::Body::from(buf))
        .unwrap())
}

// ---------------------------------------------------------------------------
// usage records
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RecordUsageBody {
    project_id: String,
    #[serde(default)]
    plan_id: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    model: String,
    #[serde(default)]
    tier: Option<String>,
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    cache_read: i64,
    #[serde(default)]
    cache_write: i64,
    #[serde(default)]
    tool_calls: i64,
    #[serde(default)]
    cost_usd: f64,
}

async fn record_usage(
    State(state): State<AppState>,
    Json(b): Json<RecordUsageBody>,
) -> ApiResult<Json<Value>> {
    let tier = b
        .tier
        .as_deref()
        .map(UsageTier::from_str)
        .transpose()?
        .unwrap_or_default();
    let rec = UsageRecord {
        id: Id::new(IdKind::UsageRecord),
        project_id: Id::from(b.project_id),
        plan_id: b.plan_id.map(Id::from),
        task_id: b.task_id.map(Id::from),
        run_id: b.run_id.map(Id::from),
        model: b.model,
        tier,
        input_tokens: b.input_tokens,
        output_tokens: b.output_tokens,
        cache_read: b.cache_read,
        cache_write: b.cache_write,
        tool_calls: b.tool_calls,
        cost_usd: b.cost_usd,
        created_at: now(),
    };
    let conn = state.conn()?;
    usage_repo::insert(&conn, &rec)?;
    state.publish(EventEnvelope {
        kind: "usage.recorded".into(),
        entity_id: Some(rec.id.to_string()),
        payload: serde_json::to_value(&rec).unwrap_or(Value::Null),
    });
    Ok(Json(json!(rec)))
}

#[derive(Debug, Deserialize)]
struct ListUsageQuery {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    plan_id: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
}

async fn list_usage(
    State(state): State<AppState>,
    Query(q): Query<ListUsageQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = if let Some(t) = q.task_id {
        usage_repo::list_by_task(&conn, &Id::from(t))?
    } else if let Some(p) = q.plan_id {
        usage_repo::list_by_plan(&conn, &Id::from(p))?
    } else if let Some(p) = q.project_id {
        usage_repo::list_by_project(&conn, &Id::from(p))?
    } else {
        return Err(DomainError::Validation("project_id, plan_id, or task_id required".into()).into());
    };
    Ok(Json(json!({ "usage_records": rows })))
}

async fn plan_usage(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let pid = Id::from(id);
    let rollup: UsageRollup = usage_repo::rollup_for_plan(&conn, &pid)?;
    let records = usage_repo::list_by_plan(&conn, &pid)?;
    Ok(Json(json!({ "rollup": rollup, "records": records })))
}

#[derive(Debug, Deserialize)]
struct PreflightQuery {
    #[serde(default)]
    tier: Option<String>,
}

async fn task_preflight(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<PreflightQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let tid = Id::from(id);
    let task = task_repo::get(&conn, &tid)?;
    // Resolve project_id via round → plan → project.
    let round = round_repo::get(&conn, &task.round_id)?;
    let plan = plan_repo::get(&conn, &round.plan_id)?;
    let tier = match q.tier.as_deref() {
        Some(t) => UsageTier::from_str(t)?,
        None => UsageTier::from_str(&task.short_code).unwrap_or_default(),
    };
    let avg = usage_repo::preflight_avg_for_tier(&conn, &plan.project_id, tier)?;
    Ok(Json(json!({
        "task_id": task.id,
        "tier": tier.to_string(),
        "historical_avg_cost_usd": avg,
        "samples_present": avg.is_some(),
    })))
}

// ---------------------------------------------------------------------------
// plan import/export
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ProjectScopedQuery {
    project_id: String,
}

async fn export_plans(
    State(state): State<AppState>,
    Query(q): Query<ProjectScopedQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let pid = Id::from(q.project_id);
    let project = project_repo::get(&conn, &pid)?;
    let plans = plan_repo::list_by_project(&conn, &pid)?;
    let mut bundle = Vec::with_capacity(plans.len());
    for p in plans {
        let requirements = requirement_repo::list_by_plan(&conn, &p.id)?;
        let decisions = decision_repo::list_by_plan(&conn, &p.id)?;
        let scenarios = scenario_repo::list_by_plan(&conn, &p.id)?;
        let rounds = round_repo::list_by_plan(&conn, &p.id)?;
        let mut tasks = Vec::new();
        for r in &rounds {
            tasks.extend(task_repo::list_by_round(&conn, &r.id)?);
        }
        bundle.push(json!({
            "plan": p,
            "requirements": requirements,
            "decisions": decisions,
            "scenarios": scenarios,
            "rounds": rounds,
            "tasks": tasks,
        }));
    }
    Ok(Json(json!({
        "version": 1,
        "project": project,
        "plans": bundle,
    })))
}

#[derive(Debug, Deserialize)]
struct ImportPlansBody {
    #[serde(default)]
    project_id: Option<String>,
    plans: Vec<Value>,
}

async fn import_plans(
    State(state): State<AppState>,
    Json(b): Json<ImportPlansBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let mut imported_plans = 0;
    let mut imported_requirements = 0;
    let mut imported_decisions = 0;
    let mut imported_scenarios = 0;
    let mut imported_rounds = 0;
    let mut imported_tasks = 0;

    for entry in b.plans {
        if let Some(plan_v) = entry.get("plan") {
            if let Ok(plan) = serde_json::from_value::<sdi_core::plan::Plan>(plan_v.clone()) {
                let mut plan = plan;
                if let Some(pid) = &b.project_id {
                    plan.project_id = Id::from(pid.clone());
                }
                // Best-effort: ignore conflicts (already-present plans).
                if plan_repo::insert(&conn, &plan).is_ok() {
                    imported_plans += 1;
                }
            }
        }
        if let Some(arr) = entry.get("requirements").and_then(|v| v.as_array()) {
            for v in arr {
                if let Ok(r) = serde_json::from_value::<sdi_core::requirement::Requirement>(v.clone()) {
                    if requirement_repo::insert(&conn, &r).is_ok() {
                        imported_requirements += 1;
                    }
                }
            }
        }
        if let Some(arr) = entry.get("decisions").and_then(|v| v.as_array()) {
            for v in arr {
                if let Ok(d) = serde_json::from_value::<sdi_core::decision::Decision>(v.clone()) {
                    if decision_repo::insert(&conn, &d).is_ok() {
                        imported_decisions += 1;
                    }
                }
            }
        }
        if let Some(arr) = entry.get("scenarios").and_then(|v| v.as_array()) {
            for v in arr {
                if let Ok(s) = serde_json::from_value::<sdi_core::scenario::Scenario>(v.clone()) {
                    if scenario_repo::insert(&conn, &s).is_ok() {
                        imported_scenarios += 1;
                    }
                }
            }
        }
        if let Some(arr) = entry.get("rounds").and_then(|v| v.as_array()) {
            for v in arr {
                if let Ok(r) = serde_json::from_value::<sdi_core::round::Round>(v.clone()) {
                    if round_repo::insert(&conn, &r).is_ok() {
                        imported_rounds += 1;
                    }
                }
            }
        }
        if let Some(arr) = entry.get("tasks").and_then(|v| v.as_array()) {
            for v in arr {
                if let Ok(t) = serde_json::from_value::<sdi_core::task::Task>(v.clone()) {
                    if task_repo::insert(&conn, &t).is_ok() {
                        imported_tasks += 1;
                    }
                }
            }
        }
    }

    Ok(Json(json!({
        "imported": {
            "plans": imported_plans,
            "requirements": imported_requirements,
            "decisions": imported_decisions,
            "scenarios": imported_scenarios,
            "rounds": imported_rounds,
            "tasks": imported_tasks,
        }
    })))
}

// ---------------------------------------------------------------------------
// knowledge import/export
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ExportKnowledgeQuery {
    project_id: String,
    #[serde(default)]
    scope: Option<String>,
}

async fn export_knowledge(
    State(state): State<AppState>,
    Query(q): Query<ExportKnowledgeQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let scope = q.scope.as_deref().map(KnowledgeScope::from_str).transpose()?;
    let items = knowledge_repo::list_by_project(&conn, &Id::from(q.project_id), scope)?;
    Ok(Json(json!({
        "version": 1,
        "knowledge": items,
    })))
}

#[derive(Debug, Deserialize)]
struct ImportKnowledgeBody {
    #[serde(default)]
    project_id: Option<String>,
    knowledge: Vec<Value>,
}

async fn import_knowledge(
    State(state): State<AppState>,
    Json(b): Json<ImportKnowledgeBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let mut imported = 0;
    for v in b.knowledge {
        if let Ok(mut k) = serde_json::from_value::<Knowledge>(v) {
            if let Some(pid) = &b.project_id {
                k.project_id = Id::from(pid.clone());
            }
            if knowledge_repo::insert(&conn, &k).is_ok() {
                imported += 1;
            }
        }
    }
    Ok(Json(json!({ "imported": imported })))
}

// ---------------------------------------------------------------------------
// events replay
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct EventsReplayQuery {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    since: Option<String>,
    #[serde(default = "default_replay_limit")]
    limit: usize,
}

fn default_replay_limit() -> usize {
    200
}

async fn events_replay(
    State(state): State<AppState>,
    Query(q): Query<EventsReplayQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let since = match q.since {
        Some(s) => Some(
            DateTime::parse_from_rfc3339(&s)
                .map_err(|e| DomainError::Validation(format!("invalid 'since' timestamp: {e}")))?
                .with_timezone(&chrono::Utc),
        ),
        None => None,
    };
    let project_id = q.project_id.map(Id::from);
    let events = event_repo::tail_since(&conn, project_id.as_ref(), since, q.limit)?;
    Ok(Json(json!({ "events": events })))
}
