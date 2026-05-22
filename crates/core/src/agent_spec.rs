//! Layer-2 specialist sub-agents registered as first-class records so the
//! orchestrator (Layer 1) can spawn/recall instances at runtime (M5).
//!
//! Only the seeded eight roles below are valid in v0.4 — M5 self-organization
//! adjusts `instance_count` but never adds new roles (PRD §5.5 Layer 1).

use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};

pub type AgentSpecId = Id;

/// Canonical specialist names enumerated by D-7.
pub const STOCK_AGENTS: &[&str] = &[
    "gwt-converter",
    "scenario-decomposer",
    "impl-coder",
    "test-runner",
    "regression-runner",
    "disruption-analyst",
    "decision-resolver",
    "schema-architect",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub id: AgentSpecId,
    pub name: String,
    pub role: String,
    pub system_prompt: String,
    pub instance_count: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl AgentSpec {
    /// Reject any role not in the canonical eight (M5 forbids dynamic role
    /// invention; humans add new roles via plugin manifest edits + migration).
    pub fn validate_name(name: &str) -> DomainResult<()> {
        if STOCK_AGENTS.contains(&name) {
            Ok(())
        } else {
            Err(DomainError::Validation(format!(
                "unknown agent name: {name} (must be one of the eight stock specialists)"
            )))
        }
    }

    /// M5 — instance_count is bounded so the orchestrator cannot fork without
    /// bound. Mirrors the SQL CHECK in migration 006.
    pub fn validate_instance_count(count: i64) -> DomainResult<()> {
        if !(0..=16).contains(&count) {
            return Err(DomainError::Validation(format!(
                "instance_count {count} out of range 0..=16"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_name_rejected() {
        let err = AgentSpec::validate_name("random-bot").unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn stock_names_accepted() {
        for n in STOCK_AGENTS {
            AgentSpec::validate_name(n).unwrap();
        }
    }

    #[test]
    fn instance_count_bounds() {
        AgentSpec::validate_instance_count(0).unwrap();
        AgentSpec::validate_instance_count(16).unwrap();
        assert!(AgentSpec::validate_instance_count(-1).is_err());
        assert!(AgentSpec::validate_instance_count(17).is_err());
    }
}
