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

/// Where the policy applies. Mirrors the SQL CHECK constraint in migration 006.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyScopeKind {
    Plan,
    DecisionKind,
    Global,
}

impl fmt::Display for AutonomyScopeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AutonomyScopeKind::Plan => "plan",
            AutonomyScopeKind::DecisionKind => "decision_kind",
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
    pub mode: AutonomyMode,
    pub set_at: Timestamp,
    pub set_by: String,
    pub reason: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl AutonomyPolicy {
    /// Validate the (scope_kind, plan_id, decision_kind) tuple — must match the
    /// SQL CHECK in migration 006.
    pub fn validate_scope(
        scope_kind: AutonomyScopeKind,
        plan_id: Option<&Id>,
        decision_kind: Option<&str>,
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
            }
            AutonomyScopeKind::DecisionKind => {
                if decision_kind.is_none() {
                    return Err(DomainError::Validation(
                        "scope=decision_kind requires decision_kind".into(),
                    ));
                }
            }
            AutonomyScopeKind::Global => {
                if plan_id.is_some() || decision_kind.is_some() {
                    return Err(DomainError::Validation(
                        "scope=global forbids plan_id and decision_kind".into(),
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
        let err = AutonomyPolicy::validate_scope(AutonomyScopeKind::Plan, None, None).unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn validate_scope_global_forbids_plan_id() {
        let pid: Id = "PLAN-x".into();
        let err = AutonomyPolicy::validate_scope(AutonomyScopeKind::Global, Some(&pid), None)
            .unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
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
