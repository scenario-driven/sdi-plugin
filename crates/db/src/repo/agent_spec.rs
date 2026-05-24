//! AgentSpec repository (Layer-2 specialists). Only canonical roles
//! (`STOCK_AGENTS` ∪ `STOCK_META_AGENTS`) are valid (`AgentSpec::validate_name`
//! enforces this before SQL). M5 sets `instance_count`; rows never get
//! deleted, only adjusted.
//!
//! v0.5 (D26 sybil-fix) widened the uniqueness tuple from `(name)` to
//! `(name, stance)` so the same role can carry multiple stance instances. The
//! repo writes the full v0.5 column set (stance, created_by, origin_plan_id,
//! tool_allowlist_json, decision_kinds_json, blast_radius_rules_json, status,
//! expires_at) on every upsert. `get_by_name` still resolves by name alone for
//! callers that only address the legacy stock row (which the migration
//! backfilled with stance=neutral). Stance-aware lookups go through
//! `get_by_name_stance`.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, parsed, s, s_opt, ts, ts_opt};
use rusqlite::{params, Connection, Row};
use sdi_core::agent_spec::{AgentSpec, AgentSpecStatus};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::Id;
use sdi_core::pattern::Stance;

const COLS: &str = "id, name, stance, role, system_prompt, instance_count, \
                    created_by, origin_plan_id, tool_allowlist_json, \
                    decision_kinds_json, blast_radius_rules_json, \
                    status, expires_at, created_at, updated_at";

fn row_to_spec(row: &Row<'_>) -> rusqlite::Result<AgentSpec> {
    Ok(AgentSpec {
        id: Id::from(s(row, 0)?),
        name: s(row, 1)?,
        stance: parsed::<Stance>(row, 2)?,
        role: s(row, 3)?,
        system_prompt: s(row, 4)?,
        instance_count: row.get(5)?,
        created_by: s_opt(row, 6)?,
        origin_plan_id: s_opt(row, 7)?.map(Id::from),
        tool_allowlist_json: s_opt(row, 8)?,
        decision_kinds_json: s_opt(row, 9)?,
        blast_radius_rules_json: s_opt(row, 10)?,
        status: parsed::<AgentSpecStatus>(row, 11)?,
        expires_at: ts_opt(row, 12)?,
        created_at: ts(row, 13)?,
        updated_at: ts(row, 14)?,
    })
}

pub fn upsert(conn: &Connection, spec: &AgentSpec) -> DomainResult<()> {
    AgentSpec::validate_name(&spec.name)?;
    AgentSpec::validate_instance_count(spec.instance_count)?;
    conn.execute(
        &format!(
            "INSERT INTO agent_specs({COLS}) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) \
             ON CONFLICT(name, stance) DO UPDATE SET \
                role = excluded.role, \
                system_prompt = excluded.system_prompt, \
                instance_count = excluded.instance_count, \
                created_by = excluded.created_by, \
                origin_plan_id = excluded.origin_plan_id, \
                tool_allowlist_json = excluded.tool_allowlist_json, \
                decision_kinds_json = excluded.decision_kinds_json, \
                blast_radius_rules_json = excluded.blast_radius_rules_json, \
                status = excluded.status, \
                expires_at = excluded.expires_at, \
                updated_at = excluded.updated_at"
        ),
        params![
            spec.id.as_str(),
            spec.name,
            spec.stance.to_string(),
            spec.role,
            spec.system_prompt,
            spec.instance_count,
            spec.created_by,
            spec.origin_plan_id.as_ref().map(|i| i.as_str().to_string()),
            spec.tool_allowlist_json,
            spec.decision_kinds_json,
            spec.blast_radius_rules_json,
            spec.status.to_string(),
            spec.expires_at.map(fmt_ts),
            fmt_ts(spec.created_at),
            fmt_ts(spec.updated_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

/// Resolves by `name` alone. Returns the row regardless of stance — useful for
/// legacy stock-agent callers; ambiguous when the role carries multiple
/// stances. Prefer `get_by_name_stance` when more than one stance is possible.
pub fn get_by_name(conn: &Connection, name: &str) -> DomainResult<AgentSpec> {
    conn.query_row(
        &format!("SELECT {COLS} FROM agent_specs WHERE name = ?1 LIMIT 1"),
        [name],
        row_to_spec,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(name.to_string()),
        other => map_sqlite_err(other),
    })
}

/// D26 — stance-aware lookup. Returns Err(NotFound) when no row matches.
pub fn get_by_name_stance(
    conn: &Connection,
    name: &str,
    stance: Stance,
) -> DomainResult<AgentSpec> {
    conn.query_row(
        &format!("SELECT {COLS} FROM agent_specs WHERE name = ?1 AND stance = ?2"),
        params![name, stance.to_string()],
        row_to_spec,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => {
            DomainError::NotFound(format!("{name}/{}", stance.as_str()))
        }
        other => map_sqlite_err(other),
    })
}

pub fn list_all(conn: &Connection) -> DomainResult<Vec<AgentSpec>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM agent_specs ORDER BY name, stance"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt.query_map([], row_to_spec).map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// D26 — list distinct (name, stance) tuples for a single role. The daemon's
/// graph consensus gate uses this to count sybil-resistant reviewers.
pub fn list_stances_for_name(conn: &Connection, name: &str) -> DomainResult<Vec<AgentSpec>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM agent_specs WHERE name = ?1 \
             AND status = 'active' ORDER BY stance"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([name], row_to_spec)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn set_instance_count(conn: &Connection, name: &str, instance_count: i64) -> DomainResult<()> {
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
            stance: Stance::Neutral,
            role: "specialist".into(),
            system_prompt: "stub".into(),
            instance_count: 1,
            created_by: None,
            origin_plan_id: None,
            tool_allowlist_json: None,
            decision_kinds_json: None,
            blast_radius_rules_json: None,
            status: AgentSpecStatus::Active,
            expires_at: None,
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn upsert_replaces_by_unique_name_stance() {
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

    #[test]
    fn same_name_distinct_stance_coexist() {
        let pool = fixture();
        let conn = pool.get().unwrap();
        let mut proposer = mk("impl-coder");
        proposer.stance = Stance::Proposer;
        proposer.id = Id::new(IdKind::AgentSpec);
        let mut devil = mk("impl-coder");
        devil.stance = Stance::DevilAdvocate;
        devil.id = Id::new(IdKind::AgentSpec);
        upsert(&conn, &proposer).unwrap();
        upsert(&conn, &devil).unwrap();
        let stances = list_stances_for_name(&conn, "impl-coder").unwrap();
        assert_eq!(stances.len(), 2);
    }

    #[test]
    fn v05_metadata_round_trips() {
        let pool = fixture();
        let conn = pool.get().unwrap();
        let mut s = mk("pattern-orchestrator");
        s.stance = Stance::Proposer;
        s.created_by = Some("seed".into());
        s.tool_allowlist_json = Some(r#"["Read","Bash"]"#.into());
        s.decision_kinds_json = Some(r#"["proposal","consensus"]"#.into());
        s.blast_radius_rules_json = Some(r#"{"architecture":{"add":2}}"#.into());
        upsert(&conn, &s).unwrap();
        let fetched = get_by_name_stance(&conn, "pattern-orchestrator", Stance::Proposer).unwrap();
        assert_eq!(fetched.created_by.as_deref(), Some("seed"));
        assert_eq!(
            fetched.tool_allowlist_json.as_deref(),
            Some(r#"["Read","Bash"]"#)
        );
        assert_eq!(
            fetched.decision_kinds_json.as_deref(),
            Some(r#"["proposal","consensus"]"#)
        );
        assert_eq!(
            fetched.blast_radius_rules_json.as_deref(),
            Some(r#"{"architecture":{"add":2}}"#)
        );
        assert_eq!(fetched.status, AgentSpecStatus::Active);
    }

    #[test]
    fn meta_agent_names_accepted() {
        let pool = fixture();
        let conn = pool.get().unwrap();
        for name in ["pattern-orchestrator", "pattern-critic", "reversal-runner"] {
            upsert(&conn, &mk(name)).unwrap();
        }
        let all = list_all(&conn).unwrap();
        assert!(all.iter().any(|a| a.name == "pattern-orchestrator"));
        assert!(all.iter().any(|a| a.name == "pattern-critic"));
        assert!(all.iter().any(|a| a.name == "reversal-runner"));
    }
}
