//! M1 Blackboard — AgentNote: append-only journal for asynchronous
//! agent-to-agent communication. Notes are scoped to plan / round / scenario
//! / task, classified by kind, and never deleted (retire via `retired_at`).

use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub type AgentNoteId = Id;

/// Anchor of the note. Exactly one of (plan / round / scenario / task) id is
/// set; mirrors the SQL CHECK in migration 006.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentNoteScope {
    Plan,
    Round,
    Scenario,
    Task,
}

impl fmt::Display for AgentNoteScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AgentNoteScope::Plan => "plan",
            AgentNoteScope::Round => "round",
            AgentNoteScope::Scenario => "scenario",
            AgentNoteScope::Task => "task",
        })
    }
}

impl FromStr for AgentNoteScope {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "plan" => Ok(AgentNoteScope::Plan),
            "round" => Ok(AgentNoteScope::Round),
            "scenario" => Ok(AgentNoteScope::Scenario),
            "task" => Ok(AgentNoteScope::Task),
            other => Err(DomainError::Validation(format!(
                "unknown agent_note scope: {other}"
            ))),
        }
    }
}

/// Note semantics. `handoff` is the only kind that requires `to_agent`.
/// `dissent` triggers M3 escalation (per D20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentNoteKind {
    Hypothesis,
    Observation,
    Question,
    Dissent,
    Evidence,
    Handoff,
}

impl fmt::Display for AgentNoteKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AgentNoteKind::Hypothesis => "hypothesis",
            AgentNoteKind::Observation => "observation",
            AgentNoteKind::Question => "question",
            AgentNoteKind::Dissent => "dissent",
            AgentNoteKind::Evidence => "evidence",
            AgentNoteKind::Handoff => "handoff",
        })
    }
}

impl FromStr for AgentNoteKind {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "hypothesis" => Ok(AgentNoteKind::Hypothesis),
            "observation" => Ok(AgentNoteKind::Observation),
            "question" => Ok(AgentNoteKind::Question),
            "dissent" => Ok(AgentNoteKind::Dissent),
            "evidence" => Ok(AgentNoteKind::Evidence),
            "handoff" => Ok(AgentNoteKind::Handoff),
            other => Err(DomainError::Validation(format!(
                "unknown agent_note kind: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNote {
    pub id: AgentNoteId,
    pub project_id: Id,
    pub plan_id: Option<Id>,
    pub round_id: Option<Id>,
    pub scenario_id: Option<Id>,
    pub task_id: Option<Id>,
    pub scope_kind: AgentNoteScope,
    pub kind: AgentNoteKind,
    pub from_agent: String,
    pub to_agent: Option<String>,
    pub body: String,
    pub payload: serde_json::Value,
    pub receipt_acknowledged_at: Option<Timestamp>,
    pub retired_at: Option<Timestamp>,
    pub retired_reason: Option<String>,
    pub created_at: Timestamp,
}

impl AgentNote {
    /// Anchor validation — exactly one id must be set and must match scope_kind.
    pub fn validate_anchor(
        scope_kind: AgentNoteScope,
        plan_id: Option<&Id>,
        round_id: Option<&Id>,
        scenario_id: Option<&Id>,
        task_id: Option<&Id>,
    ) -> DomainResult<()> {
        let bits = [plan_id, round_id, scenario_id, task_id]
            .iter()
            .filter(|o| o.is_some())
            .count();
        if bits != 1 {
            return Err(DomainError::Validation(format!(
                "agent_note requires exactly one anchor id, got {bits}"
            )));
        }
        let ok = match scope_kind {
            AgentNoteScope::Plan => plan_id.is_some(),
            AgentNoteScope::Round => round_id.is_some(),
            AgentNoteScope::Scenario => scenario_id.is_some(),
            AgentNoteScope::Task => task_id.is_some(),
        };
        if !ok {
            return Err(DomainError::Validation(format!(
                "agent_note anchor id must match scope_kind={scope_kind}"
            )));
        }
        Ok(())
    }

    /// M2 — `kind=handoff` requires a `to_agent`. Mirrors SQL CHECK.
    pub fn validate_handoff(kind: AgentNoteKind, to_agent: Option<&str>) -> DomainResult<()> {
        if matches!(kind, AgentNoteKind::Handoff)
            && to_agent.map(str::is_empty).unwrap_or(true)
        {
            return Err(DomainError::Validation(
                "handoff note requires non-empty to_agent".into(),
            ));
        }
        Ok(())
    }

    /// True when the note is still considered active on the blackboard
    /// (M1 retains retired notes for audit but excludes them from active read).
    pub fn is_active(&self) -> bool {
        self.retired_at.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> Id {
        s.into()
    }

    #[test]
    fn anchor_requires_exactly_one_id() {
        let err = AgentNote::validate_anchor(AgentNoteScope::Plan, None, None, None, None)
            .unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
        let err = AgentNote::validate_anchor(
            AgentNoteScope::Plan,
            Some(&id("PLAN-x")),
            Some(&id("ROUND-y")),
            None,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn anchor_id_must_match_scope() {
        let err = AgentNote::validate_anchor(
            AgentNoteScope::Plan,
            None,
            Some(&id("ROUND-y")),
            None,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn handoff_requires_to_agent() {
        let err = AgentNote::validate_handoff(AgentNoteKind::Handoff, None).unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
        AgentNote::validate_handoff(AgentNoteKind::Handoff, Some("impl-coder")).unwrap();
        AgentNote::validate_handoff(AgentNoteKind::Observation, None).unwrap();
    }
}
