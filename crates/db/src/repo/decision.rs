//! Decision repository — append-only ADR log (D12). Once written, the row's
//! body never changes; supersession is encoded by inserting a new row whose
//! `supersedes_id` points at the prior decision and flipping the predecessor
//! to `superseded`.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, parsed, s, s_opt, ts, ts_opt};
use rusqlite::{params, Connection, Row};
use sdi_core::decision::{Decision, DecisionKind, DecisionStatus};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};

fn row_to_decision(row: &Row<'_>) -> rusqlite::Result<Decision> {
    Ok(Decision {
        id: Id::from(s(row, 0)?),
        plan_id: Id::from(s(row, 1)?),
        short_code: s(row, 2)?,
        title: s(row, 3)?,
        body: s(row, 4)?,
        status: parsed::<DecisionStatus>(row, 5)?,
        supersedes_id: s_opt(row, 6)?.map(Id::from),
        kind: parsed::<DecisionKind>(row, 7)?,
        proposal_id: s_opt(row, 8)?.map(Id::from),
        agent_name: s_opt(row, 9)?,
        escalated_at: ts_opt(row, 10)?,
        produced_via_pattern_id: s_opt(row, 11)?,
        reversal_plan: s_opt(row, 12)?,
        blast_radius_score: row.get::<_, i64>(13)? as i32,
        reversal_of: s_opt(row, 14)?.map(Id::from),
        created_at: ts(row, 15)?,
    })
}

const COLS: &str = "id, plan_id, short_code, title, body, status, supersedes_id, \
                    kind, proposal_id, agent_name, escalated_at, \
                    produced_via_pattern_id, reversal_plan, blast_radius_score, \
                    reversal_of, created_at";

pub fn insert(conn: &Connection, decision: &Decision) -> DomainResult<()> {
    // D28 — blast_radius_score in 0..=10.
    Decision::validate_blast_radius_score(decision.blast_radius_score)?;
    // D28 — a row carrying `reversal_of` is a unilateral rollback
    // (reversal-runner specialist). It is structurally a consensus row for the
    // audit log but it is NOT an M3-gated multi-party consensus: there was no
    // proposal/critique cycle, just an append-only undo. Skip the M3 gate for
    // these rows and require `reversal_of` to resolve.
    let is_rollback = decision.reversal_of.is_some();
    if is_rollback {
        if !matches!(decision.kind, DecisionKind::Consensus) {
            return Err(DomainError::Validation(
                "reversal_of is only valid on consensus decisions".into(),
            ));
        }
        if decision.proposal_id.is_some() {
            return Err(DomainError::Validation(
                "rollback decisions must not carry proposal_id".into(),
            ));
        }
    } else if !matches!(decision.kind, DecisionKind::Proposal) {
        // D20 — enforce M3 ordering for non-proposal, non-rollback stages.
        let existing = list_kinds_by_proposal(
            conn,
            decision
                .proposal_id
                .as_ref()
                .ok_or_else(|| DomainError::Validation(format!(
                    "{} decision requires proposal_id",
                    decision.kind
                )))?,
        )?;
        Decision::validate_stage_transition(
            decision.kind,
            &existing,
            decision.proposal_id.as_ref(),
        )?;
    } else if decision.proposal_id.is_some() {
        return Err(DomainError::Validation(
            "proposal decision must not carry proposal_id".into(),
        ));
    }
    conn.execute(
        &format!(
            "INSERT INTO decisions({COLS}) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)"
        ),
        params![
            decision.id.as_str(),
            decision.plan_id.as_str(),
            decision.short_code,
            decision.title,
            decision.body,
            decision.status.to_string(),
            decision.supersedes_id.as_ref().map(|i| i.as_str().to_string()),
            decision.kind.to_string(),
            decision.proposal_id.as_ref().map(|i| i.as_str().to_string()),
            decision.agent_name,
            decision.escalated_at.map(fmt_ts),
            decision.produced_via_pattern_id,
            decision.reversal_plan,
            decision.blast_radius_score as i64,
            decision.reversal_of.as_ref().map(|i| i.as_str().to_string()),
            fmt_ts(decision.created_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    if let Some(prev) = &decision.supersedes_id {
        conn.execute(
            "UPDATE decisions SET status = 'superseded' WHERE id = ?1",
            params![prev.as_str()],
        )
        .map_err(map_sqlite_err)?;
    }
    Ok(())
}

/// Returns every Decision.kind already recorded against a proposal, in
/// insertion order. Used by `insert` to enforce M3 ordering.
pub fn list_kinds_by_proposal(
    conn: &Connection,
    proposal_id: &Id,
) -> DomainResult<Vec<DecisionKind>> {
    let mut stmt = conn
        .prepare("SELECT kind FROM decisions WHERE proposal_id = ?1 ORDER BY created_at")
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([proposal_id.as_str()], |r| {
            let raw: String = r.get(0)?;
            Ok(raw)
        })
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        let raw = r.map_err(map_sqlite_err)?;
        let kind: DecisionKind = raw
            .parse()
            .map_err(|e: DomainError| map_sqlite_err(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?;
        out.push(kind);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &Id) -> DomainResult<Decision> {
    conn.query_row(
        &format!("SELECT {COLS} FROM decisions WHERE id = ?1"),
        [id.as_str()],
        row_to_decision,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

pub fn list_by_plan(conn: &Connection, plan_id: &Id) -> DomainResult<Vec<Decision>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM decisions WHERE plan_id = ?1 ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([plan_id.as_str()], row_to_decision)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// Refuse to mutate body; only allowed transitions are proposed→accepted and
/// either of those → superseded (handled by chain insert above).
pub fn set_status(conn: &Connection, id: &Id, status: DecisionStatus) -> DomainResult<()> {
    let n = conn
        .execute(
            "UPDATE decisions SET status = ?1 WHERE id = ?2",
            params![status.to_string(), id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

/// Marks all open (proposed/accepted) decisions on a plan as superseded —
/// used when migrating plans or retiring scope. Returns affected count.
pub fn supersede_open(conn: &Connection, plan_id: &Id) -> DomainResult<usize> {
    let _ = now();
    conn.execute(
        "UPDATE decisions SET status = 'superseded' \
         WHERE plan_id = ?1 AND status IN ('proposed','accepted')",
        params![plan_id.as_str()],
    )
    .map_err(map_sqlite_err)
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
            "sdi-repo-dec-{}-{}.db",
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
            project_id: proj.id.clone(),
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

    fn mk(plan_id: Id, supersedes: Option<Id>) -> Decision {
        Decision {
            id: Id::new(IdKind::Decision),
            plan_id,
            short_code: format!("DEC-{}", ulid::Ulid::new()),
            title: "t".into(),
            body: "b".into(),
            status: DecisionStatus::Accepted,
            supersedes_id: supersedes,
            kind: DecisionKind::Proposal,
            proposal_id: None,
            agent_name: None,
            escalated_at: None,
            produced_via_pattern_id: None,
            reversal_plan: None,
            blast_radius_score: 5,
            reversal_of: None,
            created_at: now(),
        }
    }

    #[test]
    fn supersession_chains_correctly() {
        let (pool, plan_id) = fixture();
        let conn = pool.get().unwrap();
        let a = mk(plan_id.clone(), None);
        insert(&conn, &a).unwrap();
        let b = mk(plan_id.clone(), Some(a.id.clone()));
        insert(&conn, &b).unwrap();
        let a2 = get(&conn, &a.id).unwrap();
        let b2 = get(&conn, &b.id).unwrap();
        assert_eq!(a2.status, DecisionStatus::Superseded);
        assert_eq!(b2.status, DecisionStatus::Accepted);
        assert_eq!(b2.supersedes_id.unwrap(), a.id);
    }

    #[test]
    fn v05_columns_round_trip() {
        let (pool, plan_id) = fixture();
        let conn = pool.get().unwrap();
        // Seed a pattern row so the FK on produced_via_pattern_id resolves.
        let pat = crate::repo::pattern::PatternRow {
            id: format!("CP-{}", ulid::Ulid::new()),
            short_code: "CP-dec-rt".into(),
            plan_id: plan_id.as_str().into(),
            kind: "direct".into(),
            applies_to: "decision".into(),
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
        let mut d = mk(plan_id, None);
        d.produced_via_pattern_id = Some(pat.id.clone());
        d.reversal_plan = Some(r#"{"type":"git_revert","sha":"abc123"}"#.into());
        d.blast_radius_score = 3;
        insert(&conn, &d).unwrap();
        let fetched = get(&conn, &d.id).unwrap();
        assert_eq!(fetched.produced_via_pattern_id.as_deref(), Some(pat.id.as_str()));
        assert_eq!(
            fetched.reversal_plan.as_deref(),
            Some(r#"{"type":"git_revert","sha":"abc123"}"#)
        );
        assert_eq!(fetched.blast_radius_score, 3);
        assert!(fetched.reversal_of.is_none());
    }

    #[test]
    fn out_of_range_blast_radius_rejected_at_insert() {
        let (pool, plan_id) = fixture();
        let conn = pool.get().unwrap();
        let mut d = mk(plan_id, None);
        d.blast_radius_score = 11;
        let err = insert(&conn, &d).unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }
}
