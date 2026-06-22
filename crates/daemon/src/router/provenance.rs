//! D23 provenance back-fill helpers.
//!
//! Every work entity (plan / requirement / scenario / task / decision / round)
//! must carry a `produced_via_pattern_id`. When a create path receives none —
//! the common solo-flow / CLI case — it resolves a per-plan `direct` sentinel
//! pattern: the explicit anti-pattern marker (D23) recording that no
//! CollaborationPattern was used.
//!
//! The sentinel is idempotent: one row per plan, keyed by a stable
//! `short_code`. Repeated solo writes share the same marker instead of
//! flooding the pattern timeline with one row per entity — `direct` is the
//! *absence* of orchestration, a plan-level singleton, not a per-outcome
//! pattern instance.

use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id, IdKind};
use sdi_core::plan::{Plan, PlanStatus};
use sdi_core::round::{DisruptionPolicy, InFlightPolicy, Round, RoundMode, RoundStatus};
use sdi_db::repo::pattern::{self as pattern_repo, PatternRow};
use sdi_db::repo::plan as plan_repo;
use sdi_db::repo::round as round_repo;
use sdi_db::PooledConn;

/// Stable short_codes for the per-project chore maintenance lane (#18).
const CHORE_PLAN_SHORT_CODE: &str = "CHORE";
const CHORE_ROUND_SHORT_CODE: &str = "CHORE-R";

/// Find (or create) the per-project chore container — a permanently-`active`
/// Plan (`short_code = "CHORE"`) holding one permanently-`active` Round
/// (`short_code = "CHORE-R"`) — and return `(plan_id, round_id)` (#18).
///
/// This is the lightweight maintenance lane: a `kind='chore'` task lives under
/// this round so the active-task PreToolUse gate is satisfiable for a trivial
/// consistency edit when there is no real active plan. The container is
/// idempotent — one CHORE plan + one CHORE-R round per project, reused across
/// every chore. It does NOT count as the project's active plan: migration 015's
/// partial unique indexes and `plan_repo::find_active_for_project` both exclude
/// `short_code LIKE 'CHORE%'`, so D8's single-active-plan invariant is untouched
/// for real work plans and the container can coexist with one.
///
/// Mirrors [`ensure_direct_pattern`]'s race handling: a caller that loses the
/// insert race on the UNIQUE short_code re-reads the winning row.
pub fn ensure_chore_container(conn: &PooledConn, project_id: &Id) -> DomainResult<(Id, Id)> {
    let plan_id = ensure_chore_plan(conn, project_id)?;
    let round_id = ensure_chore_round(conn, &plan_id)?;
    Ok((plan_id, round_id))
}

fn ensure_chore_plan(conn: &PooledConn, project_id: &Id) -> DomainResult<Id> {
    if let Some(existing) =
        plan_repo::find_by_project_short_code(conn, project_id, CHORE_PLAN_SHORT_CODE)?
    {
        return Ok(existing.id);
    }
    let plan = Plan {
        id: Id::new(IdKind::Plan),
        project_id: project_id.clone(),
        short_code: CHORE_PLAN_SHORT_CODE.into(),
        title: "Chores / maintenance".into(),
        body: String::new(),
        status: PlanStatus::Active,
        version: 0,
        // The container is solo-flow scaffolding; provenance is resolved lazily
        // by the chore tasks themselves (each binds the plan's `direct`
        // sentinel), never by the container shell.
        produced_via_pattern_id: None,
        approved_at: Some(now()),
        completed_at: None,
        created_at: now(),
        updated_at: now(),
    };
    match plan_repo::insert(conn, &plan) {
        Ok(()) => Ok(plan.id),
        Err(DomainError::Conflict(_)) => {
            plan_repo::find_by_project_short_code(conn, project_id, CHORE_PLAN_SHORT_CODE)?
                .map(|p| p.id)
                .ok_or_else(|| DomainError::NotFound(CHORE_PLAN_SHORT_CODE.into()))
        }
        Err(e) => Err(e),
    }
}

fn ensure_chore_round(conn: &PooledConn, plan_id: &Id) -> DomainResult<Id> {
    if let Some(existing) =
        round_repo::find_by_plan_short_code(conn, plan_id, CHORE_ROUND_SHORT_CODE)?
    {
        return Ok(existing.id);
    }
    let round = Round {
        id: Id::new(IdKind::Round),
        plan_id: plan_id.clone(),
        short_code: CHORE_ROUND_SHORT_CODE.into(),
        round_number: 1,
        mode: RoundMode::ForwardOnly,
        in_flight_policy: InFlightPolicy::Pause,
        disruption_policy: DisruptionPolicy::NeedsReview,
        status: RoundStatus::Active,
        produced_via_pattern_id: None,
        activated_at: Some(now()),
        completed_at: None,
        created_at: now(),
        updated_at: now(),
    };
    match round_repo::insert(conn, &round) {
        Ok(()) => Ok(round.id),
        Err(DomainError::Conflict(_)) => {
            round_repo::find_by_plan_short_code(conn, plan_id, CHORE_ROUND_SHORT_CODE)?
                .map(|r| r.id)
                .ok_or_else(|| DomainError::NotFound(CHORE_ROUND_SHORT_CODE.into()))
        }
        Err(e) => Err(e),
    }
}

/// Stable short_code for a plan's direct sentinel. Plan short_codes are
/// globally unique (001_core.sql), so this never collides across plans.
fn sentinel_short_code(plan_short_code: &str) -> String {
    format!("DIRECT-{plan_short_code}")
}

/// Find (or create) the `direct` sentinel CollaborationPattern for `plan_id`
/// and return its id. Idempotent: a concurrent caller that loses the insert
/// race re-reads the winning row rather than erroring.
pub fn ensure_direct_pattern(conn: &PooledConn, plan_id: &Id) -> DomainResult<String> {
    let plan = plan_repo::get(conn, plan_id)?;
    let short_code = sentinel_short_code(&plan.short_code);

    if let Some(existing) = pattern_repo::get_by_short_code(conn, plan_id, &short_code)? {
        return Ok(existing.id);
    }

    let row = PatternRow {
        id: Id::new(IdKind::Pattern).to_string(),
        short_code: short_code.clone(),
        plan_id: plan_id.to_string(),
        kind: "direct".into(),
        applies_to: "plan".into(),
        scope_id: plan_id.to_string(),
        parent_pattern_id: None,
        depth: 0,
        // `active` = this plan is currently producing work solo. `direct`
        // always passes the D26 shape gate; its cost is the forced L3 cap.
        lifecycle: "active".into(),
        steps_json: None,
        reviewers_json: None,
        fan_out_json: None,
        peer_registration_json: None,
        decided_at: None,
        decided_reason: Some(
            "solo-flow marker — produced without a collaboration pattern (D23)".into(),
        ),
        created_at: now(),
        updated_at: now(),
    };

    match pattern_repo::insert(conn, &row) {
        Ok(()) => Ok(row.id),
        // Lost an insert race on the UNIQUE short_code; the winner is readable.
        Err(DomainError::Conflict(_)) => {
            pattern_repo::get_by_short_code(conn, plan_id, &short_code)?
                .map(|p| p.id)
                .ok_or(DomainError::NotFound(short_code))
        }
        Err(e) => Err(e),
    }
}

/// Resolve and validate an explicitly-supplied CollaborationPattern reference
/// for a new work entity bound to `plan_id` (and, where relevant, `round_id`).
///
/// This is the counterpart to [`ensure_direct_pattern`]: when a create path
/// DOES carry a chosen pattern (the pattern-orchestrator picked a
/// workflow/graph/swarm/agents-as-tools for this decompose), the binding has
/// to be validated rather than silently trusted.
///
/// `pat_ref` is either a pattern ULID or a plan-scoped `short_code`. Rules:
/// - it must resolve to a pattern in **this** plan;
/// - it must be `active` — D27 only lets work be produced under patterns that
///   already passed the `pending → active` shape gate (a `pending` pattern has
///   not been validated, so binding to it would smuggle an unshaped graph);
/// - scope sanity: a `plan`-scoped pattern must target this plan and a
///   `round`-scoped pattern must target this round. Finer scopes
///   (task/scenario/decision/requirement) are accepted as the orchestrator's
///   deliberate choice for this decompose.
///
/// An invalid explicit reference returns a `Validation` error so the caller
/// sees the mistake instead of silently inheriting `direct`'s L3 cap.
pub fn resolve_bound_pattern(
    conn: &PooledConn,
    plan_id: &Id,
    round_id: Option<&Id>,
    pat_ref: &str,
) -> DomainResult<String> {
    let resolved = match pattern_repo::get(conn, &Id::from(pat_ref.to_string()))? {
        Some(row) => Some(row),
        None => pattern_repo::get_by_short_code(conn, plan_id, pat_ref)?,
    };
    let row = resolved.ok_or_else(|| {
        DomainError::Validation(format!(
            "produced_via_pattern_id {pat_ref:?} does not resolve to a CollaborationPattern in this plan"
        ))
    })?;

    if row.plan_id != plan_id.to_string() {
        return Err(DomainError::Validation(format!(
            "pattern {} belongs to a different plan",
            row.short_code
        )));
    }
    if row.lifecycle != "active" {
        return Err(DomainError::Validation(format!(
            "pattern {} is `{}` — only `active` patterns (past the D27 shape gate) may produce work",
            row.short_code, row.lifecycle
        )));
    }
    match row.applies_to.as_str() {
        "plan" if row.scope_id != plan_id.to_string() => {
            return Err(DomainError::Validation(format!(
                "plan-scoped pattern {} targets a different plan",
                row.short_code
            )));
        }
        "round" => {
            if let Some(rid) = round_id {
                if row.scope_id != rid.to_string() {
                    return Err(DomainError::Validation(format!(
                        "round-scoped pattern {} targets a different round",
                        row.short_code
                    )));
                }
            }
        }
        _ => {}
    }
    Ok(row.id)
}
