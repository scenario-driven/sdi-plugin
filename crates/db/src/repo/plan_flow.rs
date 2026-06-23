//! plan↔flow targeting link (PRD-v2 D34). A plan declares which UserFlows it
//! must satisfy; the approve gate enforces L2 step coverage over them.

use crate::map_sqlite_err;
use crate::repo::fmt_ts;
use rusqlite::{params, Connection};
use sdi_core::error::DomainResult;
use sdi_core::ids::{now, Id};

/// Idempotent — re-linking the same (plan, flow) is a no-op.
pub fn link(conn: &Connection, plan_id: &Id, flow_id: &Id) -> DomainResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO plan_flows(plan_id, flow_id, created_at) VALUES (?1, ?2, ?3)",
        params![plan_id.as_str(), flow_id.as_str(), fmt_ts(now())],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn unlink(conn: &Connection, plan_id: &Id, flow_id: &Id) -> DomainResult<()> {
    conn.execute(
        "DELETE FROM plan_flows WHERE plan_id = ?1 AND flow_id = ?2",
        params![plan_id.as_str(), flow_id.as_str()],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn list_flow_ids_for_plan(conn: &Connection, plan_id: &Id) -> DomainResult<Vec<Id>> {
    let mut stmt = conn
        .prepare("SELECT flow_id FROM plan_flows WHERE plan_id = ?1 ORDER BY created_at")
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([plan_id.as_str()], |r| r.get::<_, String>(0))
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(Id::from(r.map_err(map_sqlite_err)?));
    }
    Ok(out)
}
