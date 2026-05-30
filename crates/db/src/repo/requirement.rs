//! Requirement repository — SNAPSHOT-ONLY storage (D12). `update_body`
//! overwrites in place; history is captured via the linked Decision log.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, s, s_opt, ts};
use rusqlite::{params, Connection, Row};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};
use sdi_core::requirement::Requirement;

fn row_to_req(row: &Row<'_>) -> rusqlite::Result<Requirement> {
    Ok(Requirement {
        id: Id::from(s(row, 0)?),
        plan_id: Id::from(s(row, 1)?),
        short_code: s(row, 2)?,
        title: s(row, 3)?,
        body: s(row, 4)?,
        source: s(row, 5)?,
        created_at: ts(row, 6)?,
        updated_at: ts(row, 7)?,
        produced_via_pattern_id: s_opt(row, 8)?,
    })
}

const COLS: &str =
    "id, plan_id, short_code, title, body, source, created_at, updated_at, produced_via_pattern_id";

pub fn insert(conn: &Connection, req: &Requirement) -> DomainResult<()> {
    conn.execute(
        &format!("INSERT INTO requirements({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"),
        params![
            req.id.as_str(),
            req.plan_id.as_str(),
            req.short_code,
            req.title,
            req.body,
            req.source,
            fmt_ts(req.created_at),
            fmt_ts(req.updated_at),
            req.produced_via_pattern_id,
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get(conn: &Connection, id: &Id) -> DomainResult<Requirement> {
    conn.query_row(
        &format!("SELECT {COLS} FROM requirements WHERE id = ?1"),
        [id.as_str()],
        row_to_req,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

pub fn list_by_plan(conn: &Connection, plan_id: &Id) -> DomainResult<Vec<Requirement>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM requirements WHERE plan_id = ?1 ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([plan_id.as_str()], row_to_req)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// SNAPSHOT-ONLY rewrite: replaces title/body/source in place. The Decision
/// log captures the "why" — this function never preserves the old body.
pub fn update_body(
    conn: &Connection,
    id: &Id,
    title: &str,
    body: &str,
    source: &str,
) -> DomainResult<()> {
    let n = conn
        .execute(
            "UPDATE requirements SET title = ?1, body = ?2, source = ?3, updated_at = ?4 \
             WHERE id = ?5",
            params![title, body, source, fmt_ts(now()), id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &Id) -> DomainResult<()> {
    let n = conn
        .execute("DELETE FROM requirements WHERE id = ?1", [id.as_str()])
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{plan as plan_repo, project as proj_repo};
    use crate::{ensure_schema, open_pool};
    use sdi_core::ids::IdKind;
    use sdi_core::plan::{Plan, PlanStatus};
    use sdi_core::project::Project;

    fn fixture() -> (r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, Id) {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-req-{}-{}.db",
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
            project_id: proj.id.clone(),
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

    #[test]
    fn snapshot_overwrite() {
        let (pool, plan_id) = fixture();
        let conn = pool.get().unwrap();
        let req = Requirement {
            id: Id::new(IdKind::Requirement),
            plan_id,
            short_code: format!("REQ-{}", ulid::Ulid::new()),
            title: "A".into(),
            body: "v1".into(),
            source: "snapshot".into(),
            produced_via_pattern_id: None,
            created_at: now(),
            updated_at: now(),
        };
        insert(&conn, &req).unwrap();
        update_body(&conn, &req.id, "A", "v2", "decision-rewrite").unwrap();
        let got = get(&conn, &req.id).unwrap();
        assert_eq!(got.body, "v2");
        assert_eq!(got.source, "decision-rewrite");
    }
}
