//! AgentSpec repository (Layer-2 specialists). Only the eight stock roles
//! are valid (`AgentSpec::validate_name` enforces this before SQL). M5 sets
//! `instance_count`; rows never get deleted, only adjusted.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, s, ts};
use rusqlite::{params, Connection, Row};
use sdi_core::agent_spec::AgentSpec;
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::Id;

const COLS: &str =
    "id, name, role, system_prompt, instance_count, created_at, updated_at";

fn row_to_spec(row: &Row<'_>) -> rusqlite::Result<AgentSpec> {
    Ok(AgentSpec {
        id: Id::from(s(row, 0)?),
        name: s(row, 1)?,
        role: s(row, 2)?,
        system_prompt: s(row, 3)?,
        instance_count: row.get(4)?,
        created_at: ts(row, 5)?,
        updated_at: ts(row, 6)?,
    })
}

pub fn upsert(conn: &Connection, spec: &AgentSpec) -> DomainResult<()> {
    AgentSpec::validate_name(&spec.name)?;
    AgentSpec::validate_instance_count(spec.instance_count)?;
    conn.execute(
        &format!(
            "INSERT INTO agent_specs({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7) \
             ON CONFLICT(name) DO UPDATE SET \
                role = excluded.role, \
                system_prompt = excluded.system_prompt, \
                instance_count = excluded.instance_count, \
                updated_at = excluded.updated_at"
        ),
        params![
            spec.id.as_str(),
            spec.name,
            spec.role,
            spec.system_prompt,
            spec.instance_count,
            fmt_ts(spec.created_at),
            fmt_ts(spec.updated_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get_by_name(conn: &Connection, name: &str) -> DomainResult<AgentSpec> {
    conn.query_row(
        &format!("SELECT {COLS} FROM agent_specs WHERE name = ?1"),
        [name],
        row_to_spec,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(name.to_string()),
        other => map_sqlite_err(other),
    })
}

pub fn list_all(conn: &Connection) -> DomainResult<Vec<AgentSpec>> {
    let mut stmt = conn
        .prepare(&format!("SELECT {COLS} FROM agent_specs ORDER BY name"))
        .map_err(map_sqlite_err)?;
    let rows = stmt.query_map([], row_to_spec).map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn set_instance_count(
    conn: &Connection,
    name: &str,
    instance_count: i64,
) -> DomainResult<()> {
    AgentSpec::validate_instance_count(instance_count)?;
    let n = conn
        .execute(
            "UPDATE agent_specs SET instance_count = ?1, updated_at = ?2 WHERE name = ?3",
            params![instance_count, fmt_ts(sdi_core::ids::now()), name],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(name.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ensure_schema, open_pool};
    use sdi_core::ids::{now, IdKind};

    fn fixture() -> r2d2::Pool<r2d2_sqlite::SqliteConnectionManager> {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-aspec-{}-{}.db",
            std::process::id(),
            ulid::Ulid::new()
        ));
        let _ = std::fs::remove_file(&tmp);
        let pool = open_pool(&tmp).unwrap();
        ensure_schema(&pool.get().unwrap()).unwrap();
        pool
    }

    fn mk(name: &str) -> AgentSpec {
        AgentSpec {
            id: Id::new(IdKind::AgentSpec),
            name: name.into(),
            role: "specialist".into(),
            system_prompt: "stub".into(),
            instance_count: 1,
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn upsert_replaces_by_unique_name() {
        let pool = fixture();
        let conn = pool.get().unwrap();
        upsert(&conn, &mk("impl-coder")).unwrap();
        let mut updated = mk("impl-coder");
        updated.role = "lead".into();
        upsert(&conn, &updated).unwrap();
        let row = get_by_name(&conn, "impl-coder").unwrap();
        assert_eq!(row.role, "lead");
    }

    #[test]
    fn unknown_role_rejected_before_sql() {
        let pool = fixture();
        let conn = pool.get().unwrap();
        let err = upsert(&conn, &mk("rogue-bot")).unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn instance_count_bounds_enforced() {
        let pool = fixture();
        let conn = pool.get().unwrap();
        upsert(&conn, &mk("impl-coder")).unwrap();
        let err = set_instance_count(&conn, "impl-coder", 100).unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }
}
