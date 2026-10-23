//! Retry budget repository — failure counters keyed by
//! (plan, scenario, failure class, signature).
//!
//! The key deliberately omits task id and title: the defect the budget stops
//! is "the same cause retried under a new task name". Cancelling, recreating
//! or renaming a task therefore cannot reset a counter. `count` returns to 0
//! only through [`release`], which requires a decision of the same plan — the
//! escalation the exhausted error asks for. `total_failures` is never reset.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, parsed, s, s_opt, ts, ts_opt};
use rusqlite::{params, Connection, Row};
use sdi_core::decision::DecisionStatus;
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};
use sdi_core::verification::{FailureClass, RetryBudget, DEFAULT_RETRY_BUDGET_CAP};

const COLS: &str = "plan_id, scenario_id, failure_class, failure_signature, count, total_failures, \
                    first_run_id, last_run_id, last_failed_at, released_by_decision_id, released_at";

fn row_to_budget(row: &Row<'_>, cap: i64) -> rusqlite::Result<RetryBudget> {
    let count: i64 = row.get(4)?;
    Ok(RetryBudget {
        plan_id: Id::from(s(row, 0)?),
        scenario_id: Id::from(s(row, 1)?),
        failure_class: parsed::<FailureClass>(row, 2)?,
        failure_signature: s(row, 3)?,
        count,
        cap,
        exhausted: count >= cap,
        total_failures: row.get(5)?,
        first_run_id: s_opt(row, 6)?.map(Id::from),
        last_run_id: s_opt(row, 7)?.map(Id::from),
        last_failed_at: ts(row, 8)?,
        released_by_decision_id: s_opt(row, 9)?.map(Id::from),
        released_at: ts_opt(row, 10)?,
    })
}

/// Effective cap for a plan: the plan-scoped (or project-global) autonomy
/// policy's `retry_budget_cap`, else [`DEFAULT_RETRY_BUDGET_CAP`].
pub fn cap_for_plan(conn: &Connection, plan_id: &Id) -> DomainResult<i64> {
    let plan = crate::repo::plan::get(conn, plan_id)?;
    let policy =
        crate::repo::autonomy_policy::resolve(conn, &plan.project_id, Some(plan_id), None, None)?;
    Ok(policy
        .map(|p| p.retry_budget_cap)
        .unwrap_or(DEFAULT_RETRY_BUDGET_CAP) as i64)
}

/// Book one failure. Idempotency is the caller's: a run is finished exactly
/// once (the `finished_at IS NULL` guard), and that is the only caller.
pub fn record_failure(
    conn: &Connection,
    plan_id: &Id,
    scenario_id: &Id,
    class: FailureClass,
    signature: &str,
    run_id: &Id,
) -> DomainResult<RetryBudget> {
    if !class.is_failure() {
        return Err(DomainError::Validation(
            "retry budget only books failures (class none given)".into(),
        ));
    }
    conn.execute(
        "INSERT INTO retry_budgets(plan_id, scenario_id, failure_class, failure_signature, \
             count, total_failures, first_run_id, last_run_id, last_failed_at) \
         VALUES (?1, ?2, ?3, ?4, 1, 1, ?5, ?5, ?6) \
         ON CONFLICT(plan_id, scenario_id, failure_class, failure_signature) DO UPDATE SET \
             count = count + 1, total_failures = total_failures + 1, \
             first_run_id = COALESCE(first_run_id, excluded.first_run_id), \
             last_run_id = excluded.last_run_id, last_failed_at = excluded.last_failed_at",
        params![
            plan_id.as_str(),
            scenario_id.as_str(),
            class.to_string(),
            signature,
            run_id.as_str(),
            fmt_ts(now()),
        ],
    )
    .map_err(map_sqlite_err)?;
    let cap = cap_for_plan(conn, plan_id)?;
    conn.query_row(
        &format!(
            "SELECT {COLS} FROM retry_budgets WHERE plan_id = ?1 AND scenario_id = ?2 \
             AND failure_class = ?3 AND failure_signature = ?4"
        ),
        params![
            plan_id.as_str(),
            scenario_id.as_str(),
            class.to_string(),
            signature
        ],
        |r| row_to_budget(r, cap),
    )
    .map_err(map_sqlite_err)
}

/// Budgets of a plan, optionally narrowed to one scenario.
pub fn list(
    conn: &Connection,
    plan_id: &Id,
    scenario_id: Option<&Id>,
) -> DomainResult<Vec<RetryBudget>> {
    let cap = cap_for_plan(conn, plan_id)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM retry_budgets WHERE plan_id = ?1 \
             AND (?2 IS NULL OR scenario_id = ?2) \
             ORDER BY scenario_id, failure_class, failure_signature"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map(
            params![
                plan_id.as_str(),
                scenario_id.map(|i| i.as_str().to_string())
            ],
            |r| row_to_budget(r, cap),
        )
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// Outstanding coarse identities retain their accounting authority until an
/// explicit release. A diagnosis cannot silently repartition old failures.
pub fn compatibility_bucket(
    conn: &Connection,
    plan_id: &Id,
    scenario_id: &Id,
    class: FailureClass,
    diagnostic_unavailable: bool,
) -> DomainResult<Option<RetryBudget>> {
    let mut budgets = Vec::new();
    for budget in list(conn, plan_id, Some(scenario_id))? {
        if budget.failure_class != class || budget.count == 0 {
            continue;
        }
        // The first run owns the bucket's provenance. A later bridged diagnosis
        // cannot turn an originally coarse counter into a precise one.
        let diagnosed = if !diagnostic_unavailable
            && budget.failure_signature.starts_with("sig:v2:")
            && !budget.failure_signature.starts_with("sig:v2:fallback:")
        {
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM runs WHERE id = ?1 AND plan_id = ?2 \
                 AND scenario_id = ?3 AND failure_class = ?4 AND failure_signature = ?5 \
                 AND CASE WHEN json_valid(failure_identity_json) THEN \
                   json_extract(failure_identity_json, '$.computed.version') = 2 \
                   AND json_extract(failure_identity_json, '$.computed.basis') = 'primary-diagnostic' \
                   AND json_extract(failure_identity_json, '$.computed.signature') = ?5 \
                 ELSE 0 END)",
                params![budget.first_run_id.as_ref().map(Id::as_str), plan_id.as_str(),
                    scenario_id.as_str(), class.to_string(), budget.failure_signature],
                |row| row.get::<_, bool>(0),
            ).map_err(map_sqlite_err)?
        } else {
            false
        };
        if !diagnosed {
            budgets.push(budget);
        }
    }
    budgets.sort_by(|a, b| {
        b.exhausted
            .cmp(&a.exhausted)
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.failure_signature.cmp(&b.failure_signature))
    });
    Ok(budgets.into_iter().next())
}

/// Exhausted budgets for any of `scenario_ids` in the plan.
pub fn exhausted_for(
    conn: &Connection,
    plan_id: &Id,
    scenario_ids: &[Id],
) -> DomainResult<Vec<RetryBudget>> {
    let mut out = Vec::new();
    for sid in scenario_ids {
        out.extend(
            list(conn, plan_id, Some(sid))?
                .into_iter()
                .filter(|b| b.exhausted),
        );
    }
    Ok(out)
}

/// Refuse when any of `scenario_ids` has an exhausted budget.
pub fn ensure_available(conn: &Connection, plan_id: &Id, scenario_ids: &[Id]) -> DomainResult<()> {
    if let Some(b) = exhausted_for(conn, plan_id, scenario_ids)?
        .into_iter()
        .next()
    {
        return Err(exhausted_error(&b));
    }
    Ok(())
}

pub fn exhausted_error(b: &RetryBudget) -> DomainError {
    DomainError::RetryBudgetExhausted {
        scenario: b.scenario_id.to_string(),
        class: b.failure_class.to_string(),
        signature: b.failure_signature.clone(),
        count: b.count,
        cap: b.cap,
    }
}

/// Release (reset `count` to 0) the scenario's budgets — all signatures, or
/// one — on the authority of a decision in the same plan. A superseded
/// decision no longer speaks for the plan and cannot release anything.
pub fn release(
    conn: &Connection,
    plan_id: &Id,
    scenario_id: &Id,
    signature: Option<&str>,
    decision_id: &Id,
) -> DomainResult<usize> {
    let decision = crate::repo::decision::get(conn, decision_id)
        .map_err(|_| DomainError::Validation(format!("decision {decision_id} not found")))?;
    if decision.plan_id != *plan_id {
        return Err(DomainError::Validation(format!(
            "decision {decision_id} belongs to plan {}, not {plan_id}",
            decision.plan_id
        )));
    }
    if decision.status == DecisionStatus::Superseded {
        return Err(DomainError::Validation(format!(
            "decision {decision_id} is superseded and cannot release a retry budget"
        )));
    }
    let n = conn
        .execute(
            "UPDATE retry_budgets SET count = 0, released_by_decision_id = ?1, released_at = ?2 \
             WHERE plan_id = ?3 AND scenario_id = ?4 AND (?5 IS NULL OR failure_signature = ?5)",
            params![
                decision_id.as_str(),
                fmt_ts(now()),
                plan_id.as_str(),
                scenario_id.as_str(),
                signature,
            ],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(format!(
            "no retry budget for scenario {scenario_id}{}",
            signature
                .map(|s| format!(" with signature {s}"))
                .unwrap_or_default()
        )));
    }
    Ok(n)
}
