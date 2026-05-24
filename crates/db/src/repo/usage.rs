//! Repository for `usage_records` (token/tool accounting).
//!
//! Aggregates are computed on read; the table is append-only.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, parsed, s, s_opt, ts};
use rusqlite::{params, Connection, Row};
use sdi_core::error::DomainResult;
use sdi_core::ids::Id;
use sdi_core::usage::{UsageRecord, UsageRollup, UsageTier};

const USAGE_COLS: &str = "id, project_id, plan_id, task_id, run_id, model, tier, \
     input_tokens, output_tokens, cache_read, cache_write, tool_calls, cost_usd, created_at";

fn row_to_usage(row: &Row<'_>) -> rusqlite::Result<UsageRecord> {
    Ok(UsageRecord {
        id: Id::from(s(row, 0)?),
        project_id: Id::from(s(row, 1)?),
        plan_id: s_opt(row, 2)?.map(Id::from),
        task_id: s_opt(row, 3)?.map(Id::from),
        run_id: s_opt(row, 4)?.map(Id::from),
        model: s(row, 5)?,
        tier: parsed::<UsageTier>(row, 6)?,
        input_tokens: row.get(7)?,
        output_tokens: row.get(8)?,
        cache_read: row.get(9)?,
        cache_write: row.get(10)?,
        tool_calls: row.get(11)?,
        cost_usd: row.get(12)?,
        created_at: ts(row, 13)?,
    })
}

pub fn insert(conn: &Connection, rec: &UsageRecord) -> DomainResult<()> {
    conn.execute(
        &format!(
            "INSERT INTO usage_records({USAGE_COLS}) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)"
        ),
        params![
            rec.id.as_str(),
            rec.project_id.as_str(),
            rec.plan_id.as_ref().map(|i| i.as_str()),
            rec.task_id.as_ref().map(|i| i.as_str()),
            rec.run_id.as_ref().map(|i| i.as_str()),
            rec.model,
            rec.tier.to_string(),
            rec.input_tokens,
            rec.output_tokens,
            rec.cache_read,
            rec.cache_write,
            rec.tool_calls,
            rec.cost_usd,
            fmt_ts(rec.created_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn list_by_project(conn: &Connection, project_id: &Id) -> DomainResult<Vec<UsageRecord>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {USAGE_COLS} FROM usage_records WHERE project_id = ?1 ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([project_id.as_str()], row_to_usage)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn list_by_plan(conn: &Connection, plan_id: &Id) -> DomainResult<Vec<UsageRecord>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {USAGE_COLS} FROM usage_records WHERE plan_id = ?1 ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([plan_id.as_str()], row_to_usage)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn list_by_task(conn: &Connection, task_id: &Id) -> DomainResult<Vec<UsageRecord>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {USAGE_COLS} FROM usage_records WHERE task_id = ?1 ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([task_id.as_str()], row_to_usage)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// Roll-up for one plan. Uses SQL aggregates so big plans don't materialise.
pub fn rollup_for_plan(conn: &Connection, plan_id: &Id) -> DomainResult<UsageRollup> {
    rollup_where(conn, "plan_id = ?1", plan_id.as_str())
}

pub fn rollup_for_task(conn: &Connection, task_id: &Id) -> DomainResult<UsageRollup> {
    rollup_where(conn, "task_id = ?1", task_id.as_str())
}

pub fn rollup_for_project(conn: &Connection, project_id: &Id) -> DomainResult<UsageRollup> {
    rollup_where(conn, "project_id = ?1", project_id.as_str())
}

fn rollup_where(conn: &Connection, where_sql: &str, arg: &str) -> DomainResult<UsageRollup> {
    let sql = format!(
        "SELECT \
            COUNT(*) AS records, \
            COALESCE(SUM(input_tokens),0), \
            COALESCE(SUM(output_tokens),0), \
            COALESCE(SUM(cache_read),0), \
            COALESCE(SUM(cache_write),0), \
            COALESCE(SUM(tool_calls),0), \
            COALESCE(SUM(cost_usd),0.0) \
         FROM usage_records WHERE {where_sql}"
    );
    let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
    let r = stmt
        .query_row([arg], |row| {
            Ok(UsageRollup {
                records: row.get(0)?,
                input_tokens: row.get(1)?,
                output_tokens: row.get(2)?,
                cache_read: row.get(3)?,
                cache_write: row.get(4)?,
                tool_calls: row.get(5)?,
                cost_usd: row.get(6)?,
            })
        })
        .map_err(map_sqlite_err)?;
    Ok(r)
}

/// Preflight estimate for a task: returns the historical average cost-per-run
/// over similar tasks (same tier within the project). If no history, returns
/// `None` so callers can fall back to a model-specific default.
pub fn preflight_avg_for_tier(
    conn: &Connection,
    project_id: &Id,
    tier: UsageTier,
) -> DomainResult<Option<f64>> {
    let mut stmt = conn
        .prepare(
            "SELECT AVG(cost_usd) FROM usage_records \
             WHERE project_id = ?1 AND tier = ?2",
        )
        .map_err(map_sqlite_err)?;
    let v: Option<f64> = stmt
        .query_row(params![project_id.as_str(), tier.to_string()], |row| {
            row.get(0)
        })
        .map_err(map_sqlite_err)?;
    Ok(v)
}
