//! Plan repository — CRUD + status transitions enforced via domain check.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, parsed, s, ts, ts_opt};
use rusqlite::{params, Connection, Row};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};
use sdi_core::plan::{Plan, PlanStatus};

fn row_to_plan(row: &Row<'_>) -> rusqlite::Result<Plan> {
    Ok(Plan {
        id: Id::from(s(row, 0)?),
        project_id: Id::from(s(row, 1)?),
        short_code: s(row, 2)?,
        title: s(row, 3)?,
        body: s(row, 4)?,
        status: parsed::<PlanStatus>(row, 5)?,
        version: row.get(6)?,
        approved_at: ts_opt(row, 7)?,
        completed_at: ts_opt(row, 8)?,
        created_at: ts(row, 9)?,
        updated_at: ts(row, 10)?,
    })
}

const COLS: &str =
    "id, project_id, short_code, title, body, status, version, approved_at, completed_at, created_at, updated_at";

pub fn insert(conn: &Connection, plan: &Plan) -> DomainResult<()> {
    conn.execute(
        &format!("INSERT INTO plans({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"),
        params![
            plan.id.as_str(),
            plan.project_id.as_str(),
            plan.short_code,
            plan.title,
            plan.body,
            plan.status.to_string(),
            plan.version,
            plan.approved_at.map(fmt_ts),
            plan.completed_at.map(fmt_ts),
            fmt_ts(plan.created_at),
            fmt_ts(plan.updated_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get(conn: &Connection, id: &Id) -> DomainResult<Plan> {
    conn.query_row(
        &format!("SELECT {COLS} FROM plans WHERE id = ?1"),
        [id.as_str()],
        row_to_plan,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

pub fn list_by_project(conn: &Connection, project_id: &Id) -> DomainResult<Vec<Plan>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM plans WHERE project_id = ?1 ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([project_id.as_str()], row_to_plan)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn find_active_for_project(conn: &Connection, project_id: &Id) -> DomainResult<Option<Plan>> {
    let r = conn.query_row(
        &format!("SELECT {COLS} FROM plans WHERE project_id = ?1 AND status = 'active'"),
        [project_id.as_str()],
        row_to_plan,
    );
    match r {
        Ok(p) => Ok(Some(p)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(map_sqlite_err(e)),
    }
}

pub fn count_confirmed_scenarios(conn: &Connection, plan_id: &Id) -> DomainResult<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM scenarios WHERE plan_id = ?1 AND status = 'confirmed'",
        [plan_id.as_str()],
        |r| r.get(0),
    )
    .map_err(map_sqlite_err)
}

/// Update mutable body/title fields. Bumps `version` + `updated_at`.
pub fn update_body(conn: &Connection, id: &Id, title: &str, body: &str) -> DomainResult<i64> {
    let now_s = fmt_ts(now());
    let n = conn
        .execute(
            "UPDATE plans SET title = ?1, body = ?2, version = version + 1, updated_at = ?3 \
             WHERE id = ?4",
            params![title, body, now_s, id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    let v: i64 = conn
        .query_row(
            "SELECT version FROM plans WHERE id = ?1",
            [id.as_str()],
            |r| r.get(0),
        )
        .map_err(map_sqlite_err)?;
    Ok(v)
}

/// Approve = draft → active. Caller must have already validated scenarios>=1 via domain check.
pub fn set_status(
    conn: &Connection,
    id: &Id,
    new_status: PlanStatus,
    approved_at: Option<chrono::DateTime<chrono::Utc>>,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
) -> DomainResult<()> {
    let n = conn
        .execute(
            "UPDATE plans SET status = ?1, approved_at = COALESCE(?2, approved_at), \
             completed_at = COALESCE(?3, completed_at), updated_at = ?4 WHERE id = ?5",
            params![
                new_status.to_string(),
                approved_at.map(fmt_ts),
                completed_at.map(fmt_ts),
                fmt_ts(now()),
                id.as_str(),
            ],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::project as proj_repo;
    use crate::{ensure_schema, open_pool};
    use sdi_core::ids::IdKind;
    use sdi_core::project::Project;

    fn pool_with_project() -> (r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, Id) {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-plan-{}-{}.db",
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
            name: "Test".into(),
            slug: format!("tst-{}", ulid::Ulid::new()),
            cwds: vec![],
            description: None,
            enabled: true,
            wiki_paths: vec!["docs".into()],
            created_at: now(),
            updated_at: now(),
        };
        proj_repo::insert(&conn, &proj).unwrap();
        (pool, proj.id)
    }

    fn sample_plan(project_id: Id) -> Plan {
        Plan {
            id: Id::new(IdKind::Plan),
            project_id,
            short_code: format!("TST-{}", ulid::Ulid::new()),
            title: "v0.1".into(),
            body: "body".into(),
            status: PlanStatus::Draft,
            version: 0,
            approved_at: None,
            completed_at: None,
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn insert_and_query_active() {
        let (pool, project_id) = pool_with_project();
        let conn = pool.get().unwrap();
        let mut plan = sample_plan(project_id.clone());
        insert(&conn, &plan).unwrap();
        plan.status = PlanStatus::Active;
        plan.approved_at = Some(now());
        set_status(&conn, &plan.id, PlanStatus::Active, plan.approved_at, None).unwrap();
        let active = find_active_for_project(&conn, &project_id)
            .unwrap()
            .unwrap();
        assert_eq!(active.id, plan.id);
    }

    #[test]
    fn body_update_bumps_version() {
        let (pool, project_id) = pool_with_project();
        let conn = pool.get().unwrap();
        let plan = sample_plan(project_id);
        insert(&conn, &plan).unwrap();
        let v1 = update_body(&conn, &plan.id, "v0.1 rev", "new body").unwrap();
        let v2 = update_body(&conn, &plan.id, "v0.1 rev2", "newer").unwrap();
        assert_eq!(v1, 1);
        assert_eq!(v2, 2);
    }

    #[test]
    fn second_active_plan_per_project_conflicts() {
        let (pool, project_id) = pool_with_project();
        let conn = pool.get().unwrap();
        let a = sample_plan(project_id.clone());
        let b = sample_plan(project_id.clone());
        insert(&conn, &a).unwrap();
        insert(&conn, &b).unwrap();
        set_status(&conn, &a.id, PlanStatus::Active, Some(now()), None).unwrap();
        let err = set_status(&conn, &b.id, PlanStatus::Active, Some(now()), None).unwrap_err();
        assert!(matches!(err, DomainError::Conflict(_)));
    }
}
