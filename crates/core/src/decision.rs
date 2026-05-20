use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub type DecisionId = Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecisionStatus {
    Proposed,
    Accepted,
    Superseded,
}

impl fmt::Display for DecisionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DecisionStatus::Proposed => "proposed",
            DecisionStatus::Accepted => "accepted",
            DecisionStatus::Superseded => "superseded",
        })
    }
}

impl FromStr for DecisionStatus {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "proposed" => Ok(DecisionStatus::Proposed),
            "accepted" => Ok(DecisionStatus::Accepted),
            "superseded" => Ok(DecisionStatus::Superseded),
            other => Err(DomainError::Validation(format!("unknown decision status: {other}"))),
        }
    }
}

/// Append-only ADR (D12). Decisions are immutable; later decisions may set
/// `supersedes_id` to chain a replacement and the prior row flips to
/// `superseded` status — the original body is preserved as written.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub id: DecisionId,
    pub plan_id: Id,
    pub short_code: String,
    pub title: String,
    pub body: String,
    pub status: DecisionStatus,
    pub supersedes_id: Option<Id>,
    pub created_at: Timestamp,
}
