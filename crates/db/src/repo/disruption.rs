//! Disruption review repository (PRD §6 #4).
//!
//! `has_pending` is the function the round-activation gate calls; everything
//! else is the maintenance surface (insert / list / resolve) that the
//! HTTP router proxies.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, json, parsed, s, s_opt, ts, ts_opt};
use rusqlite::{params, Connection, Row};
use sdi_core::disruption::{
    DisruptionResolution, DisruptionReview, DisruptionReviewStatus, DisruptionSource,
};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};

fn row_to_review(row: &Row<'_>) -> rusqlite::Result<DisruptionReview> {
    let resolution_raw: Option<String> = row.get(6)?;
    let resolution = match resolution_raw {
        Some(r) => Some(r.parse::<DisruptionResolution>().map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e))
        })?),
        None => None,
    };
    Ok(DisruptionReview {
        id: Id::from(s(row, 0)?),
        plan_id: Id::from(s(row, 1)?),
        source_kind: parsed::<DisruptionSource>(row, 2)?,
        source_id: Id::from(s(row, 3)?),
        impacted_scenario_ids: json::<Vec<Id>>(row, 4)?,
        status: parsed::<DisruptionReviewStatus>(row, 5)?,
        resolution,
        note: s_opt(row, 7)?,
        created_at: ts(row, 8)?,
        resolved_at: ts_opt(row, 9)?,
    })
}

const COLS: &str = "id, plan_id, source_kind, source_id, impacted_scenario_ids, status, \
                    resolution, note, created_at, resolved_at";

pub fn insert(conn: &Connection, review: &DisruptionReview) -> DomainResult<()> {
    review.validate_create()?;
    let scenarios = serde_json::to_string(&review.impacted_scenario_ids)
        .map_err(|e| DomainError::Validation(format!("encode impacted_scenario_ids: {e}")))?;
    conn.execute(
        &format!("INSERT INTO disruption_reviews({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"),
        params![
            review.id.as_str(),
            review.plan_id.as_str(),
            review.source_kind.to_string(),
            review.source_id.as_str(),
            scenarios,
            review.status.to_string(),
            review.resolution.as_ref().map(|r| r.to_string()),
            review.note,
            fmt_ts(review.created_at),
            review.resolved_at.map(fmt_ts),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get(conn: &Connection, id: &Id) -> DomainResult<DisruptionReview> {
    conn.query_row(
        &format!("SELECT {COLS} FROM disruption_reviews WHERE id = ?1"),
        [id.as_str()],
        row_to_review,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

pub fn list_by_plan(
    conn: &Connection,
    plan_id: &Id,
    status: Option<DisruptionReviewStatus>,
) -> DomainResult<Vec<DisruptionReview>> {
    let (sql, args): (String, Vec<String>) = match status {
        Some(s) => (
            format!(
                "SELECT {COLS} FROM disruption_reviews \
                 WHERE plan_id = ?1 AND status = ?2 ORDER BY created_at"
            ),
            vec![plan_id.as_str().to_string(), s.to_string()],
        ),
        None => (
            format!(
                "SELECT {COLS} FROM disruption_reviews \
                 WHERE plan_id = ?1 ORDER BY created_at"
            ),
            vec![plan_id.as_str().to_string()],
        ),
    };
    let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(args.iter()), row_to_review)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// Round activation gate: returns true iff any review on this plan is still
/// pending. Caller maps `true → DomainError::DisruptionPending`.
pub fn has_pending(conn: &Connection, plan_id: &Id) -> DomainResult<bool> {
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM disruption_reviews \
             WHERE plan_id = ?1 AND status = 'pending'",
            [plan_id.as_str()],
            |r| r.get(0),
        )
        .map_err(map_sqlite_err)?;
    Ok(n > 0)
}

pub fn resolve(
    conn: &Connection,
    id: &Id,
    resolution: Option<DisruptionResolution>,
    note: Option<&str>,
) -> DomainResult<()> {
    let existing = get(conn, id)?;
    if matches!(existing.status, DisruptionReviewStatus::Resolved) {
        return Err(DomainError::InvalidTransition {
            entity: "disruption_review".into(),
            from: "resolved".into(),
            to: "resolved".into(),
        });
    }
    let n = conn
        .execute(
            "UPDATE disruption_reviews \
             SET status = 'resolved', resolution = ?1, note = ?2, resolved_at = ?3 \
             WHERE id = ?4",
            params![
                resolution.map(|r| r.to_string()),
                note,
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
    use crate::repo::{plan as plan_repo, project as proj_repo, scenario as scn_repo};
    use crate::{ensure_schema, open_pool};
    use sdi_core::ids::IdKind;
    use sdi_core::plan::{Plan, PlanStatus};
    use sdi_core::project::Project;
    use sdi_core::scenario::{Scenario, ScenarioStatus};

    fn fixture() -> (
        r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
        Id, /* plan */
        Id, /* scenario */
    ) {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-drv-{}-{}.db",
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
            created_at: now(),
            updated_at: now(),
        };
        scn_repo::insert(&conn, &scn).unwrap();
        (pool, plan.id, scn.id)
    }

    fn mk(plan_id: Id, scenario_ids: Vec<Id>, source_id: Id) -> DisruptionReview {
        DisruptionReview {
            id: Id::new(IdKind::DisruptionReview),
            plan_id,
            source_kind: DisruptionSource::Scenario,
            source_id,
            impacted_scenario_ids: scenario_ids,
            status: DisruptionReviewStatus::Pending,
            resolution: None,
            note: None,
            created_at: now(),
            resolved_at: None,
        }
    }

    #[test]
    fn insert_rejects_empty_impacted() {
        let (pool, plan_id, _scn) = fixture();
        let conn = pool.get().unwrap();
        let review = mk(plan_id, vec![], Id::new(IdKind::Scenario));
        let err = insert(&conn, &review).unwrap_err();
        assert!(
            matches!(err, DomainError::Validation(_)),
            "expected Validation, got {err:?}"
        );
    }

    #[test]
    fn pending_blocks_then_resolves() {
        let (pool, plan_id, scn_id) = fixture();
        let conn = pool.get().unwrap();
        assert!(!has_pending(&conn, &plan_id).unwrap());

        let review = mk(plan_id.clone(), vec![scn_id], Id::new(IdKind::Scenario));
        insert(&conn, &review).unwrap();
        assert!(has_pending(&conn, &plan_id).unwrap());

        resolve(
            &conn,
            &review.id,
            Some(DisruptionResolution::Retire),
            Some("retired by reviewer"),
        )
        .unwrap();
        assert!(!has_pending(&conn, &plan_id).unwrap());

        let after = get(&conn, &review.id).unwrap();
        assert_eq!(after.status, DisruptionReviewStatus::Resolved);
        assert_eq!(after.resolution, Some(DisruptionResolution::Retire));
        assert_eq!(after.note.as_deref(), Some("retired by reviewer"));
        assert!(after.resolved_at.is_some());
    }

    #[test]
    fn double_resolve_rejected() {
        let (pool, plan_id, scn_id) = fixture();
        let conn = pool.get().unwrap();
        let review = mk(plan_id, vec![scn_id], Id::new(IdKind::Scenario));
        insert(&conn, &review).unwrap();
        resolve(&conn, &review.id, None, None).unwrap();
        let err = resolve(&conn, &review.id, None, None).unwrap_err();
        assert!(matches!(err, DomainError::InvalidTransition { .. }));
    }

    #[test]
    fn list_filters_by_status() {
        let (pool, plan_id, scn_id) = fixture();
        let conn = pool.get().unwrap();
        let r1 = mk(
            plan_id.clone(),
            vec![scn_id.clone()],
            Id::new(IdKind::Scenario),
        );
        let r2 = mk(plan_id.clone(), vec![scn_id], Id::new(IdKind::Requirement));
        insert(&conn, &r1).unwrap();
        insert(&conn, &r2).unwrap();
        resolve(&conn, &r1.id, Some(DisruptionResolution::Keep), None).unwrap();

        let pending =
            list_by_plan(&conn, &plan_id, Some(DisruptionReviewStatus::Pending)).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, r2.id);

        let all = list_by_plan(&conn, &plan_id, None).unwrap();
        assert_eq!(all.len(), 2);
    }
}
