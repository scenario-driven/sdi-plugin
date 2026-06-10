//! CollaborationPattern repository (D22 — 7th first-class entity).
//!
//! Pure CRUD against `collaboration_patterns`. Shape validation (D26) and
//! lifecycle transition rules (D27) live in the domain layer / daemon — this
//! module does not enforce them. JSON payload columns are stored as raw
//! strings; callers serialize/deserialize via `sdi_core::pattern` types when
//! they need the typed view.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, s, s_opt, ts, ts_opt};
use rusqlite::{params, Connection, Row};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};

const COLS: &str = "id, short_code, plan_id, kind, applies_to, scope_id, \
                    parent_pattern_id, depth, lifecycle, \
                    steps_json, reviewers_json, fan_out_json, peer_registration_json, \
                    decided_at, decided_reason, created_at, updated_at";

/// Flat row mirror of `collaboration_patterns`. Repos return this; the daemon
/// (U3) maps to/from `sdi_core::pattern::CollaborationPattern` using its own
/// JSON ser/de — we keep the storage shape opaque to the typed payloads so
/// schema additions stay additive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternRow {
    pub id: String,
    pub short_code: String,
    pub plan_id: String,
    pub kind: String,
    pub applies_to: String,
    pub scope_id: String,
    pub parent_pattern_id: Option<String>,
    pub depth: i64,
    pub lifecycle: String,
    pub steps_json: Option<String>,
    pub reviewers_json: Option<String>,
    pub fan_out_json: Option<String>,
    pub peer_registration_json: Option<String>,
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
    pub decided_reason: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

fn row_to_pattern(row: &Row<'_>) -> rusqlite::Result<PatternRow> {
    Ok(PatternRow {
        id: s(row, 0)?,
        short_code: s(row, 1)?,
        plan_id: s(row, 2)?,
        kind: s(row, 3)?,
        applies_to: s(row, 4)?,
        scope_id: s(row, 5)?,
        parent_pattern_id: s_opt(row, 6)?,
        depth: row.get(7)?,
        lifecycle: s(row, 8)?,
        steps_json: s_opt(row, 9)?,
        reviewers_json: s_opt(row, 10)?,
        fan_out_json: s_opt(row, 11)?,
        peer_registration_json: s_opt(row, 12)?,
        decided_at: ts_opt(row, 13)?,
        decided_reason: s_opt(row, 14)?,
        created_at: ts(row, 15)?,
        updated_at: ts(row, 16)?,
    })
}

pub fn insert(conn: &Connection, row: &PatternRow) -> DomainResult<()> {
    conn.execute(
        &format!(
            "INSERT INTO collaboration_patterns({COLS}) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)"
        ),
        params![
            row.id,
            row.short_code,
            row.plan_id,
            row.kind,
            row.applies_to,
            row.scope_id,
            row.parent_pattern_id,
            row.depth,
            row.lifecycle,
            row.steps_json,
            row.reviewers_json,
            row.fan_out_json,
            row.peer_registration_json,
            row.decided_at.map(fmt_ts),
            row.decided_reason,
            fmt_ts(row.created_at),
            fmt_ts(row.updated_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

/// Move a pattern to a new lifecycle stage. Terminal lifecycles
/// (`converged`, `dissensus`, `aborted`) also stamp `decided_at` with the
/// current time. Callers validate the transition + shape (D26/D27) before
/// invoking; this function is mechanical.
pub fn update_lifecycle(
    conn: &Connection,
    id: &Id,
    new_lifecycle: &str,
    decided_reason: Option<&str>,
) -> DomainResult<()> {
    let terminal = matches!(new_lifecycle, "converged" | "dissensus" | "aborted");
    let now_s = fmt_ts(now());
    let decided_at = if terminal { Some(now_s.clone()) } else { None };
    let n = conn
        .execute(
            "UPDATE collaboration_patterns \
             SET lifecycle = ?1, \
                 decided_at = COALESCE(?2, decided_at), \
                 decided_reason = COALESCE(?3, decided_reason), \
                 updated_at = ?4 \
             WHERE id = ?5",
            params![
                new_lifecycle,
                decided_at,
                decided_reason,
                now_s,
                id.as_str()
            ],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

pub fn get(conn: &Connection, id: &Id) -> DomainResult<Option<PatternRow>> {
    let result = conn.query_row(
        &format!("SELECT {COLS} FROM collaboration_patterns WHERE id = ?1"),
        [id.as_str()],
        row_to_pattern,
    );
    match result {
        Ok(p) => Ok(Some(p)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(map_sqlite_err(e)),
    }
}

pub fn get_by_short_code(
    conn: &Connection,
    plan_id: &Id,
    short_code: &str,
) -> DomainResult<Option<PatternRow>> {
    let result = conn.query_row(
        &format!(
            "SELECT {COLS} FROM collaboration_patterns \
             WHERE plan_id = ?1 AND short_code = ?2"
        ),
        params![plan_id.as_str(), short_code],
        row_to_pattern,
    );
    match result {
        Ok(p) => Ok(Some(p)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(map_sqlite_err(e)),
    }
}

pub fn list_by_plan(conn: &Connection, plan_id: &Id) -> DomainResult<Vec<PatternRow>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM collaboration_patterns \
             WHERE plan_id = ?1 ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([plan_id.as_str()], row_to_pattern)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// All patterns currently in `lifecycle = 'active'`, optionally scoped to a
/// project (through plans). Used by the daemon's PreToolUse hook surface
/// (`/patterns/active`) for D26 integrity gates, and — project-scoped — by
/// the dashboard topbar badge, which must agree with the project's Patterns
/// view rather than count rows from every project in the database.
pub fn list_active(conn: &Connection, project_id: Option<&str>) -> DomainResult<Vec<PatternRow>> {
    let mut out = Vec::new();
    match project_id {
        Some(pid) => {
            let cols_qualified = COLS
                .split(", ")
                .map(|c| format!("cp.{c}"))
                .collect::<Vec<_>>()
                .join(", ");
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {cols_qualified} FROM collaboration_patterns cp \
                     JOIN plans p ON p.id = cp.plan_id \
                     WHERE cp.lifecycle = 'active' AND p.project_id = ?1 \
                     ORDER BY cp.created_at"
                ))
                .map_err(map_sqlite_err)?;
            let rows = stmt
                .query_map([pid], row_to_pattern)
                .map_err(map_sqlite_err)?;
            for r in rows {
                out.push(r.map_err(map_sqlite_err)?);
            }
        }
        None => {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {COLS} FROM collaboration_patterns \
                     WHERE lifecycle = 'active' ORDER BY created_at"
                ))
                .map_err(map_sqlite_err)?;
            let rows = stmt.query_map([], row_to_pattern).map_err(map_sqlite_err)?;
            for r in rows {
                out.push(r.map_err(map_sqlite_err)?);
            }
        }
    }
    Ok(out)
}

/// Direct children of `parent_id` in the pattern DAG (D24). Used by the
/// daemon to compute depth caps + cycle detection on insert.
pub fn list_children(conn: &Connection, parent_id: &Id) -> DomainResult<Vec<PatternRow>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM collaboration_patterns \
             WHERE parent_pattern_id = ?1 ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([parent_id.as_str()], row_to_pattern)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{plan as plan_repo, project as proj_repo};
    use crate::{ensure_schema, open_pool};
    use sdi_core::ids::{now, IdKind};
    use sdi_core::plan::{Plan, PlanStatus};
    use sdi_core::project::Project;

    fn fixture() -> (r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, Id) {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-pat-{}-{}.db",
            std::process::id(),
            ulid::Ulid::new()
        ));
        let _ = std::fs::remove_file(&tmp);
        let pool = open_pool(&tmp).unwrap();
        ensure_schema(&pool.get().unwrap()).unwrap();
        let conn = pool.get().unwrap();
        let proj = Project {
            id: Id::new(IdKind::Project),
            key: "TST".into(),
            name: "n".into(),
            slug: format!("s-{}", ulid::Ulid::new()),
            cwds: vec![],
            description: None,
            enabled: true,
            wiki_paths: vec!["docs".into()],
            created_at: now(),
            updated_at: now(),
        };
        proj_repo::insert(&conn, &proj).unwrap();
        let plan = Plan {
            id: Id::new(IdKind::Plan),
            project_id: proj.id,
            short_code: format!("TST-{}", ulid::Ulid::new()),
            title: "v".into(),
            body: "".into(),
            status: PlanStatus::Draft,
            version: 0,
            produced_via_pattern_id: None,
            approved_at: None,
            completed_at: None,
            created_at: now(),
            updated_at: now(),
        };
        plan_repo::insert(&conn, &plan).unwrap();
        (pool, plan.id)
    }

    fn mk(plan_id: &Id, short: &str, kind: &str) -> PatternRow {
        PatternRow {
            id: format!("CP-{}", ulid::Ulid::new()),
            short_code: short.into(),
            plan_id: plan_id.as_str().into(),
            kind: kind.into(),
            applies_to: "plan".into(),
            scope_id: plan_id.as_str().into(),
            parent_pattern_id: None,
            depth: 0,
            lifecycle: "pending".into(),
            steps_json: None,
            reviewers_json: None,
            fan_out_json: None,
            peer_registration_json: None,
            decided_at: None,
            decided_reason: None,
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn insert_and_get_round_trip() {
        let (pool, plan_id) = fixture();
        let conn = pool.get().unwrap();
        let row = mk(&plan_id, "CP-1", "workflow");
        insert(&conn, &row).unwrap();
        let fetched = get(&conn, &Id::from(row.id.clone())).unwrap().unwrap();
        assert_eq!(fetched.short_code, "CP-1");
        assert_eq!(fetched.kind, "workflow");
        assert_eq!(fetched.lifecycle, "pending");
    }

    #[test]
    fn unique_short_code_per_table_enforced() {
        let (pool, plan_id) = fixture();
        let conn = pool.get().unwrap();
        insert(&conn, &mk(&plan_id, "CP-dup", "workflow")).unwrap();
        let err = insert(&conn, &mk(&plan_id, "CP-dup", "graph")).unwrap_err();
        assert!(matches!(err, DomainError::Conflict(_)));
    }

    #[test]
    fn update_lifecycle_stamps_decided_at_on_terminal() {
        let (pool, plan_id) = fixture();
        let conn = pool.get().unwrap();
        let row = mk(&plan_id, "CP-2", "graph");
        insert(&conn, &row).unwrap();
        let id = Id::from(row.id.clone());
        update_lifecycle(&conn, &id, "active", None).unwrap();
        let after_active = get(&conn, &id).unwrap().unwrap();
        assert_eq!(after_active.lifecycle, "active");
        assert!(after_active.decided_at.is_none());
        update_lifecycle(&conn, &id, "converged", Some("done")).unwrap();
        let after_terminal = get(&conn, &id).unwrap().unwrap();
        assert_eq!(after_terminal.lifecycle, "converged");
        assert!(after_terminal.decided_at.is_some());
        assert_eq!(after_terminal.decided_reason.as_deref(), Some("done"));
    }

    #[test]
    fn list_active_filters_by_lifecycle() {
        let (pool, plan_id) = fixture();
        let conn = pool.get().unwrap();
        let a = mk(&plan_id, "CP-a", "workflow");
        let b = mk(&plan_id, "CP-b", "swarm");
        insert(&conn, &a).unwrap();
        insert(&conn, &b).unwrap();
        update_lifecycle(&conn, &Id::from(a.id.clone()), "active", None).unwrap();
        let active = list_active(&conn, None).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, a.id);
    }

    // The dashboard badge passes project_id; patterns from other projects'
    // plans must not leak into the count (the "badge says 12, list shows 0"
    // dogfooding defect).
    #[test]
    fn list_active_scopes_to_project() {
        let (pool, plan_a) = fixture();
        let conn = pool.get().unwrap();
        let pa = mk(&plan_a, "CP-a", "workflow");
        insert(&conn, &pa).unwrap();
        update_lifecycle(&conn, &Id::from(pa.id.clone()), "active", None).unwrap();

        // Second project with its own plan + active pattern.
        let proj_b = Project {
            id: Id::new(IdKind::Project),
            key: "TSB".into(),
            name: "n".into(),
            slug: format!("s-{}", ulid::Ulid::new()),
            cwds: vec![],
            description: None,
            enabled: true,
            wiki_paths: vec!["docs".into()],
            created_at: now(),
            updated_at: now(),
        };
        proj_repo::insert(&conn, &proj_b).unwrap();
        let plan_b = Plan {
            id: Id::new(IdKind::Plan),
            project_id: proj_b.id.clone(),
            short_code: format!("TSB-{}", ulid::Ulid::new()),
            title: "v".into(),
            body: "".into(),
            status: PlanStatus::Draft,
            version: 0,
            produced_via_pattern_id: None,
            approved_at: None,
            completed_at: None,
            created_at: now(),
            updated_at: now(),
        };
        plan_repo::insert(&conn, &plan_b).unwrap();
        let pb = mk(&plan_b.id, "CP-b", "swarm");
        insert(&conn, &pb).unwrap();
        update_lifecycle(&conn, &Id::from(pb.id.clone()), "active", None).unwrap();

        let unscoped = list_active(&conn, None).unwrap();
        assert_eq!(unscoped.len(), 2);
        let scoped = list_active(&conn, Some(proj_b.id.as_str())).unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].id, pb.id);
    }

    #[test]
    fn list_children_returns_descendants_of_parent() {
        let (pool, plan_id) = fixture();
        let conn = pool.get().unwrap();
        let parent = mk(&plan_id, "CP-parent", "workflow");
        insert(&conn, &parent).unwrap();
        let mut child = mk(&plan_id, "CP-child", "swarm");
        child.parent_pattern_id = Some(parent.id.clone());
        child.depth = 1;
        insert(&conn, &child).unwrap();
        let kids = list_children(&conn, &Id::from(parent.id.clone())).unwrap();
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0].id, child.id);
    }

    #[test]
    fn get_by_short_code_scoped_to_plan() {
        let (pool, plan_id) = fixture();
        let conn = pool.get().unwrap();
        let row = mk(&plan_id, "CP-find", "graph");
        insert(&conn, &row).unwrap();
        let hit = get_by_short_code(&conn, &plan_id, "CP-find").unwrap();
        assert!(hit.is_some());
        let miss = get_by_short_code(&conn, &plan_id, "CP-missing").unwrap();
        assert!(miss.is_none());
    }
}
