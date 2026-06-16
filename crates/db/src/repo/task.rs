//! Task repository — runtime work items (D3). Insert/list/update plus the
//! `complete_with_evidence` path that enforces PRD §6.6 evidence-required at
//! the SQL boundary (in addition to domain validation in `Task::check_transition`).

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, json, parsed, s, s_opt, ts, ts_opt};
use rusqlite::{params, Connection, Row};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};
use sdi_core::task::{Task, TaskEvidence, TaskStatus};

fn row_to_task(row: &Row<'_>) -> rusqlite::Result<Task> {
    let evidence_raw: Option<String> = row.get(8)?;
    let evidence = match evidence_raw {
        Some(raw) => Some(serde_json::from_str::<TaskEvidence>(&raw).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
        })?),
        None => None,
    };
    Ok(Task {
        id: Id::from(s(row, 0)?),
        round_id: Id::from(s(row, 1)?),
        plan_id: Id::from(s(row, 2)?),
        short_code: s(row, 3)?,
        description: s(row, 4)?,
        status: parsed::<TaskStatus>(row, 5)?,
        parent_scenario_ids: json::<Vec<Id>>(row, 6)?,
        parent_requirement_ids: json::<Vec<Id>>(row, 7)?,
        evidence,
        evidence_at: ts_opt(row, 9)?,
        created_at: ts(row, 10)?,
        updated_at: ts(row, 11)?,
        produced_via_pattern_id: s_opt(row, 12)?,
    })
}

const COLS: &str = "id, round_id, plan_id, short_code, description, status, parent_scenario_ids, parent_requirement_ids, evidence, evidence_at, created_at, updated_at, produced_via_pattern_id";

pub fn insert(conn: &Connection, task: &Task) -> DomainResult<()> {
    let parent_scn = serde_json::to_string(&task.parent_scenario_ids)
        .map_err(|e| DomainError::Validation(format!("encode parent_scenario_ids: {e}")))?;
    let parent_req = serde_json::to_string(&task.parent_requirement_ids)
        .map_err(|e| DomainError::Validation(format!("encode parent_requirement_ids: {e}")))?;
    let evidence = match &task.evidence {
        Some(e) => Some(
            serde_json::to_string(e)
                .map_err(|err| DomainError::Validation(format!("encode evidence: {err}")))?,
        ),
        None => None,
    };
    conn.execute(
        &format!("INSERT INTO tasks({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)"),
        params![
            task.id.as_str(),
            task.round_id.as_str(),
            task.plan_id.as_str(),
            task.short_code,
            task.description,
            task.status.to_string(),
            parent_scn,
            parent_req,
            evidence,
            task.evidence_at.map(fmt_ts),
            fmt_ts(task.created_at),
            fmt_ts(task.updated_at),
            task.produced_via_pattern_id,
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get(conn: &Connection, id: &Id) -> DomainResult<Task> {
    conn.query_row(
        &format!("SELECT {COLS} FROM tasks WHERE id = ?1"),
        [id.as_str()],
        row_to_task,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

pub fn list_by_round(conn: &Connection, round_id: &Id) -> DomainResult<Vec<Task>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM tasks WHERE round_id = ?1 ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([round_id.as_str()], row_to_task)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn list_in_flight_for_plan(conn: &Connection, plan_id: &Id) -> DomainResult<Vec<Task>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM tasks t JOIN rounds r ON r.id = t.round_id \
             WHERE r.plan_id = ?1 AND t.status = 'in_progress' ORDER BY t.created_at",
            COLS.split(", ")
                .map(|c| format!("t.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([plan_id.as_str()], row_to_task)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

// All non-terminal tasks across a plan's rounds: todo + in_progress + blocked.
// Powers the Backlog view, which mirrors Clawket's backlog semantics (everything
// not yet done/cancelled). Distinct from `list_in_flight_for_plan`, which
// returns only in_progress for round disruption-policy decisions.
pub fn list_backlog_for_plan(conn: &Connection, plan_id: &Id) -> DomainResult<Vec<Task>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM tasks t JOIN rounds r ON r.id = t.round_id \
             WHERE r.plan_id = ?1 AND t.status IN ('todo','in_progress','blocked') \
             ORDER BY t.created_at",
            COLS.split(", ")
                .map(|c| format!("t.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([plan_id.as_str()], row_to_task)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn set_status(conn: &Connection, id: &Id, status: TaskStatus) -> DomainResult<()> {
    let n = conn
        .execute(
            "UPDATE tasks SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status.to_string(), fmt_ts(now()), id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

/// Atomic done transition: writes evidence + flips status + stamps evidence_at.
/// Domain layer (`Task::check_transition`) MUST be invoked by the caller before
/// this; we re-validate here as a defense-in-depth SQL gate.
pub fn complete_with_evidence(
    conn: &Connection,
    id: &Id,
    evidence: &TaskEvidence,
) -> DomainResult<()> {
    evidence.validate_for_done()?;
    let raw = serde_json::to_string(evidence)
        .map_err(|e| DomainError::Validation(format!("encode evidence: {e}")))?;
    let now_s = fmt_ts(now());
    let n = conn
        .execute(
            "UPDATE tasks SET evidence = ?1, evidence_at = ?2, status = 'done', \
             completed_at = ?2, updated_at = ?2 WHERE id = ?3",
            params![raw, now_s, id.as_str()],
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
    use crate::repo::{
        plan as plan_repo, project as proj_repo, round as round_repo, scenario as scn_repo,
    };
    use crate::{ensure_schema, open_pool};
    use sdi_core::ids::IdKind;
    use sdi_core::plan::{Plan, PlanStatus};
    use sdi_core::project::Project;
    use sdi_core::round::{DisruptionPolicy, InFlightPolicy, Round, RoundMode, RoundStatus};
    use sdi_core::scenario::{Scenario, ScenarioResult, ScenarioStatus};
    use sdi_core::task::{ScenarioEvidence as Se, TaskEvidence};

    fn fixture() -> (
        r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
        Id, /* round */
        Id, /* plan */
        Id, /* scenario */
    ) {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-task-{}-{}.db",
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
            status: PlanStatus::Active,
            version: 0,
            produced_via_pattern_id: None,
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
            retired_at: None,
            created_at: now(),
            updated_at: now(),
        };
        scn_repo::insert(&conn, &scn).unwrap();
        let rnd = Round {
            id: Id::new(IdKind::Round),
            plan_id: plan.id.clone(),
            short_code: format!("R-{}", ulid::Ulid::new()),
            round_number: 1,
            mode: RoundMode::StrictRegression,
            in_flight_policy: InFlightPolicy::Pause,
            disruption_policy: DisruptionPolicy::NeedsReview,
            status: RoundStatus::Active,
            produced_via_pattern_id: None,
            activated_at: Some(now()),
            completed_at: None,
            created_at: now(),
            updated_at: now(),
        };
        round_repo::insert(&conn, &rnd).unwrap();
        (pool, rnd.id, plan.id, scn.id)
    }

    fn mk_task(round_id: Id, plan_id: Id, parent_scn: Vec<Id>) -> Task {
        Task {
            id: Id::new(IdKind::Task),
            round_id,
            plan_id,
            short_code: format!("TASK-{}", ulid::Ulid::new()),
            description: "wire CLI".into(),
            status: TaskStatus::Todo,
            parent_scenario_ids: parent_scn,
            parent_requirement_ids: vec![],
            evidence: None,
            produced_via_pattern_id: None,
            evidence_at: None,
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn roundtrip_insert_get_list() {
        let (pool, round_id, plan_id, scn_id) = fixture();
        let conn = pool.get().unwrap();
        let t = mk_task(round_id.clone(), plan_id, vec![scn_id.clone()]);
        insert(&conn, &t).unwrap();
        let got = get(&conn, &t.id).unwrap();
        assert_eq!(got.id, t.id);
        assert_eq!(got.parent_scenario_ids, vec![scn_id]);
        let listed = list_by_round(&conn, &round_id).unwrap();
        assert_eq!(listed.len(), 1);
    }

    #[test]
    fn complete_with_evidence_writes_and_flips_status() {
        let (pool, round_id, plan_id, scn_id) = fixture();
        let conn = pool.get().unwrap();
        let mut t = mk_task(round_id, plan_id, vec![scn_id.clone()]);
        t.status = TaskStatus::InProgress;
        insert(&conn, &t).unwrap();
        let ev = TaskEvidence {
            scenarios: vec![Se {
                scenario_id: scn_id,
                result: ScenarioResult::Passing,
                evidence_ref: "src/lib.rs:42".into(),
                note: None,
            }],
            summary: Some("done".into()),
            extras: Default::default(),
        };
        complete_with_evidence(&conn, &t.id, &ev).unwrap();
        let got = get(&conn, &t.id).unwrap();
        assert_eq!(got.status, TaskStatus::Done);
        assert!(got.evidence.is_some());
        assert!(got.evidence_at.is_some());
    }

    #[test]
    fn complete_without_evidence_rejected() {
        let (pool, round_id, plan_id, _scn_id) = fixture();
        let conn = pool.get().unwrap();
        let mut t = mk_task(round_id, plan_id, vec![]);
        t.status = TaskStatus::InProgress;
        insert(&conn, &t).unwrap();
        let empty = TaskEvidence::default();
        let err = complete_with_evidence(&conn, &t.id, &empty).unwrap_err();
        assert!(matches!(err, DomainError::EvidenceRequired));
    }
}
