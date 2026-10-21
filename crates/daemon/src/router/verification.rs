//! Verification-run integrity surface + the shared gates other routers call.
//!
//! Endpoints:
//!   POST /runs/reuse-check          is there a passing run whose fingerprint
//!                                   matches the workspace now? (reuse vs stale)
//!   GET  /retry-budgets             failure counters of a plan / scenario
//!   POST /retry-budgets/release     reset a scenario's counters on the
//!                                   authority of a decision
//!
//! Gates (called by the task / round / run routers):
//!   * [`validate_run_evidence`]  — strict check of a `run:<RUN-ID>` evidence ref
//!   * [`ensure_completion_budget`] — refuse non-proven verdicts on an
//!     exhausted retry budget
//!   * [`ensure_no_unresolved_infrastructure_failures`] — round completion gate

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Query, State},
    routing::{get, post},
    Json, Router,
};
use rusqlite::Connection;
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::Id;
use sdi_core::run::{Run, RunResult};
use sdi_core::scenario::{Scenario, ScenarioResult};
use sdi_core::verification::{EvidenceRef, FailureClass};
use sdi_db::repo::{
    plan as plan_repo, project as project_repo, retry_budget as budget_repo, run as run_repo,
    scenario as scenario_repo,
};
use sdi_db::workspace;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/runs/reuse-check", post(reuse_check))
        .route("/retry-budgets", get(list_budgets))
        .route("/retry-budgets/release", post(release_budget))
}

// ---------------------------------------------------------------------------
// scenario resolution
// ---------------------------------------------------------------------------

/// Resolve a scenario reference: a scenario id, or a plan-scoped short code
/// within `plan_id`, or — with neither — within the active plan of the
/// project anchored at `cwd` (or its nearest anchored ancestor).
pub fn resolve_scenario(
    conn: &Connection,
    reference: &str,
    plan_id: Option<&str>,
    cwd: Option<&str>,
) -> DomainResult<Scenario> {
    let reference = reference.trim();
    if reference.is_empty() {
        return Err(DomainError::Validation(
            "scenario reference is empty".into(),
        ));
    }
    if let Ok(scn) = scenario_repo::get(conn, &Id::from(reference)) {
        return Ok(scn);
    }
    let plan = match plan_id {
        Some(p) => Some(Id::from(p)),
        None => match cwd {
            Some(c) => active_plan_for_cwd(conn, c)?,
            None => None,
        },
    };
    let plan = plan.ok_or_else(|| {
        DomainError::Validation(format!(
            "scenario `{reference}` is not a scenario id; pass --plan (or run inside a \
             project cwd with an active plan) to resolve a short code"
        ))
    })?;
    let sid = scenario_repo::resolve_ref(conn, &plan, reference)?
        .ok_or_else(|| DomainError::NotFound(format!("scenario `{reference}` in plan {plan}")))?;
    scenario_repo::get(conn, &sid)
}

fn active_plan_for_cwd(conn: &Connection, cwd: &str) -> DomainResult<Option<Id>> {
    let mut cur = Some(Path::new(cwd));
    while let Some(dir) = cur {
        if let Some(project) = project_repo::find_by_cwd(conn, &dir.to_string_lossy())? {
            return Ok(plan_repo::find_active_for_project(conn, &project.id)?.map(|p| p.id));
        }
        cur = dir.parent();
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// gates
// ---------------------------------------------------------------------------

/// Strictly validate a `run:<RUN-ID>` evidence reference for `verdict` on
/// `scenario_id` within `plan_id`: the run must exist, be bound to that
/// scenario and plan, be finished, match the verdict (a pass needs exit 0), and
/// its input fingerprint must still match the workspace.
pub fn validate_run_evidence(
    conn: &Connection,
    plan_id: &Id,
    scenario_id: &Id,
    verdict: ScenarioResult,
    run_id: &Id,
) -> DomainResult<Run> {
    let run = run_repo::get_run(conn, run_id)
        .map_err(|_| DomainError::EvidenceRunInvalid(format!("run {run_id} does not exist")))?;
    if run.scenario_id.as_ref() != Some(scenario_id) {
        return Err(DomainError::EvidenceRunInvalid(format!(
            "run {run_id} is not linked to scenario {scenario_id} (linked: {})",
            run.scenario_id
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "none".into())
        )));
    }
    if run.plan_id.as_ref() != Some(plan_id) {
        return Err(DomainError::EvidenceRunInvalid(format!(
            "run {run_id} belongs to another plan"
        )));
    }
    if run.finished_at.is_none() {
        return Err(DomainError::EvidenceRunInvalid(format!(
            "run {run_id} has not finished"
        )));
    }
    match verdict {
        ScenarioResult::Passing => {
            if !run.is_clean_pass() {
                return Err(DomainError::EvidenceRunInvalid(format!(
                    "run {run_id} did not pass (result {}, exit {}, class {}) and cannot evidence `passing`",
                    run.result.map(|r| r.to_string()).unwrap_or_else(|| "none".into()),
                    run.exit_code
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "none".into()),
                    run.failure_class
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "none".into()),
                )));
            }
        }
        ScenarioResult::Failing => {
            if run.result != Some(RunResult::Failure) {
                return Err(DomainError::EvidenceRunInvalid(format!(
                    "run {run_id} did not fail and cannot evidence `failing`"
                )));
            }
        }
        other => {
            return Err(DomainError::EvidenceRunInvalid(format!(
                "a run reference evidences `passing` or `failing`, not `{other}`"
            )));
        }
    }
    let fresh = run_repo::freshness(conn, &run);
    if !fresh.fresh {
        return Err(DomainError::EvidenceStale(format!(
            "run {run_id}: {} — re-run `sdi run verify --scenario {scenario_id} …` against the current inputs",
            fresh.reason.unwrap_or_else(|| "fingerprint mismatch".into())
        )));
    }
    Ok(run)
}

/// On an exhausted retry budget, only a verdict proven by a fresh passing
/// run may complete; anything else (a failing verdict, a free-text pass) is
/// the retry loop the budget exists to stop.
pub fn ensure_completion_budget(
    conn: &Connection,
    plan_id: &Id,
    scenario_id: &Id,
    verdict: ScenarioResult,
    proven_by_run: bool,
) -> DomainResult<()> {
    let exhausted = budget_repo::exhausted_for(conn, plan_id, std::slice::from_ref(scenario_id))?;
    match exhausted.first() {
        Some(_) if verdict == ScenarioResult::Passing && proven_by_run => Ok(()),
        Some(b) => Err(budget_repo::exhausted_error(b)),
        None => Ok(()),
    }
}

/// Validate an evidence reference on any write path: run refs strictly,
/// free text verbatim. Returns whether the verdict is proven by a run.
pub fn check_evidence_ref(
    conn: &Connection,
    plan_id: &Id,
    scenario_id: &Id,
    verdict: ScenarioResult,
    evidence_ref: Option<&str>,
) -> DomainResult<bool> {
    if verdict.is_daemon_assigned() {
        return Err(DomainError::Validation(format!(
            "`{verdict}` is assigned by the daemon (stale carried pass) and cannot be submitted"
        )));
    }
    let Some(raw) = evidence_ref else {
        return Ok(false);
    };
    match EvidenceRef::parse(raw)? {
        EvidenceRef::Run(run_id) => {
            validate_run_evidence(conn, plan_id, scenario_id, verdict, &run_id)?;
            Ok(true)
        }
        EvidenceRef::Free(_) => Ok(false),
    }
}

/// Round completion gate: a scenario whose latest run in the round failed for
/// harness/environment reasons has no product verdict yet — the round cannot
/// complete until a passing run supersedes it.
pub fn ensure_no_unresolved_infrastructure_failures(
    conn: &Connection,
    round_id: &Id,
) -> DomainResult<()> {
    let mut blocked = Vec::new();
    for run in run_repo::latest_finished_per_scenario_in_round(conn, round_id)? {
        let Some(class) = run.failure_class else {
            continue;
        };
        if !class.is_infrastructure() {
            continue;
        }
        let Some(sid) = run.scenario_id.as_ref() else {
            continue;
        };
        let retired = scenario_repo::get(conn, sid)
            .map(|s| s.retired_at.is_some())
            .unwrap_or(true);
        if retired {
            continue;
        }
        blocked.push(format!(
            "{sid} (run {}: {class}/{})",
            run.id,
            run.error_class.as_deref().unwrap_or("unknown")
        ));
    }
    if blocked.is_empty() {
        return Ok(());
    }
    Err(DomainError::HarnessFailureUnresolved(format!(
        "{} — fix the harness/environment and record a passing `sdi run verify` for each",
        blocked.join("; ")
    )))
}

// ---------------------------------------------------------------------------
// reuse check
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ReuseCheckBody {
    scenario: String,
    #[serde(default)]
    plan_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    command: Option<Vec<String>>,
    #[serde(default)]
    input_globs: Vec<String>,
    #[serde(default)]
    tool_versions: BTreeMap<String, String>,
}

async fn reuse_check(
    State(state): State<AppState>,
    Json(b): Json<ReuseCheckBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let scn = resolve_scenario(&conn, &b.scenario, b.plan_id.as_deref(), b.cwd.as_deref())?;
    let passing = run_repo::passing_runs_for_scenario(&conn, &scn.id)?;

    // Candidate mode: "would this exact command over these inputs be a
    // reuse?" — look for a pass with the identical fingerprint.
    if let Some(command) = b.command.as_ref().filter(|c| !c.is_empty()) {
        let cwd = b.cwd.as_deref().ok_or_else(|| {
            DomainError::Validation("reuse check with a command requires cwd".into())
        })?;
        let contract = run_repo::contract_for_scenario(&conn, Some(&scn.id))?;
        let candidate = workspace::fingerprint(
            Path::new(cwd),
            &b.input_globs,
            command,
            &b.tool_versions,
            &contract,
        )?;
        let hit = passing
            .iter()
            .find(|r| r.input_fingerprint.as_deref() == Some(candidate.as_str()));
        let status = match (hit, passing.is_empty()) {
            (Some(_), _) => "fresh",
            (None, true) => "none",
            (None, false) => "stale",
        };
        return Ok(Json(json!({
            "scenario_id": scn.id.to_string(),
            "scenario_short_code": scn.short_code,
            "status": status,
            "reuse_allowed": hit.is_some(),
            "run_id": hit.map(|r| r.id.to_string()),
            "evidence": hit.map(|r| format!("{}=passing@run:{}", scn.id, r.id)),
            "current_fingerprint": candidate,
            "latest_passing_run_id": passing.first().map(|r| r.id.to_string()),
        })));
    }

    // Latest-pass mode: is the most recent passing run still current?
    let Some(latest) = passing.first() else {
        return Ok(Json(json!({
            "scenario_id": scn.id.to_string(),
            "scenario_short_code": scn.short_code,
            "status": "none",
            "reuse_allowed": false,
            "run_id": null,
            "reason": "no passing verification run recorded for this scenario",
        })));
    };
    let f = run_repo::freshness(&conn, latest);
    Ok(Json(json!({
        "scenario_id": scn.id.to_string(),
        "scenario_short_code": scn.short_code,
        "status": if f.fresh { "fresh" } else { "stale" },
        "reuse_allowed": f.fresh,
        "run_id": latest.id.to_string(),
        "evidence": f.fresh.then(|| format!("{}=passing@run:{}", scn.id, latest.id)),
        "recorded_fingerprint": f.recorded,
        "current_fingerprint": f.current,
        "reason": f.reason,
    })))
}

// ---------------------------------------------------------------------------
// retry budgets
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct BudgetQuery {
    #[serde(default)]
    plan_id: Option<String>,
    #[serde(default)]
    scenario: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
}

async fn list_budgets(
    State(state): State<AppState>,
    Query(q): Query<BudgetQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let (plan_id, scenario_id) = match q.scenario.as_deref() {
        Some(r) => {
            let scn = resolve_scenario(&conn, r, q.plan_id.as_deref(), q.cwd.as_deref())?;
            (scn.plan_id.clone(), Some(scn.id))
        }
        None => {
            let plan = match q.plan_id {
                Some(p) => Id::from(p),
                None => q
                    .cwd
                    .as_deref()
                    .map(|c| active_plan_for_cwd(&conn, c))
                    .transpose()?
                    .flatten()
                    .ok_or_else(|| {
                        DomainError::Validation("plan_id, scenario or cwd required".into())
                    })?,
            };
            (plan, None)
        }
    };
    let rows = budget_repo::list(&conn, &plan_id, scenario_id.as_ref())?;
    let cap = budget_repo::cap_for_plan(&conn, &plan_id)?;
    Ok(Json(json!({
        "plan_id": plan_id.to_string(),
        "cap": cap,
        "budgets": rows,
        "exhausted": rows.iter().filter(|b| b.exhausted).count(),
    })))
}

#[derive(Debug, Deserialize)]
struct ReleaseBody {
    scenario: String,
    decision_id: String,
    #[serde(default)]
    plan_id: Option<String>,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
}

async fn release_budget(
    State(state): State<AppState>,
    Json(b): Json<ReleaseBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let scn = resolve_scenario(&conn, &b.scenario, b.plan_id.as_deref(), b.cwd.as_deref())?;
    let decision = Id::from(b.decision_id);
    let n = budget_repo::release(
        &conn,
        &scn.plan_id,
        &scn.id,
        b.signature.as_deref(),
        &decision,
    )?;
    let rows = budget_repo::list(&conn, &scn.plan_id, Some(&scn.id))?;
    state.publish(EventEnvelope {
        kind: "retry_budget.released".into(),
        entity_id: Some(scn.id.to_string()),
        payload: json!({
            "scenario_id": scn.id.to_string(),
            "decision_id": decision.to_string(),
            "released": n,
        }),
    });
    Ok(Json(json!({
        "scenario_id": scn.id.to_string(),
        "decision_id": decision.to_string(),
        "released": n,
        "budgets": rows,
    })))
}

/// Book a finished run's failure against its retry budget (no-op for passes
/// and for runs not bound to a scenario).
pub fn book_failure(conn: &Connection, run: &Run) -> DomainResult<Option<Value>> {
    let (Some(class), Some(sig), Some(plan), Some(scn)) = (
        run.failure_class,
        run.failure_signature.as_deref(),
        run.plan_id.as_ref(),
        run.scenario_id.as_ref(),
    ) else {
        return Ok(None);
    };
    if class == FailureClass::None {
        return Ok(None);
    }
    let b = budget_repo::record_failure(conn, plan, scn, class, sig, &run.id)?;
    Ok(Some(serde_json::to_value(&b).unwrap_or(Value::Null)))
}
