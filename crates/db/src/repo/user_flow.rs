//! UserFlow repository (PRD-v2 D33). L1 tier — one persona × one purpose journey.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, parsed, s, s_opt, ts};
use rusqlite::{params, Connection, Row};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};
use sdi_core::user_flow::{FlowStatus, UserFlow};

fn row_to_flow(row: &Row<'_>) -> rusqlite::Result<UserFlow> {
    Ok(UserFlow {
        id: Id::from(s(row, 0)?),
        project_id: Id::from(s(row, 1)?),
        short_code: s(row, 2)?,
        persona_id: Id::from(s(row, 3)?),
        purpose: s(row, 4)?,
        steps_json: s(row, 5)?,
        covers_capabilities_json: s(row, 6)?,
        status: parsed::<FlowStatus>(row, 7)?,
        produced_via_pattern_id: s_opt(row, 8)?,
        created_at: ts(row, 9)?,
        updated_at: ts(row, 10)?,
    })
}

const COLS: &str = "id, project_id, short_code, persona_id, purpose, steps_json, \
                    covers_capabilities_json, status, produced_via_pattern_id, \
                    created_at, updated_at";

pub fn insert(conn: &Connection, flow: &UserFlow) -> DomainResult<()> {
    UserFlow::validate_purpose(&flow.purpose)?;
    UserFlow::parse_steps(&flow.steps_json)?;
    UserFlow::parse_covers_capabilities(&flow.covers_capabilities_json)?;
    conn.execute(
        &format!("INSERT INTO user_flows({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"),
        params![
            flow.id.as_str(),
            flow.project_id.as_str(),
            flow.short_code,
            flow.persona_id.as_str(),
            flow.purpose,
            flow.steps_json,
            flow.covers_capabilities_json,
            flow.status.to_string(),
            flow.produced_via_pattern_id,
            fmt_ts(flow.created_at),
            fmt_ts(flow.updated_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get(conn: &Connection, id: &Id) -> DomainResult<UserFlow> {
    conn.query_row(
        &format!("SELECT {COLS} FROM user_flows WHERE id = ?1"),
        [id.as_str()],
        row_to_flow,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

pub fn list_by_project(conn: &Connection, project_id: &Id) -> DomainResult<Vec<UserFlow>> {
    query(
        conn,
        &format!("SELECT {COLS} FROM user_flows WHERE project_id = ?1 ORDER BY created_at"),
        params![project_id.as_str()],
    )
}

pub fn list_by_persona(conn: &Connection, persona_id: &Id) -> DomainResult<Vec<UserFlow>> {
    query(
        conn,
        &format!("SELECT {COLS} FROM user_flows WHERE persona_id = ?1 ORDER BY created_at"),
        params![persona_id.as_str()],
    )
}

fn query(conn: &Connection, sql: &str, p: impl rusqlite::Params) -> DomainResult<Vec<UserFlow>> {
    let mut stmt = conn.prepare(sql).map_err(map_sqlite_err)?;
    let rows = stmt.query_map(p, row_to_flow).map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn set_status(conn: &Connection, id: &Id, status: FlowStatus) -> DomainResult<()> {
    let n = conn
        .execute(
            "UPDATE user_flows SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status.to_string(), fmt_ts(now()), id.as_str()],
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
    use crate::repo::ssot as ssot_repo;
    use crate::{ensure_schema, open_pool};
    use sdi_core::ids::IdKind;
    use sdi_core::project::Project;
    use sdi_core::ssot::{Confidence, SsotNode};

    fn fixture() -> (r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, Id, Id) {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-uf-{}-{}.db",
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
        let persona = SsotNode {
            id: Id::new(IdKind::SsotNode),
            project_id: proj.id.clone(),
            short_code: format!("SN-{}", ulid::Ulid::new()),
            kind: "Persona".into(),
            title: "Power user".into(),
            facets_json: "{}".into(),
            open_markers_json: "[]".into(),
            confidence: Confidence::Unverified,
            produced_via_pattern_id: None,
            created_at: now(),
            updated_at: now(),
        };
        ssot_repo::insert_node(&conn, &persona).unwrap();
        (pool, proj.id, persona.id)
    }

    fn mk(project_id: Id, persona_id: Id) -> UserFlow {
        UserFlow {
            id: Id::new(IdKind::UserFlow),
            project_id,
            short_code: format!("UF-{}", ulid::Ulid::new()),
            persona_id,
            purpose: "결제를 완료한다".into(),
            steps_json: r#"[{"idx":0,"description":"장바구니"},{"idx":1,"description":"결제"}]"#
                .into(),
            covers_capabilities_json: "[]".into(),
            status: FlowStatus::Draft,
            produced_via_pattern_id: None,
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn roundtrip_and_status() {
        let (pool, pid, persona) = fixture();
        let conn = pool.get().unwrap();
        let f = mk(pid.clone(), persona.clone());
        insert(&conn, &f).unwrap();
        assert_eq!(get(&conn, &f.id).unwrap().purpose, "결제를 완료한다");
        assert_eq!(list_by_project(&conn, &pid).unwrap().len(), 1);
        assert_eq!(list_by_persona(&conn, &persona).unwrap().len(), 1);
        set_status(&conn, &f.id, FlowStatus::Confirmed).unwrap();
        assert_eq!(get(&conn, &f.id).unwrap().status, FlowStatus::Confirmed);
    }

    #[test]
    fn empty_purpose_rejected() {
        let (pool, pid, persona) = fixture();
        let conn = pool.get().unwrap();
        let mut f = mk(pid, persona);
        f.purpose = "  ".into();
        assert!(matches!(insert(&conn, &f), Err(DomainError::Validation(_))));
    }
}
