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
use sdi_db::repo::pattern::{self as pattern_repo, PatternRow};
use sdi_db::repo::plan as plan_repo;
use sdi_db::PooledConn;

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
        Err(DomainError::Conflict(_)) => pattern_repo::get_by_short_code(conn, plan_id, &short_code)?
            .map(|p| p.id)
            .ok_or(DomainError::NotFound(short_code)),
        Err(e) => Err(e),
    }
}
