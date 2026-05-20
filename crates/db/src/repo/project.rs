//! Project repository — CRUD + cwd anchor management.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, s, ts};
use rusqlite::{params, Connection, Row};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};
use sdi_core::project::Project;

fn row_to_project(row: &Row<'_>, cwds: Vec<String>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: Id::from(s(row, 0)?),
        key: s(row, 1)?,
        name: s(row, 2)?,
        slug: s(row, 3)?,
        cwds,
        created_at: ts(row, 4)?,
        updated_at: ts(row, 5)?,
    })
}

pub fn insert(conn: &Connection, project: &Project) -> DomainResult<()> {
    conn.execute(
        "INSERT INTO projects(id, key, name, slug, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            project.id.as_str(),
            project.key,
            project.name,
            project.slug,
            fmt_ts(project.created_at),
            fmt_ts(project.updated_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    for cwd in &project.cwds {
        attach_cwd(conn, &project.id, cwd)?;
    }
    Ok(())
}

pub fn attach_cwd(conn: &Connection, project_id: &Id, cwd: &str) -> DomainResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO project_cwds(project_id, cwd, created_at) VALUES (?1, ?2, ?3)",
        params![project_id.as_str(), cwd, fmt_ts(now())],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn detach_cwd(conn: &Connection, project_id: &Id, cwd: &str) -> DomainResult<bool> {
    let n = conn
        .execute(
            "DELETE FROM project_cwds WHERE project_id = ?1 AND cwd = ?2",
            params![project_id.as_str(), cwd],
        )
        .map_err(map_sqlite_err)?;
    Ok(n > 0)
}

pub fn list_cwds(conn: &Connection, project_id: &Id) -> DomainResult<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT cwd FROM project_cwds WHERE project_id = ?1 ORDER BY cwd")
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([project_id.as_str()], |r| r.get::<_, String>(0))
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &Id) -> DomainResult<Project> {
    let cwds = list_cwds(conn, id)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, key, name, slug, created_at, updated_at \
             FROM projects WHERE id = ?1",
        )
        .map_err(map_sqlite_err)?;
    stmt.query_row([id.as_str()], |r| row_to_project(r, cwds.clone()))
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
            other => map_sqlite_err(other),
        })
}

pub fn find_by_key(conn: &Connection, key: &str) -> DomainResult<Option<Project>> {
    let id_opt: Option<String> = conn
        .query_row(
            "SELECT id FROM projects WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(map_sqlite_err(other)),
        })?;
    match id_opt {
        Some(id) => Ok(Some(get(conn, &Id::from(id))?)),
        None => Ok(None),
    }
}

pub fn find_by_cwd(conn: &Connection, cwd: &str) -> DomainResult<Option<Project>> {
    let id_opt: Option<String> = conn
        .query_row(
            "SELECT project_id FROM project_cwds WHERE cwd = ?1",
            params![cwd],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(map_sqlite_err(other)),
        })?;
    match id_opt {
        Some(id) => Ok(Some(get(conn, &Id::from(id))?)),
        None => Ok(None),
    }
}

pub fn list(conn: &Connection) -> DomainResult<Vec<Project>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, key, name, slug, created_at, updated_at \
             FROM projects ORDER BY created_at",
        )
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([], |r| {
            // We fill cwds in a second pass to keep this query single-table.
            row_to_project(r, vec![])
        })
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        let mut p = r.map_err(map_sqlite_err)?;
        p.cwds = list_cwds(conn, &p.id)?;
        out.push(p);
    }
    Ok(out)
}

/// Update name only (key/slug are immutable identifiers).
pub fn update_name(conn: &Connection, id: &Id, name: &str) -> DomainResult<()> {
    let n = conn
        .execute(
            "UPDATE projects SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![name, fmt_ts(now()), id.as_str()],
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
    use crate::{ensure_schema, open_pool};
    use sdi_core::ids::IdKind;

    fn fresh_pool() -> r2d2::Pool<r2d2_sqlite::SqliteConnectionManager> {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-project-{}-{}.db",
            std::process::id(),
            ulid::Ulid::new()
        ));
        let _ = std::fs::remove_file(&tmp);
        let pool = open_pool(&tmp).unwrap();
        ensure_schema(&pool.get().unwrap()).unwrap();
        pool
    }

    #[test]
    fn insert_then_get_roundtrip() {
        let pool = fresh_pool();
        let conn = pool.get().unwrap();
        let proj = Project {
            id: Id::new(IdKind::Project),
            key: "SDI".into(),
            name: "SDI".into(),
            slug: "sdi".into(),
            cwds: vec!["/tmp/sdi".into()],
            created_at: now(),
            updated_at: now(),
        };
        insert(&conn, &proj).unwrap();
        let got = get(&conn, &proj.id).unwrap();
        assert_eq!(got.key, "SDI");
        assert_eq!(got.cwds, vec!["/tmp/sdi"]);
        let by_cwd = find_by_cwd(&conn, "/tmp/sdi").unwrap().unwrap();
        assert_eq!(by_cwd.id, proj.id);
        let by_key = find_by_key(&conn, "SDI").unwrap().unwrap();
        assert_eq!(by_key.id, proj.id);
    }

    #[test]
    fn duplicate_key_conflicts() {
        let pool = fresh_pool();
        let conn = pool.get().unwrap();
        let make = |key: &str| Project {
            id: Id::new(IdKind::Project),
            key: key.into(),
            name: "n".into(),
            slug: format!("s-{}", ulid::Ulid::new()),
            cwds: vec![],
            created_at: now(),
            updated_at: now(),
        };
        insert(&conn, &make("DUP")).unwrap();
        let err = insert(&conn, &make("DUP")).unwrap_err();
        assert!(matches!(err, DomainError::Conflict(_)));
    }
}
