//! Scenario repository — GWT-strict (D5) authored rows. Insert validates
//! every field non-empty after trim before touching SQL.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, json, parsed, s, s_opt, ts};
use rusqlite::{params, Connection, Row};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};
use sdi_core::pattern::ClaimStatus;
use sdi_core::scenario::{validate_claimed_resources, Scenario, ScenarioStatus};

fn row_to_scenario(row: &Row<'_>) -> rusqlite::Result<Scenario> {
    Ok(Scenario {
        id: Id::from(s(row, 0)?),
        plan_id: Id::from(s(row, 1)?),
        short_code: s(row, 2)?,
        given: s(row, 3)?,
        when_clause: s(row, 4)?,
        then_clause: s(row, 5)?,
        tags: json::<Vec<String>>(row, 6)?,
        origin_round_id: s_opt(row, 7)?.map(Id::from),
        status: parsed::<ScenarioStatus>(row, 8)?,
        depends_on: json::<Vec<String>>(row, 9)?,
        produced_by: s_opt(row, 10)?,
        verified_by: s_opt(row, 11)?,
        claimed_resources_json: s(row, 12)?,
        claim_status: parsed::<ClaimStatus>(row, 13)?,
        produced_via_pattern_id: s_opt(row, 14)?,
        created_at: ts(row, 15)?,
        updated_at: ts(row, 16)?,
    })
}

const COLS: &str = "id, plan_id, short_code, given, when_clause, then_clause, tags, \
                    origin_round_id, status, depends_on, produced_by, verified_by, \
                    claimed_resources_json, claim_status, produced_via_pattern_id, \
                    created_at, updated_at";

pub fn insert(conn: &Connection, scenario: &Scenario) -> DomainResult<()> {
    Scenario::validate_gwt(&scenario.given, &scenario.when_clause, &scenario.then_clause)?;
    Scenario::validate_depends_on(&scenario.short_code, &scenario.depends_on)?;
    // D29 — claim payload must be a valid JSON array of globs.
    validate_claimed_resources(&scenario.claimed_resources_json)?;
    let tags = serde_json::to_string(&scenario.tags)
        .map_err(|e| DomainError::Validation(format!("encode tags: {e}")))?;
    let depends_on = serde_json::to_string(&scenario.depends_on)
        .map_err(|e| DomainError::Validation(format!("encode depends_on: {e}")))?;
    conn.execute(
        &format!(
            "INSERT INTO scenarios({COLS}) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)"
        ),
        params![
            scenario.id.as_str(),
            scenario.plan_id.as_str(),
            scenario.short_code,
            scenario.given,
            scenario.when_clause,
            scenario.then_clause,
            tags,
            scenario.origin_round_id.as_ref().map(|i| i.as_str().to_string()),
            scenario.status.to_string(),
            depends_on,
            scenario.produced_by,
            scenario.verified_by,
            scenario.claimed_resources_json,
            scenario.claim_status.to_string(),
            scenario.produced_via_pattern_id,
            fmt_ts(scenario.created_at),
            fmt_ts(scenario.updated_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get(conn: &Connection, id: &Id) -> DomainResult<Scenario> {
    conn.query_row(
        &format!("SELECT {COLS} FROM scenarios WHERE id = ?1"),
        [id.as_str()],
        row_to_scenario,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

pub fn list_by_plan(conn: &Connection, plan_id: &Id) -> DomainResult<Vec<Scenario>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM scenarios WHERE plan_id = ?1 ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([plan_id.as_str()], row_to_scenario)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn count_confirmed(conn: &Connection, plan_id: &Id) -> DomainResult<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM scenarios WHERE plan_id = ?1 AND status = 'confirmed'",
        [plan_id.as_str()],
        |r| r.get(0),
    )
    .map_err(map_sqlite_err)
}

pub fn update_gwt(
    conn: &Connection,
    id: &Id,
    given: &str,
    when_clause: &str,
    then_clause: &str,
) -> DomainResult<()> {
    Scenario::validate_gwt(given, when_clause, then_clause)?;
    let n = conn
        .execute(
            "UPDATE scenarios SET given = ?1, when_clause = ?2, then_clause = ?3, updated_at = ?4 \
             WHERE id = ?5",
            params![given, when_clause, then_clause, fmt_ts(now()), id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

pub fn set_status(conn: &Connection, id: &Id, status: ScenarioStatus) -> DomainResult<()> {
    let n = conn
        .execute(
            "UPDATE scenarios SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status.to_string(), fmt_ts(now()), id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

/// D29 — set claim_status on a scenario row. Caller (daemon router) is
/// responsible for the transition rules (none → requested → active → released)
/// and overlap detection across other active claims.
pub fn set_claim_status(
    conn: &Connection,
    id: &Id,
    status: ClaimStatus,
) -> DomainResult<()> {
    let n = conn
        .execute(
            "UPDATE scenarios SET claim_status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status.to_string(), fmt_ts(now()), id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

/// D29 — list all scenarios with `claim_status='active'`, optionally narrowed
/// to a single plan. Used by `/scenarios/active-claims` for the PreToolUse
/// overlap check.
pub fn list_active_claims(
    conn: &Connection,
    plan_id: Option<&Id>,
) -> DomainResult<Vec<Scenario>> {
    let (sql, params): (String, Vec<String>) = match plan_id {
        Some(pid) => (
            format!(
                "SELECT {COLS} FROM scenarios \
                 WHERE claim_status = 'active' AND plan_id = ?1 \
                 ORDER BY updated_at"
            ),
            vec![pid.as_str().to_string()],
        ),
        None => (
            format!(
                "SELECT {COLS} FROM scenarios \
                 WHERE claim_status = 'active' ORDER BY updated_at"
            ),
            vec![],
        ),
    };
    let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params.iter()), row_to_scenario)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// FTS5-backed keyword search across given/when/then. Returns scenario IDs.
pub fn search(conn: &Connection, plan_id: &Id, query: &str, limit: usize) -> DomainResult<Vec<Scenario>> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.plan_id, s.short_code, s.given, s.when_clause, s.then_clause, \
                    s.tags, s.origin_round_id, s.status, s.depends_on, s.produced_by, \
                    s.verified_by, s.claimed_resources_json, s.claim_status, \
                    s.produced_via_pattern_id, s.created_at, s.updated_at \
             FROM scenarios s JOIN scenarios_fts f ON f.rowid = s.rowid \
             WHERE s.plan_id = ?1 AND scenarios_fts MATCH ?2 \
             ORDER BY rank LIMIT ?3",
        )
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map(params![plan_id.as_str(), query, limit as i64], row_to_scenario)
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
    use sdi_core::ids::IdKind;
    use sdi_core::plan::{Plan, PlanStatus};
    use sdi_core::project::Project;

    fn fixture() -> (r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, Id) {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-scn-{}-{}.db",
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
            approved_at: None,
            completed_at: None,
            created_at: now(),
            updated_at: now(),
        };
        plan_repo::insert(&conn, &plan).unwrap();
        (pool, plan.id)
    }

    fn mk(plan_id: Id) -> Scenario {
        Scenario {
            id: Id::new(IdKind::Scenario),
            plan_id,
            short_code: format!("SCN-{}", ulid::Ulid::new()),
            given: "user is logged in".into(),
            when_clause: "they click checkout".into(),
            then_clause: "an order is created".into(),
            tags: vec!["checkout".into(), "happy".into()],
            origin_round_id: None,
            status: ScenarioStatus::Draft,
            depends_on: vec![],
            produced_by: None,
            verified_by: None,
            claimed_resources_json: "[]".into(),
            claim_status: ClaimStatus::None,
            produced_via_pattern_id: None,
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn empty_gwt_rejected_at_insert() {
        let (pool, plan_id) = fixture();
        let conn = pool.get().unwrap();
        let mut s = mk(plan_id);
        s.given = "  ".into();
        let err = insert(&conn, &s).unwrap_err();
        assert!(matches!(err, DomainError::GwtEmpty { field: "given" }));
    }

    #[test]
    fn fts_round_trip() {
        let (pool, plan_id) = fixture();
        let conn = pool.get().unwrap();
        let s = mk(plan_id.clone());
        insert(&conn, &s).unwrap();
        let hits = search(&conn, &plan_id, "checkout", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, s.id);
    }

    #[test]
    fn v05_columns_round_trip() {
        let (pool, plan_id) = fixture();
        let conn = pool.get().unwrap();
        // Seed a pattern row so the FK on produced_via_pattern_id resolves.
        let pat = crate::repo::pattern::PatternRow {
            id: format!("CP-{}", ulid::Ulid::new()),
            short_code: "CP-rt".into(),
            plan_id: plan_id.as_str().into(),
            kind: "direct".into(),
            applies_to: "scenario".into(),
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
        };
        crate::repo::pattern::insert(&conn, &pat).unwrap();
        let mut s = mk(plan_id);
        s.claimed_resources_json = r#"["crates/db/migrations/*.sql"]"#.into();
        s.produced_via_pattern_id = Some(pat.id.clone());
        insert(&conn, &s).unwrap();
        let fetched = get(&conn, &s.id).unwrap();
        assert_eq!(fetched.claimed_resources_json, s.claimed_resources_json);
        assert_eq!(fetched.claim_status, ClaimStatus::None);
        assert_eq!(fetched.produced_via_pattern_id.as_deref(), Some(pat.id.as_str()));
    }

    #[test]
    fn invalid_claim_glob_rejected_at_insert() {
        let (pool, plan_id) = fixture();
        let conn = pool.get().unwrap();
        let mut s = mk(plan_id);
        s.claimed_resources_json = r#"["**"]"#.into();
        let err = insert(&conn, &s).unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn list_active_claims_returns_only_active() {
        let (pool, plan_id) = fixture();
        let conn = pool.get().unwrap();
        let mut a = mk(plan_id.clone());
        a.claimed_resources_json = r#"["crates/db/**.sql"]"#.into();
        insert(&conn, &a).unwrap();
        let mut b = mk(plan_id.clone());
        b.claimed_resources_json = r#"["plugin/agents/*.md"]"#.into();
        insert(&conn, &b).unwrap();
        set_claim_status(&conn, &a.id, ClaimStatus::Active).unwrap();
        let act = list_active_claims(&conn, Some(&plan_id)).unwrap();
        assert_eq!(act.len(), 1);
        assert_eq!(act[0].id, a.id);
    }
}
