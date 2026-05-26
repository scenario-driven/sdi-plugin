//! Repository for runtime artifacts: runs, task hierarchy, single-writer
//! leases. All three live behind one module since they share the task FK.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, parsed, s, s_opt, ts, ts_opt};
use rusqlite::{params, Connection, Row};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};
use sdi_core::run::{RelationKind, Run, RunResult, TaskLease, TaskRelation};
use std::collections::{HashSet, VecDeque};

// ---------------------------------------------------------------------------
// runs
// ---------------------------------------------------------------------------

const RUN_COLS: &str =
    "id, task_id, actor, session_id, started_at, finished_at, result, notes, payload";

fn row_to_run(row: &Row<'_>) -> rusqlite::Result<Run> {
    let payload_raw: String = row.get(8)?;
    Ok(Run {
        id: Id::from(s(row, 0)?),
        task_id: Id::from(s(row, 1)?),
        actor: s(row, 2)?,
        session_id: s_opt(row, 3)?,
        started_at: ts(row, 4)?,
        finished_at: ts_opt(row, 5)?,
        result: s_opt(row, 6)?
            .map(|raw| raw.parse::<RunResult>())
            .transpose()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    6,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?,
        notes: s_opt(row, 7)?,
        payload: serde_json::from_str(&payload_raw).unwrap_or(serde_json::Value::Null),
    })
}

pub fn insert_run(conn: &Connection, run: &Run) -> DomainResult<()> {
    conn.execute(
        &format!("INSERT INTO runs({RUN_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"),
        params![
            run.id.as_str(),
            run.task_id.as_str(),
            run.actor,
            run.session_id,
            fmt_ts(run.started_at),
            run.finished_at.map(fmt_ts),
            run.result.map(|r| r.to_string()),
            run.notes,
            serde_json::to_string(&run.payload).unwrap_or_else(|_| "{}".into()),
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

pub fn list_runs_by_task(conn: &Connection, task_id: &Id) -> DomainResult<Vec<Run>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {RUN_COLS} FROM runs WHERE task_id = ?1 ORDER BY started_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([task_id.as_str()], row_to_run)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn list_runs_by_session(conn: &Connection, session_id: &str) -> DomainResult<Vec<Run>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {RUN_COLS} FROM runs WHERE session_id = ?1 ORDER BY started_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([session_id], row_to_run)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
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
