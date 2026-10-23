//! `/runs` + task hierarchy + task lease routers.
//!
//! Endpoints:
//!   POST /runs                          start a run (task run, or a
//!                                       scenario-bound verification run with
//!                                       command/cwd/inputs → fingerprint)
//!   POST /runs/:id/finish               record terminal state (legacy result,
//!                                       or exit status → classification,
//!                                       signature, report digest, budget)
//!   GET  /runs/:id, /runs               read
//!   POST /tasks/:id/decompose           create subtask(s) under a parent
//!   POST /tasks/:id/subtasks            same shape (alias used by Clawket CLI)
//!   GET  /tasks/:id/ancestors|descendants|subtree
//!   POST /tasks/:id/relations           add a non-parent relation
//!   DELETE /relations/:id
//!   POST /tasks/:id/lease  + /lease/heartbeat + /lease/release
//!   GET  /tasks/stats                   status histogram

use crate::router::{provenance, verification};
use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post},
    Json, Router,
};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id, IdKind};
use sdi_core::run::{RelationKind, Run, RunResult, TaskRelation};
use sdi_core::task::{Task, TaskStatus};
use sdi_core::verification::{
    caller_signature, classify, derive_failure_cause, failure_signature, sha256_hex, ExecOutcome,
    FailureClass, SpawnError,
};
use sdi_db::repo::retry_budget as budget_repo;
use sdi_db::repo::round as round_repo;
use sdi_db::repo::run::{self as repo, RunFinish};
use sdi_db::repo::task as task_repo;
use sdi_db::workspace;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
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
    /// Task the run executes under. Optional when `scenario` is given (a
    /// verification run is scenario-scoped; the daemon links the single
    /// in-progress task that owns the scenario, if there is one).
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default = "default_actor")]
    actor: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
    /// Scenario id or plan-scoped short code this run verifies.
    #[serde(default)]
    scenario: Option<String>,
    #[serde(default)]
    plan_id: Option<String>,
    /// Verification command argv. When present the daemon computes the input
    /// fingerprint and assigns the report path.
    #[serde(default)]
    command: Option<Vec<String>>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    input_globs: Vec<String>,
    #[serde(default)]
    tool_versions: BTreeMap<String, String>,
}

fn default_actor() -> String {
    "agent".into()
}

async fn start_run(
    State(state): State<AppState>,
    Json(b): Json<StartRunBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let task = b
        .task_id
        .as_deref()
        .map(|t| task_repo::get(&conn, &Id::from(t)))
        .transpose()?;
    let scenario = match b.scenario.as_deref() {
        Some(r) => Some(verification::resolve_scenario(
            &conn,
            r,
            b.plan_id
                .as_deref()
                .or(task.as_ref().map(|t| t.plan_id.as_str())),
            b.cwd.as_deref(),
        )?),
        None => None,
    };
    if task.is_none() && scenario.is_none() {
        return Err(DomainError::Validation("task_id or scenario required".into()).into());
    }

    // Link task ↔ scenario. An explicit task must own the scenario; without
    // one, the single in-progress task of the plan that owns it is linked.
    let task = match (task, scenario.as_ref()) {
        (Some(t), Some(scn)) => {
            if !t.parent_scenario_ids.contains(&scn.id) {
                return Err(DomainError::Validation(format!(
                    "task {} does not own scenario {} (parents: [{}])",
                    t.id,
                    scn.id,
                    t.parent_scenario_ids
                        .iter()
                        .map(|i| i.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
                .into());
            }
            Some(t)
        }
        (None, Some(scn)) => {
            let owners: Vec<Task> = task_repo::list_in_flight_for_plan(&conn, &scn.plan_id)?
                .into_iter()
                .filter(|t| {
                    t.status == TaskStatus::InProgress && t.parent_scenario_ids.contains(&scn.id)
                })
                .collect();
            if owners.len() == 1 {
                owners.into_iter().next()
            } else {
                None
            }
        }
        (t, None) => t,
    };
    let plan_id = scenario
        .as_ref()
        .map(|s| s.plan_id.clone())
        .or_else(|| task.as_ref().map(|t| t.plan_id.clone()));
    let round_id = match (&task, &plan_id) {
        (Some(t), _) => Some(t.round_id.clone()),
        (None, Some(p)) => round_repo::find_active_for_plan(&conn, p)?.map(|r| r.id),
        (None, None) => None,
    };

    let run_id = Id::new(IdKind::Run);
    let command = b.command.filter(|c| !c.is_empty());
    let (input_fingerprint, report_path) = match command.as_ref() {
        Some(cmd) => {
            let scn = scenario.as_ref().ok_or_else(|| {
                DomainError::Validation("a verification command requires a scenario".into())
            })?;
            let cwd = b.cwd.as_deref().ok_or_else(|| {
                DomainError::Validation("a verification command requires cwd".into())
            })?;
            if !std::path::Path::new(cwd).is_absolute() {
                return Err(
                    DomainError::Validation(format!("cwd must be absolute, got `{cwd}`")).into(),
                );
            }
            let contract = repo::contract_for_scenario(&conn, Some(&scn.id))?;
            let fp = workspace::fingerprint(
                std::path::Path::new(cwd),
                &b.input_globs,
                cmd,
                &b.tool_versions,
                &contract,
            )?;
            let dir = state.paths.state.join("reports");
            std::fs::create_dir_all(&dir).map_err(|e| {
                DomainError::Validation(format!("create report dir {}: {e}", dir.display()))
            })?;
            let path = dir.join(format!("{run_id}.log"));
            (Some(fp), Some(path.to_string_lossy().into_owned()))
        }
        None => (None, None),
    };

    let run = Run {
        id: run_id,
        task_id: task.as_ref().map(|t| t.id.clone()),
        plan_id,
        round_id,
        scenario_id: scenario.as_ref().map(|s| s.id.clone()),
        actor: b.actor,
        session_id: b.session_id,
        started_at: now(),
        finished_at: None,
        result: None,
        notes: None,
        payload: b.payload.unwrap_or(Value::Null),
        command,
        cwd: b.cwd,
        input_globs: b.input_globs,
        tool_versions: b.tool_versions,
        input_fingerprint,
        exit_code: None,
        signal: None,
        timed_out: false,
        spawn_error: None,
        report_path,
        report_sha256: None,
        failure_class: None,
        failure_signature: None,
        failure_identity: None,
        error_class: None,
        failing_test: None,
        duration_ms: None,
    };
    repo::insert_run(&conn, &run)?;
    let fresh = repo::get_run(&conn, &run.id)?;
    let exhausted = match (&fresh.plan_id, &fresh.scenario_id) {
        (Some(p), Some(s)) => budget_repo::exhausted_for(&conn, p, std::slice::from_ref(s))?,
        _ => Vec::new(),
    };
    state.publish(EventEnvelope {
        kind: "run.started".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    let mut out = json!(fresh);
    out["exhausted_retry_budgets"] = json!(exhausted);
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
struct FinishRunBody {
    /// `success` | `failure` | `aborted`. Optional for structured finishes —
    /// derived from the exit status when omitted.
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    signal: Option<i32>,
    #[serde(default)]
    timed_out: Option<bool>,
    #[serde(default)]
    spawn_error: Option<String>,
    /// Caller override of the classified failure class.
    #[serde(default)]
    failure_class: Option<String>,
    /// Advisory hint for executions; explicit signature for manual runs.
    #[serde(default)]
    failure_signature: Option<String>,
    #[serde(default)]
    duration_ms: Option<i64>,
}

impl FinishRunBody {
    fn is_structured(&self) -> bool {
        self.exit_code.is_some()
            || self.signal.is_some()
            || self.timed_out.is_some()
            || self.spawn_error.is_some()
            || self.failure_class.is_some()
            || self.failure_signature.is_some()
    }
}

/// Cap on how much of a report is scanned for classification (the tail —
/// failure summaries print last). The digest always covers the whole file.
const REPORT_SCAN_BYTES: u64 = 4 * 1024 * 1024;

fn read_report(path: &std::path::Path) -> DomainResult<(String, String, bool)> {
    use std::io::{Read, Seek, SeekFrom};
    let bytes = std::fs::read(path)
        .map_err(|e| DomainError::Validation(format!("read report {}: {e}", path.display())))?;
    let digest = sha256_hex(&bytes);
    let mut f = std::fs::File::open(path)
        .map_err(|e| DomainError::Validation(format!("open report {}: {e}", path.display())))?;
    let len = bytes.len() as u64;
    if len > REPORT_SCAN_BYTES {
        f.seek(SeekFrom::Start(len - REPORT_SCAN_BYTES))
            .map_err(|e| DomainError::Validation(format!("seek report: {e}")))?;
    }
    let mut tail = Vec::new();
    f.read_to_end(&mut tail)
        .map_err(|e| DomainError::Validation(format!("read report: {e}")))?;
    Ok((
        format!("sha256:{digest}"),
        String::from_utf8_lossy(&tail).into_owned(),
        len > REPORT_SCAN_BYTES,
    ))
}

async fn finish_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<FinishRunBody>,
) -> ApiResult<Json<Value>> {
    let mut conn = state.conn()?;
    let rid = Id::from(id);
    let run = repo::get_run(&conn, &rid)?;
    if run.finished_at.is_some() {
        return Err(DomainError::Validation(format!("run {rid} already finished")).into());
    }

    if !b.is_structured() && run.command.is_none() {
        let raw = b.result.as_deref().ok_or_else(|| {
            DomainError::Validation("result required (success|failure|aborted)".into())
        })?;
        let result = RunResult::from_str(raw)?;
        repo::finish_run(&conn, &rid, result, b.notes.as_deref())?;
        let fresh = repo::get_run(&conn, &rid)?;
        state.publish(EventEnvelope {
            kind: "run.finished".into(),
            entity_id: Some(fresh.id.to_string()),
            payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
        });
        return Ok(Json(json!(fresh)));
    }

    let outcome = ExecOutcome {
        exit_code: b.exit_code,
        signal: b.signal,
        timed_out: b.timed_out.unwrap_or(false),
        spawn_error: b
            .spawn_error
            .as_deref()
            .map(SpawnError::from_str)
            .transpose()?,
    };
    let (report_sha256, output, report_scan_truncated) = match run.report_path.as_deref() {
        Some(p) if std::path::Path::new(p).is_file() => {
            let path = std::path::Path::new(p);
            if !path.starts_with(&state.paths.state) {
                return Err(DomainError::Validation(format!(
                    "report path {p} is outside the daemon state directory"
                ))
                .into());
            }
            let (d, t, truncated) = read_report(path)?;
            (Some(d), t, truncated)
        }
        _ => (None, String::new(), false),
    };
    let cwd = run.cwd.as_deref().map(std::path::PathBuf::from);
    let top = cwd.as_deref().and_then(workspace::toplevel);
    let in_repo = |raw: &str| match (top.as_deref(), cwd.as_deref()) {
        (Some(t), Some(c)) => workspace::in_repo_path(t, c, raw),
        _ => None,
    };
    let classification = classify(&outcome, &output, &in_repo);

    let result = match b.result.as_deref() {
        Some(raw) => RunResult::from_str(raw)?,
        None if outcome.passed() => RunResult::Success,
        None => RunResult::Failure,
    };
    if result == RunResult::Success && !outcome.passed() {
        return Err(DomainError::Validation(format!(
            "result `success` contradicts the exit status ({})",
            classification.reason
        ))
        .into());
    }
    let class = match b.failure_class.as_deref() {
        Some(raw) => FailureClass::from_str(raw)?,
        None if result == RunResult::Success => FailureClass::None,
        None => classification.failure_class,
    };
    match (result, class) {
        (RunResult::Success, c) if c.is_failure() => {
            return Err(DomainError::Validation(format!(
                "a successful run cannot carry failure class `{c}`"
            ))
            .into())
        }
        (RunResult::Failure, FailureClass::None) => {
            return Err(DomainError::Validation(
                "a failed run needs a failure class other than `none`".into(),
            )
            .into())
        }
        _ => {}
    }
    // Aborted runs (operator cancel) carry no verdict and book no budget.
    let class = if result == RunResult::Aborted {
        FailureClass::None
    } else {
        class
    };
    let execution = run.command.is_some()
        || b.exit_code.is_some()
        || b.signal.is_some()
        || b.timed_out.is_some()
        || b.spawn_error.is_some();
    let caller_hint = b
        .failure_signature
        .as_deref()
        .map(caller_signature)
        .transpose()?;
    let contract_id = match (&run.scenario_id, &run.command) {
        (Some(s), _) => s.to_string(),
        (None, Some(c)) => c.join(" "),
        (None, None) => run
            .task_id
            .as_ref()
            .map(|t| t.to_string())
            .unwrap_or_default(),
    };
    let computed = (class.is_failure() && execution).then(|| {
        derive_failure_cause(
            &contract_id,
            class,
            &classification.error_class,
            if report_scan_truncated { "" } else { &output },
            &in_repo,
        )
    });
    // Serialize selection, terminal state and the single debit. A failed debit
    // must leave the run unfinished so a retry cannot lose its accounting.
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(sdi_db::map_sqlite_err)?;
    let bridge = match (&computed, &run.plan_id, &run.scenario_id) {
        (Some(identity), Some(plan), Some(scn)) => budget_repo::compatibility_bucket(
            &tx,
            plan,
            scn,
            class,
            identity.signature.starts_with("sig:v2:fallback:"),
        )?,
        _ => None,
    };
    let signature = if let Some(identity) = &computed {
        Some(
            bridge
                .as_ref()
                .map(|b| b.failure_signature.clone())
                .unwrap_or_else(|| identity.signature.clone()),
        )
    } else if class.is_failure() {
        Some(match caller_hint.as_deref() {
            // Only the daemon may mint a diagnosed identity. Manual hints
            // remain coarse compatibility authority even if they mimic it.
            Some(raw) if raw.starts_with("sig:v2:") => format!("caller:{raw}"),
            Some(raw) => raw.to_owned(),
            None => failure_signature(
                &contract_id,
                class,
                classification.failing_test.as_deref(),
                &classification.error_class,
                classification.top_frame.as_deref(),
            ),
        })
    } else {
        None
    };
    let failure_identity = computed.as_ref().map(|identity| {
        json!({
            "computed": {"version": identity.version, "basis": identity.basis,
                "diagnostic": identity.diagnostic, "signature": identity.signature},
            "scope": "primary-only",
            "diagnostic_unavailable": report_scan_truncated.then_some("report-scan-truncated"),
            "booked_signature": signature,
            "caller_hint": caller_hint,
            "compatibility_bridge": bridge.as_ref().map(|_| {
                if identity.signature.starts_with("sig:v2:fallback:") {
                    "diagnostic-unavailable"
                } else { "outstanding-coarse-budget" }
            }),
        })
    });

    repo::finish_structured(
        &tx,
        &rid,
        &RunFinish {
            result: Some(result),
            notes: b
                .notes
                .clone()
                .or_else(|| Some(classification.reason.clone())),
            exit_code: outcome.exit_code,
            signal: outcome.signal,
            timed_out: outcome.timed_out,
            spawn_error: outcome.spawn_error,
            report_sha256,
            failure_class: Some(class),
            failure_signature: signature,
            failure_identity,
            error_class: class
                .is_failure()
                .then(|| classification.error_class.clone()),
            failing_test: classification.failing_test.clone(),
            duration_ms: b.duration_ms,
        },
    )?;
    let fresh = repo::get_run(&tx, &rid)?;
    let budget = verification::book_failure(&tx, &fresh)?;
    tx.commit().map_err(sdi_db::map_sqlite_err)?;
    state.publish(EventEnvelope {
        kind: "run.finished".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    let mut out = json!(fresh);
    out["classification_reason"] = json!(classification.reason);
    out["retry_budget"] = budget.unwrap_or(Value::Null);
    Ok(Json(out))
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
    #[serde(default)]
    scenario_id: Option<String>,
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
    } else if let Some(s) = q.scenario_id {
        repo::list_runs_by_scenario(&conn, &Id::from(s))?
    } else {
        return Err(
            DomainError::Validation("task_id, session_id or scenario_id required".into()).into(),
        );
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
            // A subtask inherits its parent's lane: decomposing a chore yields
            // chores, decomposing a D3 task yields tasks.
            kind: parent.kind.clone(),
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
