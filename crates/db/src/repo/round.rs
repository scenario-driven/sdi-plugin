//! Round repository — round lifecycle + per-round scenario verdicts.
//!
//! `start_round_with_carryover` implements PRD §6.3 auto-regression: when a
//! new round is activated under `strict-regression`, every confirmed scenario
//! from the prior round is carried into `scenario_results` with the prior
//! verdict (or 'passing' on first inheritance). Disrupted scenarios surface
//! as 'failing' so the user must re-evaluate.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, parsed, s, s_opt, ts, ts_opt};
use rusqlite::{params, Connection, Row};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};
use sdi_core::round::{DisruptionPolicy, InFlightPolicy, Round, RoundMode, RoundStatus};
use sdi_core::scenario::ScenarioResult;

fn row_to_round(row: &Row<'_>) -> rusqlite::Result<Round> {
    Ok(Round {
        id: Id::from(s(row, 0)?),
        plan_id: Id::from(s(row, 1)?),
        short_code: s(row, 2)?,
        round_number: row.get(3)?,
        mode: parsed::<RoundMode>(row, 4)?,
        in_flight_policy: parsed::<InFlightPolicy>(row, 5)?,
        disruption_policy: parsed::<DisruptionPolicy>(row, 6)?,
        status: parsed::<RoundStatus>(row, 7)?,
        activated_at: ts_opt(row, 8)?,
        completed_at: ts_opt(row, 9)?,
        created_at: ts(row, 10)?,
        updated_at: ts(row, 11)?,
    })
}

const COLS: &str = "id, plan_id, short_code, round_number, mode, in_flight_policy, disruption_policy, status, activated_at, completed_at, created_at, updated_at";

pub fn insert(conn: &Connection, round: &Round) -> DomainResult<()> {
    conn.execute(
        &format!("INSERT INTO rounds({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"),
        params![
            round.id.as_str(),
            round.plan_id.as_str(),
            round.short_code,
            round.round_number,
            round.mode.to_string(),
            round.in_flight_policy.to_string(),
            round.disruption_policy.to_string(),
            round.status.to_string(),
            round.activated_at.map(fmt_ts),
            round.completed_at.map(fmt_ts),
            fmt_ts(round.created_at),
            fmt_ts(round.updated_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get(conn: &Connection, id: &Id) -> DomainResult<Round> {
    conn.query_row(
        &format!("SELECT {COLS} FROM rounds WHERE id = ?1"),
        [id.as_str()],
        row_to_round,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

pub fn list_by_plan(conn: &Connection, plan_id: &Id) -> DomainResult<Vec<Round>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM rounds WHERE plan_id = ?1 ORDER BY round_number"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([plan_id.as_str()], row_to_round)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn find_active_for_plan(conn: &Connection, plan_id: &Id) -> DomainResult<Option<Round>> {
    let r = conn.query_row(
        &format!("SELECT {COLS} FROM rounds WHERE plan_id = ?1 AND status = 'active'"),
        [plan_id.as_str()],
        row_to_round,
    );
    match r {
        Ok(p) => Ok(Some(p)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(map_sqlite_err(e)),
    }
}

pub fn latest_completed_for_plan(conn: &Connection, plan_id: &Id) -> DomainResult<Option<Round>> {
    let r = conn.query_row(
        &format!(
            "SELECT {COLS} FROM rounds WHERE plan_id = ?1 AND status = 'completed' \
             ORDER BY round_number DESC LIMIT 1"
        ),
        [plan_id.as_str()],
        row_to_round,
    );
    match r {
        Ok(p) => Ok(Some(p)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(map_sqlite_err(e)),
    }
}

pub fn next_round_number(conn: &Connection, plan_id: &Id) -> DomainResult<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(round_number), 0) + 1 FROM rounds WHERE plan_id = ?1",
        [plan_id.as_str()],
        |r| r.get(0),
    )
    .map_err(map_sqlite_err)
}

pub fn set_status(
    conn: &Connection,
    id: &Id,
    status: RoundStatus,
    activated_at: Option<chrono::DateTime<chrono::Utc>>,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
) -> DomainResult<()> {
    let n = conn
        .execute(
            "UPDATE rounds SET status = ?1, activated_at = COALESCE(?2, activated_at), \
             completed_at = COALESCE(?3, completed_at), updated_at = ?4 WHERE id = ?5",
            params![
                status.to_string(),
                activated_at.map(fmt_ts),
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

// ---------------------------------------------------------------------------
// scenario_results — per-round verdicts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ScenarioResultRow {
    pub round_id: Id,
    pub scenario_id: Id,
    pub result: ScenarioResult,
    pub note: Option<String>,
    pub evidence_ref: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub fn upsert_result(
    conn: &Connection,
    round_id: &Id,
    scenario_id: &Id,
    result: ScenarioResult,
    note: Option<&str>,
    evidence_ref: Option<&str>,
) -> DomainResult<()> {
    conn.execute(
        "INSERT INTO scenario_results(round_id, scenario_id, result, note, evidence_ref, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(round_id, scenario_id) DO UPDATE SET \
            result = excluded.result, note = excluded.note, \
            evidence_ref = excluded.evidence_ref, updated_at = excluded.updated_at",
        params![
            round_id.as_str(),
            scenario_id.as_str(),
            result.to_string(),
            note,
            evidence_ref,
            fmt_ts(now()),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn list_results(conn: &Connection, round_id: &Id) -> DomainResult<Vec<ScenarioResultRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT round_id, scenario_id, result, note, evidence_ref, updated_at \
             FROM scenario_results WHERE round_id = ?1 ORDER BY scenario_id",
        )
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([round_id.as_str()], |r| {
            Ok(ScenarioResultRow {
                round_id: Id::from(s(r, 0)?),
                scenario_id: Id::from(s(r, 1)?),
                result: parsed::<ScenarioResult>(r, 2)?,
                note: s_opt(r, 3)?,
                evidence_ref: s_opt(r, 4)?,
                updated_at: ts(r, 5)?,
            })
        })
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// PRD §6.3 R2+ auto-regression population.
///
/// Carries forward only scenarios that actually received a verdict in the
/// previous round — `passing`, `failing`, `impacted`, or `retired` all flow
/// through verbatim. Scenarios `confirmed` on the plan but with NO prior
/// verdict (i.e. added between rounds, or otherwise unevaluated) are
/// intentionally left absent from the new round's queue: they have never been
/// tested, so silently auto-marking them `passing` would be a false-positive
/// regression. The LLM/Task pipeline must produce evidence for them via
/// `/rounds/:id/results` like any other new development.
///
/// Disrupted scenarios pre-marked `impacted` by the caller before carry are
/// preserved (the existing-row check below skips overwriting them).
pub fn carry_over_results(
    conn: &Connection,
    new_round: &Round,
    prev_round_id: &Id,
) -> DomainResult<usize> {
    let mut stmt = conn
        .prepare(
            "SELECT sr.scenario_id, sr.result \
             FROM scenario_results sr \
             JOIN scenarios s ON s.id = sr.scenario_id \
             WHERE sr.round_id = ?1 AND s.plan_id = ?2 AND s.status = 'confirmed'",
        )
        .map_err(map_sqlite_err)?;
    let pairs: Vec<(String, String)> = stmt
        .query_map(
            params![prev_round_id.as_str(), new_round.plan_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(map_sqlite_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite_err)?;
    let mut inserted = 0usize;
    for (scenario_id, prev_result) in pairs {
        let existing: Option<String> = conn
            .query_row(
                "SELECT result FROM scenario_results WHERE round_id = ?1 AND scenario_id = ?2",
                params![new_round.id.as_str(), &scenario_id],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(map_sqlite_err(other)),
            })?;
        if existing.is_some() {
            continue;
        }
        let carry: ScenarioResult = match prev_result.as_str() {
            "retired" => ScenarioResult::Retired,
            "failing" => ScenarioResult::Failing,
            "impacted" => ScenarioResult::Impacted,
            "passing" => ScenarioResult::Passing,
            other => {
                return Err(DomainError::Validation(format!(
                    "unknown carry-over verdict `{other}` for scenario {scenario_id}",
                )));
            }
        };
        upsert_result(
            conn,
            &new_round.id,
            &Id::from(scenario_id),
            carry,
            None,
            None,
        )?;
        inserted += 1;
    }
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{plan as plan_repo, project as proj_repo, scenario as scn_repo};
    use crate::{ensure_schema, open_pool};
    use sdi_core::ids::IdKind;
    use sdi_core::plan::{Plan, PlanStatus};
    use sdi_core::project::Project;
    use sdi_core::scenario::{Scenario, ScenarioStatus};

    fn fixture() -> (r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, Id, Id) {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-round-{}-{}.db",
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
            status: PlanStatus::Active,
            version: 0,
            approved_at: Some(now()),
            completed_at: None,
            created_at: now(),
            updated_at: now(),
        };
        plan_repo::insert(&conn, &plan).unwrap();
        let scn = Scenario {
            id: Id::new(IdKind::Scenario),
            plan_id: plan.id.clone(),
            short_code: format!("SCN-{}", ulid::Ulid::new()),
            given: "g".into(),
            when_clause: "w".into(),
            then_clause: "t".into(),
            tags: vec![],
            origin_round_id: None,
            status: ScenarioStatus::Confirmed,
            depends_on: vec![],
            produced_by: None,
            verified_by: None,
            claimed_resources_json: "[]".into(),
            claim_status: sdi_core::pattern::ClaimStatus::None,
            produced_via_pattern_id: None,
            created_at: now(),
            updated_at: now(),
        };
        scn_repo::insert(&conn, &scn).unwrap();
        (pool, plan.id, scn.id)
    }

    fn mk_round(plan_id: Id, num: i64, status: RoundStatus) -> Round {
        Round {
            id: Id::new(IdKind::Round),
            plan_id,
            short_code: format!("R-{}", ulid::Ulid::new()),
            round_number: num,
            mode: RoundMode::StrictRegression,
            in_flight_policy: InFlightPolicy::Pause,
            disruption_policy: DisruptionPolicy::NeedsReview,
            status,
            activated_at: if status == RoundStatus::Planning {
                None
            } else {
                Some(now())
            },
            completed_at: if status == RoundStatus::Completed {
                Some(now())
            } else {
                None
            },
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn carry_over_passes_default() {
        let (pool, plan_id, scn_id) = fixture();
        let conn = pool.get().unwrap();
        let r1 = mk_round(plan_id.clone(), 1, RoundStatus::Completed);
        let r2 = mk_round(plan_id.clone(), 2, RoundStatus::Active);
        insert(&conn, &r1).unwrap();
        upsert_result(&conn, &r1.id, &scn_id, ScenarioResult::Passing, None, None).unwrap();
        insert(&conn, &r2).unwrap();
        let n = carry_over_results(&conn, &r2, &r1.id).unwrap();
        assert_eq!(n, 1);
        let rows = list_results(&conn, &r2.id).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].result, ScenarioResult::Passing);
    }

    #[test]
    fn carry_over_preserves_failing() {
        let (pool, plan_id, scn_id) = fixture();
        let conn = pool.get().unwrap();
        let r1 = mk_round(plan_id.clone(), 1, RoundStatus::Completed);
        let r2 = mk_round(plan_id.clone(), 2, RoundStatus::Active);
        insert(&conn, &r1).unwrap();
        upsert_result(&conn, &r1.id, &scn_id, ScenarioResult::Failing, None, None).unwrap();
        insert(&conn, &r2).unwrap();
        carry_over_results(&conn, &r2, &r1.id).unwrap();
        let rows = list_results(&conn, &r2.id).unwrap();
        assert_eq!(rows[0].result, ScenarioResult::Failing);
    }

    #[test]
    fn second_active_round_per_plan_conflicts() {
        let (pool, plan_id, _scn_id) = fixture();
        let conn = pool.get().unwrap();
        let r1 = mk_round(plan_id.clone(), 1, RoundStatus::Active);
        let r2 = mk_round(plan_id.clone(), 2, RoundStatus::Active);
        insert(&conn, &r1).unwrap();
        let err = insert(&conn, &r2).unwrap_err();
        assert!(matches!(err, DomainError::Conflict(_)));
    }
}
