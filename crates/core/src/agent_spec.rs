//! Layer-2 specialist sub-agents registered as first-class records so the
//! orchestrator (Layer 1) can spawn/recall instances at runtime (M5).
//!
//! v0.5 adds three meta-specialists for pattern enforcement + reversibility
//! (D22~D29): `pattern-orchestrator`, `pattern-critic`, `reversal-runner`.
//! Stance is part of the uniqueness tuple (D26 sybil-fix).

use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use crate::pattern::Stance;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub type AgentSpecId = Id;

/// Canonical specialist names enumerated by D-7 (v0.4 set).
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

/// D22~D29 meta-specialists added in v0.5 — name + default stance.
/// Seeders insert these in addition to `STOCK_AGENTS` (stance=Neutral).
pub const STOCK_META_AGENTS: &[(&str, Stance)] = &[
    ("pattern-orchestrator", Stance::Proposer),
    ("pattern-critic", Stance::DevilAdvocate),
    ("reversal-runner", Stance::Neutral),
];

/// AgentSpec lifecycle status. `Active` rows are dispatch candidates; `Expired`
/// rows are kept as audit history (D26 consensus gates never look at expired).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentSpecStatus {
    Active,
    Expired,
}

impl fmt::Display for AgentSpecStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AgentSpecStatus::Active => "active",
            AgentSpecStatus::Expired => "expired",
        })
    }
}

impl FromStr for AgentSpecStatus {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "active" => Ok(AgentSpecStatus::Active),
            "expired" => Ok(AgentSpecStatus::Expired),
            other => Err(DomainError::Validation(format!(
                "unknown agent_spec status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub id: AgentSpecId,
    pub name: String,
    /// D26 — distinctness tuple for consensus gates is `(name, stance)`.
    #[serde(default = "default_stance")]
    pub stance: Stance,
    pub role: String,
    pub system_prompt: String,
    pub instance_count: i64,
    /// Who/what created this row (e.g. `seed`, `user:<id>`, `agent:<name>`).
    #[serde(default)]
    pub created_by: Option<String>,
    /// When a spec was authored under a specific plan (e.g. plan-scoped
    /// reviewer), the plan id; else NULL.
    #[serde(default)]
    pub origin_plan_id: Option<Id>,
    /// JSON-encoded allowlist of tools this agent may invoke. NULL = inherit.
    #[serde(default)]
    pub tool_allowlist_json: Option<String>,
    /// JSON-encoded list of decision-kinds this agent may emit / approve.
    /// `["*"]` means all kinds (used by reversal-runner).
    #[serde(default)]
    pub decision_kinds_json: Option<String>,
    /// JSON-encoded rules mapping decision-kind → blast_radius_score
    /// adjustments. NULL falls back to global defaults.
    #[serde(default)]
    pub blast_radius_rules_json: Option<String>,
    /// D26 — Active vs Expired (audit-only).
    #[serde(default = "default_status")]
    pub status: AgentSpecStatus,
    /// Optional expiry timestamp. NULL = perpetual.
    #[serde(default)]
    pub expires_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

fn default_stance() -> Stance {
    Stance::Neutral
}

fn default_status() -> AgentSpecStatus {
    AgentSpecStatus::Active
}

impl AgentSpec {
    /// Accept either the canonical eight (`STOCK_AGENTS`) or the three v0.5
    /// meta-specialists (`STOCK_META_AGENTS`). M5 forbids dynamic role
    /// invention outside these two lists.
    pub fn validate_name(name: &str) -> DomainResult<()> {
        if STOCK_AGENTS.contains(&name) || STOCK_META_AGENTS.iter().any(|(n, _)| *n == name) {
            Ok(())
        } else {
            Err(DomainError::Validation(format!(
                "unknown agent name: {name} (must be a stock or meta specialist)"
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
    fn meta_specialist_names_accepted() {
        for (n, _) in STOCK_META_AGENTS {
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
