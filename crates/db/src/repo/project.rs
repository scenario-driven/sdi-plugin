//! Project repository — CRUD + cwd anchor management.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, s, ts};
use rusqlite::{params, params_from_iter, types::Value as SqlValue, Connection, Row};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};
use sdi_core::project::{default_wiki_paths, Project};

fn row_to_project(row: &Row<'_>, cwds: Vec<String>) -> rusqlite::Result<Project> {
    let wiki_paths_json: String = row.get(8)?;
    let wiki_paths: Vec<String> =
        serde_json::from_str(&wiki_paths_json).unwrap_or_else(|_| default_wiki_paths());
    let enabled_int: i64 = row.get(7)?;
    Ok(Project {
        id: Id::from(s(row, 0)?),
        key: s(row, 1)?,
        name: s(row, 2)?,
        slug: s(row, 3)?,
        cwds,
        description: row.get::<_, Option<String>>(6)?,
        enabled: enabled_int != 0,
        wiki_paths,
        created_at: ts(row, 4)?,
        updated_at: ts(row, 5)?,
    })
}

const SELECT_COLUMNS: &str =
    "id, key, name, slug, created_at, updated_at, description, enabled, wiki_paths_json";

pub fn insert(conn: &Connection, project: &Project) -> DomainResult<()> {
    let wiki_paths_json = serde_json::to_string(&project.wiki_paths)
        .unwrap_or_else(|_| "[\"docs\"]".to_string());
    conn.execute(
        "INSERT INTO projects(id, key, name, slug, created_at, updated_at, \
         description, enabled, wiki_paths_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            project.id.as_str(),
            project.key,
            project.name,
            project.slug,
            fmt_ts(project.created_at),
            fmt_ts(project.updated_at),
            project.description,
            if project.enabled { 1_i64 } else { 0_i64 },
            wiki_paths_json,
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
    let sql = format!("SELECT {SELECT_COLUMNS} FROM projects WHERE id = ?1");
    let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
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
    let sql = format!("SELECT {SELECT_COLUMNS} FROM projects ORDER BY created_at");
    let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
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

/// Update name only (key/slug are immutable identifiers). Kept as a thin
/// wrapper around `update_fields` for backward-compatible callers.
pub fn update_name(conn: &Connection, id: &Id, name: &str) -> DomainResult<()> {
    update_fields(
        conn,
        id,
        &UpdateFields {
            name: Some(name.to_string()),
            ..UpdateFields::default()
        },
    )
}

/// PATCH-style field-set for `update_fields`.
///
/// - `Option<None>` on `description` clears the field to NULL; `Some(String)`
///   sets it.
/// - `enabled` is a plain bool — `None` leaves it untouched.
/// - `wiki_paths` replaces the entire array (snapshot semantics on this
///   sub-field, mirroring the Clawket pattern).
#[derive(Debug, Default, Clone)]
pub struct UpdateFields {
    pub name: Option<String>,
    /// Outer `Option`: present in the patch? Inner `Option`: set or clear?
    pub description: Option<Option<String>>,
    pub enabled: Option<bool>,
    pub wiki_paths: Option<Vec<String>>,
}

pub fn update_fields(conn: &Connection, id: &Id, fields: &UpdateFields) -> DomainResult<()> {
    let mut sets: Vec<&'static str> = Vec::new();
    let mut vals: Vec<SqlValue> = Vec::new();

    if let Some(name) = &fields.name {
        sets.push("name = ?");
        vals.push(SqlValue::Text(name.clone()));
    }
    if let Some(desc) = &fields.description {
        sets.push("description = ?");
        vals.push(match desc {
            Some(s) => SqlValue::Text(s.clone()),
            None => SqlValue::Null,
        });
    }
    if let Some(enabled) = fields.enabled {
        sets.push("enabled = ?");
        vals.push(SqlValue::Integer(if enabled { 1 } else { 0 }));
    }
    if let Some(wp) = &fields.wiki_paths {
        sets.push("wiki_paths_json = ?");
        let json = serde_json::to_string(wp).unwrap_or_else(|_| "[\"docs\"]".to_string());
        vals.push(SqlValue::Text(json));
    }

    if sets.is_empty() {
        // Nothing to update — confirm the project exists so the caller's
        // `get()` round-trip surfaces NotFound consistently.
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM projects WHERE id = ?1",
                params![id.as_str()],
                |r| r.get(0),
            )
            .map_err(map_sqlite_err)?;
        if exists == 0 {
            return Err(DomainError::NotFound(id.to_string()));
        }
        return Ok(());
    }

    sets.push("updated_at = ?");
    vals.push(SqlValue::Text(fmt_ts(now())));
    vals.push(SqlValue::Text(id.to_string()));

    let sql = format!(
        "UPDATE projects SET {} WHERE id = ?",
        sets.join(", "),
    );
    let n = conn
        .execute(&sql, params_from_iter(vals.iter()))
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

/// Soft-disable the project — sets `enabled = 0`. Idempotent.
pub fn set_enabled(conn: &Connection, id: &Id, enabled: bool) -> DomainResult<()> {
    update_fields(
        conn,
        id,
        &UpdateFields {
            enabled: Some(enabled),
            ..UpdateFields::default()
        },
    )
}

/// Count of tasks anchored to the project whose `status = 'in_progress'`.
/// The router uses this to enforce the default-409 on `DELETE /projects/:id`
/// — `?force=true` callers skip the check.
pub fn count_in_flight_tasks(conn: &Connection, id: &Id) -> DomainResult<i64> {
    conn.query_row(
        "SELECT COUNT(1) \
         FROM tasks t \
         JOIN rounds r ON r.id = t.round_id \
         JOIN plans  p ON p.id = r.plan_id \
         WHERE p.project_id = ?1 AND t.status = 'in_progress'",
        params![id.as_str()],
        |r| r.get(0),
    )
    .map_err(map_sqlite_err)
}

/// Hard-delete the project and every PROJ-scoped row across the schema.
///
/// Every project-FK table declares `ON DELETE CASCADE` (see migrations
/// 001/003/007 — `plans`, `project_cwds`, `comments`, `activity`,
/// `knowledge`, `events`, `autonomy_policies`, `collaboration_patterns`)
/// and the plan-FK tables (`requirements`, `decisions`, `scenarios`,
/// `rounds`, `disruption_reviews`, `questions`) cascade transitively.
/// `agent_notes` carries `project_id NOT NULL REFERENCES projects(id)
/// ON DELETE CASCADE` from migration 006. With `PRAGMA foreign_keys=ON`
/// (enforced by `pool::open_pool`), a single `DELETE FROM projects`
/// statement tears the whole subtree down atomically. We still wrap the
/// delete in an explicit transaction so a mid-cascade error rolls back
/// instead of leaving orphans.
pub fn delete_cascade(conn: &mut Connection, id: &Id) -> DomainResult<()> {
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(map_sqlite_err)?;
    let n = tx
        .execute(
            "DELETE FROM projects WHERE id = ?1",
            params![id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        // Roll back so the caller can surface NotFound without leaving a
        // dangling transaction on the connection.
        let _ = tx.rollback();
        return Err(DomainError::NotFound(id.to_string()));
    }
    tx.commit().map_err(map_sqlite_err)?;
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

    fn fresh_project(key: &str) -> Project {
        Project {
            id: Id::new(IdKind::Project),
            key: key.into(),
            name: "n".into(),
            slug: format!("s-{}", ulid::Ulid::new()),
            cwds: vec![],
            description: None,
            enabled: true,
            wiki_paths: default_wiki_paths(),
            created_at: now(),
            updated_at: now(),
        }
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
            description: Some("seed".into()),
            enabled: true,
            wiki_paths: vec!["docs".into(), "wiki".into()],
            created_at: now(),
            updated_at: now(),
        };
        insert(&conn, &proj).unwrap();
        let got = get(&conn, &proj.id).unwrap();
        assert_eq!(got.key, "SDI");
        assert_eq!(got.cwds, vec!["/tmp/sdi"]);
        assert_eq!(got.description.as_deref(), Some("seed"));
        assert!(got.enabled);
        assert_eq!(got.wiki_paths, vec!["docs", "wiki"]);
        let by_cwd = find_by_cwd(&conn, "/tmp/sdi").unwrap().unwrap();
        assert_eq!(by_cwd.id, proj.id);
        let by_key = find_by_key(&conn, "SDI").unwrap().unwrap();
        assert_eq!(by_key.id, proj.id);
    }

    #[test]
    fn duplicate_key_conflicts() {
        let pool = fresh_pool();
        let conn = pool.get().unwrap();
        insert(&conn, &fresh_project("DUP")).unwrap();
        let err = insert(&conn, &fresh_project("DUP")).unwrap_err();
        assert!(matches!(err, DomainError::Conflict(_)));
    }

    #[test]
    fn update_fields_patches_in_place() {
        let pool = fresh_pool();
        let conn = pool.get().unwrap();
        let p = fresh_project("UPD");
        insert(&conn, &p).unwrap();
        update_fields(
            &conn,
            &p.id,
            &UpdateFields {
                name: Some("Renamed".into()),
                description: Some(Some("a body".into())),
                enabled: Some(false),
                wiki_paths: Some(vec!["docs".into(), "wiki".into(), "/abs/path".into()]),
            },
        )
        .unwrap();
        let got = get(&conn, &p.id).unwrap();
        assert_eq!(got.name, "Renamed");
        assert_eq!(got.description.as_deref(), Some("a body"));
        assert!(!got.enabled);
        assert_eq!(got.wiki_paths.len(), 3);

        // Clearing description to NULL.
        update_fields(
            &conn,
            &p.id,
            &UpdateFields {
                description: Some(None),
                ..UpdateFields::default()
            },
        )
        .unwrap();
        assert!(get(&conn, &p.id).unwrap().description.is_none());
    }

    #[test]
    fn update_fields_on_unknown_id_returns_not_found() {
        let pool = fresh_pool();
        let conn = pool.get().unwrap();
        let err = update_fields(
            &conn,
            &Id::new(IdKind::Project),
            &UpdateFields {
                name: Some("ghost".into()),
                ..UpdateFields::default()
            },
        )
        .unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)));
    }

    #[test]
    fn delete_cascade_removes_project_and_children() {
        let pool = fresh_pool();
        let mut conn = pool.get().unwrap();
        let p = fresh_project("DEL");
        insert(&conn, &p).unwrap();
        attach_cwd(&conn, &p.id, "/tmp/del").unwrap();
        // Sanity: cwd row present before delete.
        let cwd_before: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM project_cwds WHERE project_id = ?1",
                params![p.id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cwd_before, 1);
        delete_cascade(&mut conn, &p.id).unwrap();
        // Project gone.
        assert!(matches!(
            get(&conn, &p.id).unwrap_err(),
            DomainError::NotFound(_)
        ));
        // Cascade reached project_cwds.
        let cwd_after: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM project_cwds WHERE project_id = ?1",
                params![p.id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cwd_after, 0);
    }

    #[test]
    fn delete_cascade_on_unknown_id_returns_not_found() {
        let pool = fresh_pool();
        let mut conn = pool.get().unwrap();
        let err = delete_cascade(&mut conn, &Id::new(IdKind::Project)).unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)));
    }

    #[test]
    fn count_in_flight_tasks_zero_on_empty_project() {
        let pool = fresh_pool();
        let conn = pool.get().unwrap();
        let p = fresh_project("CNT");
        insert(&conn, &p).unwrap();
        assert_eq!(count_in_flight_tasks(&conn, &p.id).unwrap(), 0);
    }

    #[test]
    fn set_enabled_toggles_idempotently() {
        let pool = fresh_pool();
        let conn = pool.get().unwrap();
        let p = fresh_project("EN");
        insert(&conn, &p).unwrap();
        set_enabled(&conn, &p.id, false).unwrap();
        assert!(!get(&conn, &p.id).unwrap().enabled);
        // Idempotent.
        set_enabled(&conn, &p.id, false).unwrap();
        assert!(!get(&conn, &p.id).unwrap().enabled);
        set_enabled(&conn, &p.id, true).unwrap();
        assert!(get(&conn, &p.id).unwrap().enabled);
    }
}
