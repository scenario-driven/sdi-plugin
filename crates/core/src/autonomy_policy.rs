//! D14 — AutonomyPolicy: the sixth first-class entity. Per-scope autonomy
//! mode (L3/L4/L5) gates the position of the human approval window on
//! consensus decisions (D20). It does NOT gate the multi-agent communication
//! substrate (M1~M5), which runs mode-independent (D19).

use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub type AutonomyPolicyId = Id;

/// Where the policy applies. Mirrors the SQL CHECK constraint in migrations
/// 006 (`plan`/`decision_kind`/`global`) and 007 (D25 added `pattern_kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyScopeKind {
    Plan,
    DecisionKind,
    PatternKind,
    Global,
}

impl fmt::Display for AutonomyScopeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AutonomyScopeKind::Plan => "plan",
            AutonomyScopeKind::DecisionKind => "decision_kind",
            AutonomyScopeKind::PatternKind => "pattern_kind",
            AutonomyScopeKind::Global => "global",
        })
    }
}

impl FromStr for AutonomyScopeKind {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "plan" => Ok(AutonomyScopeKind::Plan),
            "decision_kind" => Ok(AutonomyScopeKind::DecisionKind),
            "pattern_kind" => Ok(AutonomyScopeKind::PatternKind),
            "global" => Ok(AutonomyScopeKind::Global),
            other => Err(DomainError::Validation(format!(
                "unknown autonomy scope_kind: {other}"
            ))),
        }
    }
}

/// L3 = ask (human confirms each decision).
/// L4 = act-with-review (auto on consensus; user can revoke before apply).
/// L5 = act-and-notify (auto on consensus; user notified post-hoc).
///
/// D17: new plans default to L5; plans with external surface default to L4;
/// decision-kind ∈ {architecture, schema, naming-canonical} are forced to L4.
/// D18: circuit breaker demotes all modes to L3 instantly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AutonomyMode {
    L3,
    L4,
    L5,
}

impl AutonomyMode {
    /// D17 — decision-kinds that always require L4 (or stricter).
    /// `kind` is matched against the canonical decision-kind string.
    pub fn is_forced_l4(kind: &str) -> bool {
        matches!(kind, "architecture" | "schema" | "naming-canonical")
    }

    /// Returns the strictness floor for a given decision-kind.
    /// Higher (numerically smaller variant index) = stricter.
    pub fn floor_for_kind(kind: &str) -> AutonomyMode {
        if Self::is_forced_l4(kind) {
            AutonomyMode::L4
        } else {
            AutonomyMode::L3
        }
    }

    /// True iff `self` is at least as strict as `floor`.
    /// L3 is the strictest (always asks); L5 is the loosest.
    pub fn satisfies(self, floor: AutonomyMode) -> bool {
        let rank = |m: AutonomyMode| match m {
            AutonomyMode::L3 => 0,
            AutonomyMode::L4 => 1,
            AutonomyMode::L5 => 2,
        };
        rank(self) <= rank(floor)
    }

    /// D18 — circuit breaker demotion.
    pub fn demoted() -> Self {
        AutonomyMode::L3
    }

    /// D25 — pattern-kind default modes:
    /// `workflow=L5`, `graph=L5`, `swarm=L4`, `agents-as-tools=L4`,
    /// `direct=L3` (forced). Unknown kinds fall back to L3 (strictest).
    pub fn default_pattern_kind_mode(kind: &str) -> AutonomyMode {
        match kind {
            "workflow" => AutonomyMode::L5,
            "graph" => AutonomyMode::L5,
            "swarm" => AutonomyMode::L4,
            "agents-as-tools" => AutonomyMode::L4,
            "direct" => AutonomyMode::L3,
            _ => AutonomyMode::L3,
        }
    }

    /// D25 — when plan-level mode and pattern-level mode are both present,
    /// the strictest (numerically smallest rank) wins. L3 > L4 > L5 in
    /// strictness, so the returned mode is the one that "asks more often".
    pub fn effective_mode(plan_mode: AutonomyMode, pattern_mode: AutonomyMode) -> AutonomyMode {
        let rank = |m: AutonomyMode| match m {
            AutonomyMode::L3 => 0,
            AutonomyMode::L4 => 1,
            AutonomyMode::L5 => 2,
        };
        if rank(plan_mode) <= rank(pattern_mode) {
            plan_mode
        } else {
            pattern_mode
        }
    }
}

impl fmt::Display for AutonomyMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AutonomyMode::L3 => "L3",
            AutonomyMode::L4 => "L4",
            AutonomyMode::L5 => "L5",
        })
    }
}

impl FromStr for AutonomyMode {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "L3" => Ok(AutonomyMode::L3),
            "L4" => Ok(AutonomyMode::L4),
            "L5" => Ok(AutonomyMode::L5),
            other => Err(DomainError::Validation(format!(
                "unknown autonomy mode: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomyPolicy {
    pub id: AutonomyPolicyId,
    pub project_id: Id,
    pub plan_id: Option<Id>,
    pub scope_kind: AutonomyScopeKind,
    pub decision_kind: Option<String>,
    /// D25 — when scope_kind = pattern_kind, the pattern kind this policy
    /// scopes to (`workflow` / `graph` / `swarm` / `agents-as-tools` /
    /// `direct`). NULL for any other scope.
    #[serde(default)]
    pub pattern_kind: Option<String>,
    pub mode: AutonomyMode,
    /// D28 — `blast_radius_score ≤ l5_threshold` is one of the three L5
    /// unlock conditions. Default 5 = architecture/schema (score 10/8) auto
    /// downgrade to L4.
    #[serde(default = "default_l5_threshold")]
    pub l5_threshold: i32,
    /// D24 — CollaborationPattern.depth chain cap. Default 3 = "plan workflow
    /// → step swarm → swarm step agents-as-tools".
    #[serde(default = "default_pattern_depth_cap")]
    pub pattern_depth_cap: i32,
    /// D29 — when true, a plan's N active scenarios may only claim from one
    /// session at a time. Default false (multi-session collaboration natural).
    #[serde(default)]
    pub plan_single_session_lock: bool,
    /// D17 — marks a plan that ships publish/deploy/external API; defaults
    /// its mode to L4 instead of L5.
    #[serde(default)]
    pub external_surface: bool,
    /// L4 timed-gate auto-apply delay in milliseconds. NULL → no auto-apply.
    #[serde(default)]
    pub timeout_ms: Option<i64>,
    /// D17 — true when the policy was set by a forced-L4 invariant
    /// (architecture/schema/naming-canonical) rather than a user override.
    #[serde(default)]
    pub forced: bool,
    pub set_at: Timestamp,
    pub set_by: String,
    pub reason: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

fn default_l5_threshold() -> i32 {
    5
}

fn default_pattern_depth_cap() -> i32 {
    3
}

impl AutonomyPolicy {
    /// Validate the (scope_kind, plan_id, decision_kind, pattern_kind) tuple
    /// — must match the SQL CHECK in migrations 006 + 007.
    pub fn validate_scope(
        scope_kind: AutonomyScopeKind,
        plan_id: Option<&Id>,
        decision_kind: Option<&str>,
        pattern_kind: Option<&str>,
    ) -> DomainResult<()> {
        match scope_kind {
            AutonomyScopeKind::Plan => {
                if plan_id.is_none() {
                    return Err(DomainError::Validation(
                        "scope=plan requires plan_id".into(),
                    ));
                }
                if decision_kind.is_some() {
                    return Err(DomainError::Validation(
                        "scope=plan forbids decision_kind".into(),
                    ));
                }
                if pattern_kind.is_some() {
                    return Err(DomainError::Validation(
                        "scope=plan forbids pattern_kind".into(),
                    ));
                }
            }
            AutonomyScopeKind::DecisionKind => {
                if decision_kind.is_none() {
                    return Err(DomainError::Validation(
                        "scope=decision_kind requires decision_kind".into(),
                    ));
                }
                if pattern_kind.is_some() {
                    return Err(DomainError::Validation(
                        "scope=decision_kind forbids pattern_kind".into(),
                    ));
                }
            }
            AutonomyScopeKind::PatternKind => {
                if pattern_kind.is_none() {
                    return Err(DomainError::Validation(
                        "scope=pattern_kind requires pattern_kind".into(),
                    ));
                }
                if plan_id.is_some() {
                    return Err(DomainError::Validation(
                        "scope=pattern_kind forbids plan_id".into(),
                    ));
                }
                if decision_kind.is_some() {
                    return Err(DomainError::Validation(
                        "scope=pattern_kind forbids decision_kind".into(),
                    ));
                }
                if let Some(pk) = pattern_kind {
                    if !matches!(
                        pk,
                        "workflow" | "graph" | "swarm" | "agents-as-tools" | "direct"
                    ) {
                        return Err(DomainError::Validation(format!(
                            "unknown pattern_kind: {pk}"
                        )));
                    }
                }
            }
            AutonomyScopeKind::Global => {
                if plan_id.is_some() || decision_kind.is_some() || pattern_kind.is_some() {
                    return Err(DomainError::Validation(
                        "scope=global forbids plan_id / decision_kind / pattern_kind".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// D17 — when scope=decision_kind, reject modes weaker than the floor.
    pub fn validate_mode_against_kind(
        scope_kind: AutonomyScopeKind,
        decision_kind: Option<&str>,
        mode: AutonomyMode,
    ) -> DomainResult<()> {
        if matches!(scope_kind, AutonomyScopeKind::DecisionKind) {
            if let Some(kind) = decision_kind {
                let floor = AutonomyMode::floor_for_kind(kind);
                if !mode.satisfies(floor) {
                    return Err(DomainError::AutonomyGateBlocked {
                        kind: kind.into(),
                        required: floor.to_string(),
                        actual: mode.to_string(),
                    });
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_forces_architecture_to_l4() {
        assert_eq!(AutonomyMode::floor_for_kind("architecture"), AutonomyMode::L4);
        assert_eq!(AutonomyMode::floor_for_kind("schema"), AutonomyMode::L4);
        assert_eq!(
            AutonomyMode::floor_for_kind("naming-canonical"),
            AutonomyMode::L4
        );
        assert_eq!(AutonomyMode::floor_for_kind("general"), AutonomyMode::L3);
    }

    #[test]
    fn l5_does_not_satisfy_l4_floor() {
        assert!(!AutonomyMode::L5.satisfies(AutonomyMode::L4));
        assert!(AutonomyMode::L4.satisfies(AutonomyMode::L4));
        assert!(AutonomyMode::L3.satisfies(AutonomyMode::L4));
    }

    #[test]
    fn validate_scope_plan_requires_plan_id() {
        let err =
            AutonomyPolicy::validate_scope(AutonomyScopeKind::Plan, None, None, None).unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn validate_scope_global_forbids_plan_id() {
        let pid: Id = "PLAN-x".into();
        let err =
            AutonomyPolicy::validate_scope(AutonomyScopeKind::Global, Some(&pid), None, None)
                .unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn validate_scope_pattern_kind_requires_pattern_kind() {
        let err = AutonomyPolicy::validate_scope(
            AutonomyScopeKind::PatternKind,
            None,
            None,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn validate_scope_pattern_kind_accepts_known_kind() {
        AutonomyPolicy::validate_scope(
            AutonomyScopeKind::PatternKind,
            None,
            None,
            Some("workflow"),
        )
        .unwrap();
    }

    #[test]
    fn validate_scope_pattern_kind_rejects_unknown_kind() {
        let err = AutonomyPolicy::validate_scope(
            AutonomyScopeKind::PatternKind,
            None,
            None,
            Some("rogue"),
        )
        .unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn default_pattern_kind_mode_defaults() {
        assert_eq!(AutonomyMode::default_pattern_kind_mode("workflow"), AutonomyMode::L5);
        assert_eq!(AutonomyMode::default_pattern_kind_mode("graph"), AutonomyMode::L5);
        assert_eq!(AutonomyMode::default_pattern_kind_mode("swarm"), AutonomyMode::L4);
        assert_eq!(
            AutonomyMode::default_pattern_kind_mode("agents-as-tools"),
            AutonomyMode::L4
        );
        assert_eq!(AutonomyMode::default_pattern_kind_mode("direct"), AutonomyMode::L3);
    }

    #[test]
    fn effective_mode_picks_strictest() {
        assert_eq!(
            AutonomyMode::effective_mode(AutonomyMode::L5, AutonomyMode::L4),
            AutonomyMode::L4
        );
        assert_eq!(
            AutonomyMode::effective_mode(AutonomyMode::L4, AutonomyMode::L5),
            AutonomyMode::L4
        );
        assert_eq!(
            AutonomyMode::effective_mode(AutonomyMode::L3, AutonomyMode::L5),
            AutonomyMode::L3
        );
    }

    #[test]
    fn validate_mode_blocks_l5_on_architecture() {
        let err = AutonomyPolicy::validate_mode_against_kind(
            AutonomyScopeKind::DecisionKind,
            Some("architecture"),
            AutonomyMode::L5,
        )
        .unwrap_err();
        assert!(matches!(err, DomainError::AutonomyGateBlocked { .. }));
    }
}
