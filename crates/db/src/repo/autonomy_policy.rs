//! AutonomyPolicy repository (D14). Upsert-on-scope semantics — the unique
//! index `uniq_autonomy_scope` enforces one row per
//! (project, scope_kind, plan_id, decision_kind, pattern_kind) tuple (D25
//! widened the tuple to include pattern_kind), so `set` rewrites the existing
//! row instead of inserting a duplicate.
//!
//! v0.5 (D24/D25/D28/D29) added `pattern_kind`, `l5_threshold`,
//! `pattern_depth_cap`, `plan_single_session_lock`, `external_surface`,
//! `timeout_ms`, `forced`. The INSERT now writes the full v0.5 column set and
//! ON CONFLICT updates everything except `id`/`project_id`/`scope_kind`/
//! `plan_id`/`decision_kind`/`pattern_kind`/`created_at` (those identify the
//! row or are immutable).

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, parsed, s, s_opt, ts};
use rusqlite::{params, Connection, Row};
use sdi_core::autonomy_policy::{AutonomyMode, AutonomyPolicy, AutonomyScopeKind};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::Id;

const COLS: &str = "id, project_id, plan_id, scope_kind, decision_kind, pattern_kind, mode, \
                    l5_threshold, pattern_depth_cap, plan_single_session_lock, \
                    external_surface, timeout_ms, forced, \
                    set_at, set_by, reason, created_at, updated_at";

fn row_to_policy(row: &Row<'_>) -> rusqlite::Result<AutonomyPolicy> {
    let bool_col = |idx: usize| -> rusqlite::Result<bool> {
        let v: i64 = row.get(idx)?;
        Ok(v != 0)
    };
    Ok(AutonomyPolicy {
        id: Id::from(s(row, 0)?),
        project_id: Id::from(s(row, 1)?),
        plan_id: s_opt(row, 2)?.map(Id::from),
        scope_kind: parsed::<AutonomyScopeKind>(row, 3)?,
        decision_kind: s_opt(row, 4)?,
        pattern_kind: s_opt(row, 5)?,
        mode: parsed::<AutonomyMode>(row, 6)?,
        l5_threshold: row.get::<_, i64>(7)? as i32,
        pattern_depth_cap: row.get::<_, i64>(8)? as i32,
        plan_single_session_lock: bool_col(9)?,
        external_surface: bool_col(10)?,
        timeout_ms: row.get::<_, Option<i64>>(11)?,
        forced: bool_col(12)?,
        set_at: ts(row, 13)?,
        set_by: s(row, 14)?,
        reason: s_opt(row, 15)?,
        created_at: ts(row, 16)?,
        updated_at: ts(row, 17)?,
    })
}

/// Upsert against the unique
/// (project, scope_kind, plan_id, decision_kind, pattern_kind) tuple. New rows
/// get `created_at = updated_at = policy.created_at`; an existing row's
/// `created_at` is preserved, the rest move forward. Caller is responsible for
/// validation (see `AutonomyPolicy::validate_scope` and
/// `validate_mode_against_kind`).
pub fn upsert(conn: &Connection, policy: &AutonomyPolicy) -> DomainResult<()> {
    AutonomyPolicy::validate_scope(
        policy.scope_kind,
        policy.plan_id.as_ref(),
        policy.decision_kind.as_deref(),
        policy.pattern_kind.as_deref(),
    )?;
    AutonomyPolicy::validate_mode_against_kind(
        policy.scope_kind,
        policy.decision_kind.as_deref(),
        policy.mode,
    )?;
    conn.execute(
        &format!(
            "INSERT INTO autonomy_policies({COLS}) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18) \
             ON CONFLICT(project_id, scope_kind, \
                         COALESCE(plan_id,''), \
                         COALESCE(decision_kind,''), \
                         COALESCE(pattern_kind,'')) \
             DO UPDATE SET mode = excluded.mode, \
                           l5_threshold = excluded.l5_threshold, \
                           pattern_depth_cap = excluded.pattern_depth_cap, \
                           plan_single_session_lock = excluded.plan_single_session_lock, \
                           external_surface = excluded.external_surface, \
                           timeout_ms = excluded.timeout_ms, \
                           forced = excluded.forced, \
                           set_at = excluded.set_at, \
                           set_by = excluded.set_by, \
                           reason = excluded.reason, \
                           updated_at = excluded.updated_at"
        ),
        params![
            policy.id.as_str(),
            policy.project_id.as_str(),
            policy.plan_id.as_ref().map(|i| i.as_str().to_string()),
            policy.scope_kind.to_string(),
            policy.decision_kind,
            policy.pattern_kind,
            policy.mode.to_string(),
            policy.l5_threshold as i64,
            policy.pattern_depth_cap as i64,
            policy.plan_single_session_lock as i64,
            policy.external_surface as i64,
            policy.timeout_ms,
            policy.forced as i64,
            fmt_ts(policy.set_at),
            policy.set_by,
            policy.reason,
            fmt_ts(policy.created_at),
            fmt_ts(policy.updated_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn list_by_project(conn: &Connection, project_id: &Id) -> DomainResult<Vec<AutonomyPolicy>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM autonomy_policies WHERE project_id = ?1 \
             ORDER BY scope_kind, COALESCE(plan_id, ''), COALESCE(decision_kind, ''), \
                      COALESCE(pattern_kind, '')"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([project_id.as_str()], row_to_policy)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// Lookup the policy applicable to a (project, plan_id, decision_kind,
/// pattern_kind) tuple. Specificity order: plan-scoped > pattern-kind-scoped >
/// decision-kind-scoped > global. Returns the first hit in that order, or
/// `None` when no policy is set.
pub fn resolve(
    conn: &Connection,
    project_id: &Id,
    plan_id: Option<&Id>,
    decision_kind: Option<&str>,
    pattern_kind: Option<&str>,
) -> DomainResult<Option<AutonomyPolicy>> {
    if let Some(pid) = plan_id {
        if let Some(p) = scope_lookup(
            conn,
            project_id,
            AutonomyScopeKind::Plan,
            Some(pid),
            None,
            None,
        )? {
            return Ok(Some(p));
        }
    }
    if let Some(pk) = pattern_kind {
        if let Some(p) = scope_lookup(
            conn,
            project_id,
            AutonomyScopeKind::PatternKind,
            None,
            None,
            Some(pk),
        )? {
            return Ok(Some(p));
        }
    }
    if let Some(kind) = decision_kind {
        if let Some(p) = scope_lookup(
            conn,
            project_id,
            AutonomyScopeKind::DecisionKind,
            None,
            Some(kind),
            None,
        )? {
            return Ok(Some(p));
        }
    }
    scope_lookup(
        conn,
        project_id,
        AutonomyScopeKind::Global,
        None,
        None,
        None,
    )
}

fn scope_lookup(
    conn: &Connection,
    project_id: &Id,
    scope_kind: AutonomyScopeKind,
    plan_id: Option<&Id>,
    decision_kind: Option<&str>,
    pattern_kind: Option<&str>,
) -> DomainResult<Option<AutonomyPolicy>> {
    let sql = format!(
        "SELECT {COLS} FROM autonomy_policies \
         WHERE project_id = ?1 AND scope_kind = ?2 \
           AND COALESCE(plan_id,'') = ?3 \
           AND COALESCE(decision_kind,'') = ?4 \
           AND COALESCE(pattern_kind,'') = ?5 \
         LIMIT 1"
    );
    let result = conn.query_row(
        &sql,
        params![
            project_id.as_str(),
            scope_kind.to_string(),
            plan_id.map(|i| i.as_str().to_string()).unwrap_or_default(),
            decision_kind.unwrap_or("").to_string(),
            pattern_kind.unwrap_or("").to_string(),
        ],
        row_to_policy,
    );
    match result {
        Ok(p) => Ok(Some(p)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(map_sqlite_err(e)),
    }
}

/// D18 — circuit breaker. Demote every policy under the project to L3 in a
/// single statement; returns the affected row count.
pub fn circuit_breaker(
    conn: &Connection,
    project_id: &Id,
    actor: &str,
    reason: &str,
    at: chrono::DateTime<chrono::Utc>,
) -> DomainResult<usize> {
    let now_s = fmt_ts(at);
    conn.execute(
        "UPDATE autonomy_policies SET mode = 'L3', \
            set_at = ?1, set_by = ?2, reason = ?3, updated_at = ?1 \
         WHERE project_id = ?4 AND mode <> 'L3'",
        params![now_s, actor, reason, project_id.as_str()],
    )
    .map_err(|e| match e {
        rusqlite::Error::SqliteFailure(_, _) => {
            DomainError::Validation(format!("circuit_breaker: {e}"))
        }
        other => map_sqlite_err(other),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::project as proj_repo;
    use crate::{ensure_schema, open_pool};
    use sdi_core::ids::{now, IdKind};
    use sdi_core::project::Project;

    fn fixture() -> (r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, Id) {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-apol-{}-{}.db",
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
        (pool, proj.id)
    }

    fn mk_global(project_id: Id, mode: AutonomyMode) -> AutonomyPolicy {
        AutonomyPolicy {
            id: Id::new(IdKind::AutonomyPolicy),
            project_id,
            plan_id: None,
            scope_kind: AutonomyScopeKind::Global,
            decision_kind: None,
            pattern_kind: None,
            mode,
            l5_threshold: 5,
            pattern_depth_cap: 3,
            plan_single_session_lock: false,
            external_surface: false,
            timeout_ms: None,
            forced: false,
            set_at: now(),
            set_by: "agent".into(),
            reason: None,
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn upsert_replaces_existing_row() {
        let (pool, project_id) = fixture();
        let conn = pool.get().unwrap();
        upsert(&conn, &mk_global(project_id.clone(), AutonomyMode::L5)).unwrap();
        upsert(&conn, &mk_global(project_id.clone(), AutonomyMode::L4)).unwrap();
        let all = list_by_project(&conn, &project_id).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].mode, AutonomyMode::L4);
    }

    #[test]
    fn resolve_falls_back_to_global() {
        let (pool, project_id) = fixture();
        let conn = pool.get().unwrap();
        upsert(&conn, &mk_global(project_id.clone(), AutonomyMode::L5)).unwrap();
        let p = resolve(&conn, &project_id, None, None, None)
            .unwrap()
            .unwrap();
        assert_eq!(p.mode, AutonomyMode::L5);
    }

    #[test]
    fn circuit_breaker_demotes_to_l3() {
        let (pool, project_id) = fixture();
        let conn = pool.get().unwrap();
        upsert(&conn, &mk_global(project_id.clone(), AutonomyMode::L5)).unwrap();
        let n = circuit_breaker(&conn, &project_id, "user", "panic", now()).unwrap();
        assert_eq!(n, 1);
        let p = resolve(&conn, &project_id, None, None, None)
            .unwrap()
            .unwrap();
        assert_eq!(p.mode, AutonomyMode::L3);
    }

    #[test]
    fn forced_l4_rejects_l5_on_architecture_scope() {
        let (pool, project_id) = fixture();
        let conn = pool.get().unwrap();
        let policy = AutonomyPolicy {
            id: Id::new(IdKind::AutonomyPolicy),
            project_id,
            plan_id: None,
            scope_kind: AutonomyScopeKind::DecisionKind,
            decision_kind: Some("architecture".into()),
            pattern_kind: None,
            mode: AutonomyMode::L5,
            l5_threshold: 5,
            pattern_depth_cap: 3,
            plan_single_session_lock: false,
            external_surface: false,
            timeout_ms: None,
            forced: true,
            set_at: now(),
            set_by: "agent".into(),
            reason: None,
            created_at: now(),
            updated_at: now(),
        };
        let err = upsert(&conn, &policy).unwrap_err();
        assert!(matches!(err, DomainError::AutonomyGateBlocked { .. }));
    }

    #[test]
    fn pattern_kind_scope_round_trips_v05_columns() {
        let (pool, project_id) = fixture();
        let conn = pool.get().unwrap();
        let policy = AutonomyPolicy {
            id: Id::new(IdKind::AutonomyPolicy),
            project_id: project_id.clone(),
            plan_id: None,
            scope_kind: AutonomyScopeKind::PatternKind,
            decision_kind: None,
            pattern_kind: Some("swarm".into()),
            mode: AutonomyMode::L4,
            l5_threshold: 7,
            pattern_depth_cap: 5,
            plan_single_session_lock: true,
            external_surface: true,
            timeout_ms: Some(30_000),
            forced: false,
            set_at: now(),
            set_by: "agent".into(),
            reason: Some("test".into()),
            created_at: now(),
            updated_at: now(),
        };
        upsert(&conn, &policy).unwrap();
        let resolved = resolve(&conn, &project_id, None, None, Some("swarm"))
            .unwrap()
            .unwrap();
        assert_eq!(resolved.mode, AutonomyMode::L4);
        assert_eq!(resolved.pattern_kind.as_deref(), Some("swarm"));
        assert_eq!(resolved.l5_threshold, 7);
        assert_eq!(resolved.pattern_depth_cap, 5);
        assert!(resolved.plan_single_session_lock);
        assert!(resolved.external_surface);
        assert_eq!(resolved.timeout_ms, Some(30_000));
        assert!(!resolved.forced);
        assert_eq!(resolved.reason.as_deref(), Some("test"));
    }

    #[test]
    fn pattern_and_decision_kind_coexist_in_same_project() {
        let (pool, project_id) = fixture();
        let conn = pool.get().unwrap();
        // pattern-kind policy (workflow → L5)
        upsert(
            &conn,
            &AutonomyPolicy {
                id: Id::new(IdKind::AutonomyPolicy),
                project_id: project_id.clone(),
                plan_id: None,
                scope_kind: AutonomyScopeKind::PatternKind,
                decision_kind: None,
                pattern_kind: Some("workflow".into()),
                mode: AutonomyMode::L5,
                l5_threshold: 5,
                pattern_depth_cap: 3,
                plan_single_session_lock: false,
                external_surface: false,
                timeout_ms: None,
                forced: false,
                set_at: now(),
                set_by: "agent".into(),
                reason: None,
                created_at: now(),
                updated_at: now(),
            },
        )
        .unwrap();
        // decision-kind policy (architecture → forced L4 satisfies its own floor)
        upsert(
            &conn,
            &AutonomyPolicy {
                id: Id::new(IdKind::AutonomyPolicy),
                project_id: project_id.clone(),
                plan_id: None,
                scope_kind: AutonomyScopeKind::DecisionKind,
                decision_kind: Some("architecture".into()),
                pattern_kind: None,
                mode: AutonomyMode::L4,
                l5_threshold: 5,
                pattern_depth_cap: 3,
                plan_single_session_lock: false,
                external_surface: false,
                timeout_ms: None,
                forced: true,
                set_at: now(),
                set_by: "agent".into(),
                reason: None,
                created_at: now(),
                updated_at: now(),
            },
        )
        .unwrap();
        let all = list_by_project(&conn, &project_id).unwrap();
        assert_eq!(all.len(), 2);
    }
}
