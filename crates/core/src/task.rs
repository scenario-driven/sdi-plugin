use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use crate::scenario::ScenarioResult;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

pub type TaskId = Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    InProgress,
    Done,
    Cancelled,
    Blocked,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TaskStatus::Todo => "todo",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Done => "done",
            TaskStatus::Cancelled => "cancelled",
            TaskStatus::Blocked => "blocked",
        })
    }
}

impl FromStr for TaskStatus {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "todo" => Ok(TaskStatus::Todo),
            "in_progress" => Ok(TaskStatus::InProgress),
            "done" => Ok(TaskStatus::Done),
            "cancelled" => Ok(TaskStatus::Cancelled),
            "blocked" => Ok(TaskStatus::Blocked),
            other => Err(DomainError::Validation(format!(
                "unknown task status: {other}"
            ))),
        }
    }
}

/// One scenario's verdict + evidence reference. Lives inside TaskEvidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioEvidence {
    pub scenario_id: Id,
    pub result: ScenarioResult,
    /// Free-form reference: file:line, URL, log path, etc.
    pub evidence_ref: String,
    pub note: Option<String>,
}

/// Structured evidence (PRD §6.6). Required on `done` transition.
///
/// Fields are `serde(default)` so the daemon can accept partial payloads
/// (including `{}`) and route them through the domain `validate_for_done`
/// gate, which returns `EVIDENCE_REQUIRED` — a far better client error than
/// axum's generic 422 JSON-deserialization rejection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskEvidence {
    #[serde(default)]
    pub scenarios: Vec<ScenarioEvidence>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub extras: BTreeMap<String, String>,
}

impl TaskEvidence {
    pub fn is_empty(&self) -> bool {
        self.scenarios.is_empty() && self.summary.is_none() && self.extras.is_empty()
    }

    /// Evidence must cover at least one scenario, and every covered scenario must have a non-empty ref.
    pub fn validate_for_done(&self) -> DomainResult<()> {
        if self.is_empty() {
            return Err(DomainError::EvidenceRequired);
        }
        if self.scenarios.is_empty() && self.summary.is_none() {
            return Err(DomainError::EvidenceRequired);
        }
        for entry in &self.scenarios {
            if entry.evidence_ref.trim().is_empty() {
                return Err(DomainError::Validation(format!(
                    "evidence_ref empty for scenario {}",
                    entry.scenario_id
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub round_id: Id,
    pub short_code: String,
    pub description: String,
    pub status: TaskStatus,
    pub parent_scenario_ids: Vec<Id>,
    pub parent_requirement_ids: Vec<Id>,
    pub evidence: Option<TaskEvidence>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub evidence_at: Option<Timestamp>,
}

impl Task {
    pub fn check_transition(
        &self,
        target: TaskStatus,
        evidence: Option<&TaskEvidence>,
    ) -> DomainResult<()> {
        use TaskStatus::*;
        let ok = matches!(
            (self.status, target),
            (Todo, InProgress)
                | (Todo, Cancelled)
                | (Todo, Blocked)
                | (InProgress, Done)
                | (InProgress, Cancelled)
                | (InProgress, Blocked)
                | (Blocked, Todo)
                | (Blocked, InProgress)
                | (Blocked, Cancelled)
        );
        if !ok {
            return Err(DomainError::InvalidTransition {
                entity: "task".into(),
                from: self.status.to_string(),
                to: target.to_string(),
            });
        }
        if target == TaskStatus::Done {
            let ev = evidence.ok_or(DomainError::EvidenceRequired)?;
            ev.validate_for_done()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::ScenarioResult;

    fn sample_task() -> Task {
        Task {
            id: Id::from("TASK-X"),
            round_id: Id::from("ROUND-X"),
            short_code: "SDI-1".into(),
            description: "x".into(),
            status: TaskStatus::InProgress,
            parent_scenario_ids: vec![],
            parent_requirement_ids: vec![],
            evidence: None,
            created_at: crate::ids::now(),
            updated_at: crate::ids::now(),
            evidence_at: None,
        }
    }

    #[test]
    fn done_without_evidence_rejected() {
        let t = sample_task();
        let err = t.check_transition(TaskStatus::Done, None).unwrap_err();
        assert!(matches!(err, DomainError::EvidenceRequired));
    }

    #[test]
    fn done_with_evidence_accepted() {
        let t = sample_task();
        let ev = TaskEvidence {
            scenarios: vec![ScenarioEvidence {
                scenario_id: Id::from("SCN-1"),
                result: ScenarioResult::Passing,
                evidence_ref: "src/lib.rs:42".into(),
                note: None,
            }],
            summary: Some("ok".into()),
            extras: Default::default(),
        };
        t.check_transition(TaskStatus::Done, Some(&ev)).unwrap();
    }

    #[test]
    fn invalid_transition_rejected() {
        let mut t = sample_task();
        t.status = TaskStatus::Done;
        let err = t
            .check_transition(TaskStatus::InProgress, None)
            .unwrap_err();
        assert!(matches!(err, DomainError::InvalidTransition { .. }));
    }
}
