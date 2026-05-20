//! Collaboration surface: comments, questions, activity log.
//!
//! - **Comment**: polymorphic anchor on plan|task|scenario|round (exactly one).
//! - **Question**: single-answer Q&A on a plan; `open` → `answered`.
//! - **Activity**: append-only audit/timeline feed (distinct from SSE `events`,
//!   which is wire-level — activity carries human summaries).

use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

// -----------------------------------------------------------------------------
// Comments
// -----------------------------------------------------------------------------

pub type CommentId = Id;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: CommentId,
    pub project_id: Id,
    /// Exactly one of the four anchors is `Some`. Enforced by DB CHECK and the
    /// `Comment::new` constructor.
    pub plan_id: Option<Id>,
    pub task_id: Option<Id>,
    pub scenario_id: Option<Id>,
    pub round_id: Option<Id>,
    pub author: String,
    pub body: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentAnchor {
    Plan,
    Task,
    Scenario,
    Round,
}

impl Comment {
    /// Enforce the "exactly one anchor" invariant in the domain layer, so a
    /// caller can't accidentally bypass the DB CHECK (e.g. when seeding tests).
    pub fn validate_anchor(&self) -> DomainResult<CommentAnchor> {
        let set = [
            self.plan_id.is_some(),
            self.task_id.is_some(),
            self.scenario_id.is_some(),
            self.round_id.is_some(),
        ];
        let count = set.iter().filter(|b| **b).count();
        if count != 1 {
            return Err(DomainError::Validation(format!(
                "comment must anchor on exactly one of plan|task|scenario|round (got {count})"
            )));
        }
        Ok(match (self.plan_id.is_some(), self.task_id.is_some(), self.scenario_id.is_some()) {
            (true, _, _) => CommentAnchor::Plan,
            (_, true, _) => CommentAnchor::Task,
            (_, _, true) => CommentAnchor::Scenario,
            _ => CommentAnchor::Round,
        })
    }
}

// -----------------------------------------------------------------------------
// Questions
// -----------------------------------------------------------------------------

pub type QuestionId = Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionStatus {
    Open,
    Answered,
}

impl fmt::Display for QuestionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            QuestionStatus::Open => "open",
            QuestionStatus::Answered => "answered",
        })
    }
}

impl FromStr for QuestionStatus {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "open" => Ok(Self::Open),
            "answered" => Ok(Self::Answered),
            other => Err(DomainError::Validation(format!("unknown question status: {other}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    pub id: QuestionId,
    pub plan_id: Id,
    pub asker: String,
    pub body: String,
    pub answer: Option<String>,
    pub answered_by: Option<String>,
    pub answered_at: Option<Timestamp>,
    pub status: QuestionStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

// -----------------------------------------------------------------------------
// Activity
// -----------------------------------------------------------------------------

pub type ActivityId = Id;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub id: ActivityId,
    pub project_id: Id,
    pub actor: String,
    /// Dotted kind, mirrors SSE event kinds: `plan.approved`, `task.completed`,
    /// `round.completed`, `decision.created`, etc. Free-form so future surfaces
    /// can add new kinds without a migration.
    pub kind: String,
    pub entity_id: Option<Id>,
    pub summary: String,
    pub payload: serde_json::Value,
    pub created_at: Timestamp,
}
