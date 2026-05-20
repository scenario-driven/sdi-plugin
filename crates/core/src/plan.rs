use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub type PlanId = Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanStatus {
    Draft,
    Active,
    Completed,
}

impl fmt::Display for PlanStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PlanStatus::Draft => "draft",
            PlanStatus::Active => "active",
            PlanStatus::Completed => "completed",
        })
    }
}

impl FromStr for PlanStatus {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "draft" => Ok(PlanStatus::Draft),
            "active" => Ok(PlanStatus::Active),
            "completed" => Ok(PlanStatus::Completed),
            other => Err(DomainError::Validation(format!("unknown plan status: {other}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: PlanId,
    pub project_id: Id,
    pub short_code: String,
    pub title: String,
    /// Markdown body — SNAPSHOT-ONLY per D12 (overwritten in place).
    pub body: String,
    pub status: PlanStatus,
    /// Increments on each update — optimistic concurrency.
    pub version: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub approved_at: Option<Timestamp>,
    pub completed_at: Option<Timestamp>,
}

impl Plan {
    /// D8 — approve gate: status must be draft and the plan must have at least
    /// one confirmed scenario (callsite asserts the latter and passes the bool).
    pub fn check_can_approve(&self, has_confirmed_scenarios: bool) -> DomainResult<()> {
        if self.status != PlanStatus::Draft {
            return Err(DomainError::InvalidTransition {
                entity: "plan".into(),
                from: self.status.to_string(),
                to: "active".into(),
            });
        }
        if !has_confirmed_scenarios {
            return Err(DomainError::ScenariosRequired);
        }
        Ok(())
    }

    pub fn check_can_complete(&self) -> DomainResult<()> {
        if self.status != PlanStatus::Active {
            return Err(DomainError::InvalidTransition {
                entity: "plan".into(),
                from: self.status.to_string(),
                to: "completed".into(),
            });
        }
        Ok(())
    }
}
