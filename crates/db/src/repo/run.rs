//! Repository for runtime artifacts: runs, task hierarchy, single-writer
//! leases. All three live behind one module since they share the task FK.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, parsed, s, s_opt, ts, ts_opt};
use crate::workspace;
use rusqlite::{params, Connection, Row};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};
use sdi_core::run::{RelationKind, Run, RunResult, TaskLease, TaskRelation};
use sdi_core::verification::{contract_hash, FailureClass, SpawnError};
use std::collections::{HashSet, VecDeque};

// ---------------------------------------------------------------------------
// runs
// ---------------------------------------------------------------------------

const RUN_COLS: &str =
    "id, task_id, actor, session_id, started_at, finished_at, result, notes, payload, \
     plan_id, round_id, scenario_id, command_json, cwd, input_globs_json, tool_versions_json, \
     input_fingerprint, exit_code, signal, timed_out, spawn_error, report_path, report_sha256, \
     failure_class, failure_signature, error_class, failing_test, duration_ms";

fn conv_err(idx: usize, e: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
}

fn parse_opt<T>(row: &Row<'_>, idx: usize) -> rusqlite::Result<Option<T>>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    s_opt(row, idx)?
        .map(|raw| raw.parse::<T>())
        .transpose()
        .map_err(|e| conv_err(idx, e))
}

fn row_to_run(row: &Row<'_>) -> rusqlite::Result<Run> {
    let payload_raw: String = row.get(8)?;
    let command: Option<Vec<String>> = s_opt(row, 12)?
        .map(|raw| serde_json::from_str(&raw))
        .transpose()
        .map_err(|e| conv_err(12, e))?;
    let globs_raw: String = row.get(14)?;
    let tools_raw: String = row.get(15)?;
    Ok(Run {
        id: Id::from(s(row, 0)?),
        task_id: s_opt(row, 1)?.map(Id::from),
        actor: s(row, 2)?,
        session_id: s_opt(row, 3)?,
        started_at: ts(row, 4)?,
        finished_at: ts_opt(row, 5)?,
        result: parse_opt::<RunResult>(row, 6)?,
        notes: s_opt(row, 7)?,
        payload: serde_json::from_str(&payload_raw).unwrap_or(serde_json::Value::Null),
        plan_id: s_opt(row, 9)?.map(Id::from),
        round_id: s_opt(row, 10)?.map(Id::from),
        scenario_id: s_opt(row, 11)?.map(Id::from),
        command,
        cwd: s_opt(row, 13)?,
        input_globs: serde_json::from_str(&globs_raw).map_err(|e| conv_err(14, e))?,
        tool_versions: serde_json::from_str(&tools_raw).map_err(|e| conv_err(15, e))?,
        input_fingerprint: s_opt(row, 16)?,
        exit_code: row.get::<_, Option<i64>>(17)?.map(|v| v as i32),
        signal: row.get::<_, Option<i64>>(18)?.map(|v| v as i32),
        timed_out: row.get::<_, i64>(19)? != 0,
        spawn_error: parse_opt::<SpawnError>(row, 20)?,
        report_path: s_opt(row, 21)?,
        report_sha256: s_opt(row, 22)?,
        failure_class: parse_opt::<FailureClass>(row, 23)?,
        failure_signature: s_opt(row, 24)?,
        error_class: s_opt(row, 25)?,
        failing_test: s_opt(row, 26)?,
        duration_ms: row.get::<_, Option<i64>>(27)?,
    })
}

pub fn insert_run(conn: &Connection, run: &Run) -> DomainResult<()> {
    let command = run
        .command
        .as_ref()
        .map(|c| serde_json::to_string(c).unwrap_or_else(|_| "[]".into()));
    conn.execute(
        &format!(
            "INSERT INTO runs({RUN_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,\
             ?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28)"
        ),
        params![
            run.id.as_str(),
            run.task_id.as_ref().map(|i| i.as_str().to_string()),
            run.actor,
            run.session_id,
            fmt_ts(run.started_at),
            run.finished_at.map(fmt_ts),
            run.result.map(|r| r.to_string()),
            run.notes,
            serde_json::to_string(&run.payload).unwrap_or_else(|_| "{}".into()),
            run.plan_id.as_ref().map(|i| i.as_str().to_string()),
            run.round_id.as_ref().map(|i| i.as_str().to_string()),
            run.scenario_id.as_ref().map(|i| i.as_str().to_string()),
            command,
            run.cwd,
            serde_json::to_string(&run.input_globs).unwrap_or_else(|_| "[]".into()),
            serde_json::to_string(&run.tool_versions).unwrap_or_else(|_| "{}".into()),
            run.input_fingerprint,
            run.exit_code.map(i64::from),
            run.signal.map(i64::from),
            run.timed_out as i64,
            run.spawn_error.map(|e| e.to_string()),
            run.report_path,
            run.report_sha256,
            run.failure_class.map(|c| c.to_string()),
            run.failure_signature,
            run.error_class,
            run.failing_test,
            run.duration_ms,
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get_run(conn: &Connection, id: &Id) -> DomainResult<Run> {
    conn.query_row(
        &format!("SELECT {RUN_COLS} FROM runs WHERE id = ?1"),
        [id.as_str()],
        row_to_run,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

fn query_runs(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
) -> DomainResult<Vec<Run>> {
    let mut stmt = conn.prepare(sql).map_err(map_sqlite_err)?;
    let rows = stmt.query_map(params, row_to_run).map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn list_runs_by_task(conn: &Connection, task_id: &Id) -> DomainResult<Vec<Run>> {
    query_runs(
        conn,
        &format!("SELECT {RUN_COLS} FROM runs WHERE task_id = ?1 ORDER BY started_at"),
        &[&task_id.as_str()],
    )
}

pub fn list_runs_by_session(conn: &Connection, session_id: &str) -> DomainResult<Vec<Run>> {
    query_runs(
        conn,
        &format!("SELECT {RUN_COLS} FROM runs WHERE session_id = ?1 ORDER BY started_at"),
        &[&session_id],
    )
}

pub fn list_runs_by_scenario(conn: &Connection, scenario_id: &Id) -> DomainResult<Vec<Run>> {
    query_runs(
        conn,
        &format!("SELECT {RUN_COLS} FROM runs WHERE scenario_id = ?1 ORDER BY started_at"),
        &[&scenario_id.as_str()],
    )
}

/// Finished clean passes for a scenario, newest first.
pub fn passing_runs_for_scenario(conn: &Connection, scenario_id: &Id) -> DomainResult<Vec<Run>> {
    query_runs(
        conn,
        &format!(
            "SELECT {RUN_COLS} FROM runs WHERE scenario_id = ?1 AND finished_at IS NOT NULL \
             AND result = 'success' AND exit_code = 0 AND input_fingerprint IS NOT NULL \
             ORDER BY finished_at DESC"
        ),
        &[&scenario_id.as_str()],
    )
}

/// The most recent finished run per scenario within a round.
pub fn latest_finished_per_scenario_in_round(
    conn: &Connection,
    round_id: &Id,
) -> DomainResult<Vec<Run>> {
    query_runs(
        conn,
        &format!(
            "SELECT {RUN_COLS} FROM runs r WHERE r.round_id = ?1 AND r.scenario_id IS NOT NULL \
               AND r.finished_at IS NOT NULL \
               AND r.finished_at = (SELECT MAX(r2.finished_at) FROM runs r2 \
                                    WHERE r2.round_id = r.round_id AND r2.scenario_id = r.scenario_id \
                                      AND r2.finished_at IS NOT NULL) \
             ORDER BY r.scenario_id"
        ),
        &[&round_id.as_str()],
    )
}

pub fn finish_run(
    conn: &Connection,
    id: &Id,
    result: RunResult,
    notes: Option<&str>,
) -> DomainResult<()> {
    let n = conn
        .execute(
            "UPDATE runs SET finished_at = ?1, result = ?2, notes = COALESCE(?3, notes) \
             WHERE id = ?4 AND finished_at IS NULL",
            params![fmt_ts(now()), result.to_string(), notes, id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::Validation(format!(
            "run {id} not found or already finished"
        )));
    }
    Ok(())
}

/// Terminal state of a structured (verification) run.
#[derive(Debug, Clone, Default)]
pub struct RunFinish {
    pub result: Option<RunResult>,
    pub notes: Option<String>,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub spawn_error: Option<SpawnError>,
    pub report_sha256: Option<String>,
    pub failure_class: Option<FailureClass>,
    pub failure_signature: Option<String>,
    pub error_class: Option<String>,
    pub failing_test: Option<String>,
    pub duration_ms: Option<i64>,
}

pub fn finish_structured(conn: &Connection, id: &Id, f: &RunFinish) -> DomainResult<()> {
    let result = f
        .result
        .ok_or_else(|| DomainError::Validation("run finish requires a result".into()))?;
    let n = conn
        .execute(
            "UPDATE runs SET finished_at = ?1, result = ?2, notes = COALESCE(?3, notes), \
                 exit_code = ?4, signal = ?5, timed_out = ?6, spawn_error = ?7, \
                 report_sha256 = ?8, failure_class = ?9, failure_signature = ?10, \
                 error_class = ?11, failing_test = ?12, duration_ms = ?13 \
             WHERE id = ?14 AND finished_at IS NULL",
            params![
                fmt_ts(now()),
                result.to_string(),
                f.notes,
                f.exit_code.map(i64::from),
                f.signal.map(i64::from),
                f.timed_out as i64,
                f.spawn_error.map(|e| e.to_string()),
                f.report_sha256,
                f.failure_class.map(|c| c.to_string()),
                f.failure_signature,
                f.error_class,
                f.failing_test,
                f.duration_ms,
                id.as_str(),
            ],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::Validation(format!(
            "run {id} not found or already finished"
        )));
    }
    Ok(())
}

/// The scenario contract digest a run's fingerprint is computed against.
/// A run without a scenario binds to the empty contract.
pub fn contract_for_scenario(conn: &Connection, scenario_id: Option<&Id>) -> DomainResult<String> {
    match scenario_id {
        Some(sid) => {
            let scn = crate::repo::scenario::get(conn, sid)?;
            Ok(contract_hash(
                &scn.given,
                &scn.when_clause,
                &scn.then_clause,
            ))
        }
        None => Ok(String::new()),
    }
}

/// Recompute a run's input fingerprint against the workspace as it is now.
/// Errors (cwd gone, no longer a git work tree, scenario deleted) mean the
/// recorded fingerprint cannot be confirmed — callers treat that as stale.
pub fn current_fingerprint(conn: &Connection, run: &Run) -> DomainResult<String> {
    let cwd = run
        .cwd
        .as_deref()
        .ok_or_else(|| DomainError::Validation(format!("run {} recorded no cwd", run.id)))?;
    let command = run.command.clone().unwrap_or_default();
    let contract = contract_for_scenario(conn, run.scenario_id.as_ref())?;
    workspace::fingerprint(
        std::path::Path::new(cwd),
        &run.input_globs,
        &command,
        &run.tool_versions,
        &contract,
    )
}

/// Freshness of a recorded fingerprint against the current workspace.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Freshness {
    pub recorded: Option<String>,
    pub current: Option<String>,
    pub fresh: bool,
    /// Why freshness could not be confirmed (recompute error / no fingerprint).
    pub reason: Option<String>,
}

pub fn freshness(conn: &Connection, run: &Run) -> Freshness {
    let Some(recorded) = run.input_fingerprint.clone() else {
        return Freshness {
            recorded: None,
            current: None,
            fresh: false,
            reason: Some("run recorded no input fingerprint".into()),
        };
    };
    match current_fingerprint(conn, run) {
        Ok(current) => Freshness {
            fresh: current == recorded,
            reason: (current != recorded)
                .then(|| "inputs changed since the run (fingerprint mismatch)".to_string()),
            recorded: Some(recorded),
            current: Some(current),
        },
        Err(e) => Freshness {
            recorded: Some(recorded),
            current: None,
            fresh: false,
            reason: Some(e.to_string()),
        },
    }
}

// ---------------------------------------------------------------------------
// task_relations
// ---------------------------------------------------------------------------

const REL_COLS: &str = "id, parent_id, child_id, kind, created_at";

fn row_to_relation(row: &Row<'_>) -> rusqlite::Result<TaskRelation> {
    Ok(TaskRelation {
        id: Id::from(s(row, 0)?),
        parent_id: Id::from(s(row, 1)?),
        child_id: Id::from(s(row, 2)?),
        kind: parsed::<RelationKind>(row, 3)?,
        created_at: ts(row, 4)?,
    })
}

pub fn insert_relation(conn: &Connection, rel: &TaskRelation) -> DomainResult<()> {
    if rel.parent_id == rel.child_id {
        return Err(DomainError::Validation(
            "task relation cannot point at itself".into(),
        ));
    }
    conn.execute(
        &format!("INSERT INTO task_relations({REL_COLS}) VALUES (?1,?2,?3,?4,?5)"),
        params![
            rel.id.as_str(),
            rel.parent_id.as_str(),
            rel.child_id.as_str(),
            rel.kind.to_string(),
            fmt_ts(rel.created_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn delete_relation(conn: &Connection, id: &Id) -> DomainResult<()> {
    let n = conn
        .execute("DELETE FROM task_relations WHERE id = ?1", [id.as_str()])
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

pub fn list_relations_for_task(conn: &Connection, task_id: &Id) -> DomainResult<Vec<TaskRelation>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {REL_COLS} FROM task_relations \
             WHERE parent_id = ?1 OR child_id = ?1 ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([task_id.as_str()], row_to_relation)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// BFS upward through `parent` relations. Returns ids in order from immediate
/// parent → root. Cycle-safe via visited set.
pub fn ancestors(conn: &Connection, task_id: &Id) -> DomainResult<Vec<Id>> {
    let mut stmt = conn
        .prepare(
            "SELECT parent_id FROM task_relations \
             WHERE child_id = ?1 AND kind = 'parent'",
        )
        .map_err(map_sqlite_err)?;
    let mut frontier = VecDeque::from([task_id.clone()]);
    let mut seen: HashSet<String> = HashSet::from([task_id.to_string()]);
    let mut out = Vec::new();
    while let Some(cur) = frontier.pop_front() {
        let rows = stmt
            .query_map([cur.as_str()], |r| r.get::<_, String>(0))
            .map_err(map_sqlite_err)?;
        for r in rows {
            let p = r.map_err(map_sqlite_err)?;
            if seen.insert(p.clone()) {
                out.push(Id::from(p.clone()));
                frontier.push_back(Id::from(p));
            }
        }
    }
    Ok(out)
}

/// BFS downward through `parent` relations. Returns ids in BFS order.
pub fn descendants(conn: &Connection, task_id: &Id) -> DomainResult<Vec<Id>> {
    let mut stmt = conn
        .prepare(
            "SELECT child_id FROM task_relations \
             WHERE parent_id = ?1 AND kind = 'parent'",
        )
        .map_err(map_sqlite_err)?;
    let mut frontier = VecDeque::from([task_id.clone()]);
    let mut seen: HashSet<String> = HashSet::from([task_id.to_string()]);
    let mut out = Vec::new();
    while let Some(cur) = frontier.pop_front() {
        let rows = stmt
            .query_map([cur.as_str()], |r| r.get::<_, String>(0))
            .map_err(map_sqlite_err)?;
        for r in rows {
            let c = r.map_err(map_sqlite_err)?;
            if seen.insert(c.clone()) {
                out.push(Id::from(c.clone()));
                frontier.push_back(Id::from(c));
            }
        }
    }
    Ok(out)
}

/// Task status histogram across the entire DB. Correct for `/metrics`, where a
/// server-wide gauge spanning every project is the intended Prometheus semantic.
pub fn status_counts(conn: &Connection) -> DomainResult<Vec<(String, i64)>> {
    let mut stmt = conn
        .prepare("SELECT status, COUNT(*) FROM tasks GROUP BY status")
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// Task status histogram scoped to one project, joining tasks → rounds → plans.
/// Correct for `/dashboard` and `sdi task stats`, where a global count would
/// leak other projects' tasks in a shared multi-project database.
pub fn status_counts_for_project(
    conn: &Connection,
    project_id: &Id,
) -> DomainResult<Vec<(String, i64)>> {
    let mut stmt = conn
        .prepare(
            "SELECT t.status, COUNT(*) \
             FROM tasks t \
             JOIN rounds r ON t.round_id = r.id \
             JOIN plans p ON r.plan_id = p.id \
             WHERE p.project_id = ?1 \
             GROUP BY t.status",
        )
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([project_id.as_str()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// task_leases
// ---------------------------------------------------------------------------

const LEASE_COLS: &str = "task_id, holder, acquired_at, heartbeat_at, expires_at";

fn row_to_lease(row: &Row<'_>) -> rusqlite::Result<TaskLease> {
    Ok(TaskLease {
        task_id: Id::from(s(row, 0)?),
        holder: s(row, 1)?,
        acquired_at: ts(row, 2)?,
        heartbeat_at: ts(row, 3)?,
        expires_at: ts(row, 4)?,
    })
}

/// Acquire a lease atomically: succeeds if no row exists OR the existing row
/// has expired. The returned bool is `true` on acquisition, `false` if a
/// different holder still owns the lease.
pub fn acquire_lease(
    conn: &Connection,
    task_id: &Id,
    holder: &str,
    ttl_seconds: i64,
) -> DomainResult<bool> {
    let now_t = now();
    let now_s = fmt_ts(now_t);
    let exp_s = fmt_ts(now_t + chrono::Duration::seconds(ttl_seconds));
    // Try INSERT first.
    let inserted = conn
        .execute(
            "INSERT OR IGNORE INTO task_leases(task_id, holder, acquired_at, heartbeat_at, expires_at) \
             VALUES (?1,?2,?3,?3,?4)",
            params![task_id.as_str(), holder, now_s, exp_s],
        )
        .map_err(map_sqlite_err)?;
    if inserted == 1 {
        return Ok(true);
    }
    // Existing row — overwrite only if expired.
    let updated = conn
        .execute(
            "UPDATE task_leases SET holder = ?1, acquired_at = ?2, heartbeat_at = ?2, expires_at = ?3 \
             WHERE task_id = ?4 AND expires_at < ?2",
            params![holder, now_s, exp_s, task_id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    Ok(updated == 1)
}

/// Refresh the lease's heartbeat. Only succeeds if the caller still owns it.
pub fn heartbeat_lease(
    conn: &Connection,
    task_id: &Id,
    holder: &str,
    ttl_seconds: i64,
) -> DomainResult<bool> {
    let now_t = now();
    let now_s = fmt_ts(now_t);
    let exp_s = fmt_ts(now_t + chrono::Duration::seconds(ttl_seconds));
    let n = conn
        .execute(
            "UPDATE task_leases SET heartbeat_at = ?1, expires_at = ?2 \
             WHERE task_id = ?3 AND holder = ?4",
            params![now_s, exp_s, task_id.as_str(), holder],
        )
        .map_err(map_sqlite_err)?;
    Ok(n == 1)
}

pub fn release_lease(conn: &Connection, task_id: &Id, holder: &str) -> DomainResult<bool> {
    let n = conn
        .execute(
            "DELETE FROM task_leases WHERE task_id = ?1 AND holder = ?2",
            params![task_id.as_str(), holder],
        )
        .map_err(map_sqlite_err)?;
    Ok(n == 1)
}

pub fn get_lease(conn: &Connection, task_id: &Id) -> DomainResult<Option<TaskLease>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {LEASE_COLS} FROM task_leases WHERE task_id = ?1"
        ))
        .map_err(map_sqlite_err)?;
    let mut rows = stmt
        .query_map([task_id.as_str()], row_to_lease)
        .map_err(map_sqlite_err)?;
    match rows.next() {
        Some(r) => Ok(Some(r.map_err(map_sqlite_err)?)),
        None => Ok(None),
    }
}
