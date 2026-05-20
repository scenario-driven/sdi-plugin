//! Disruption review — PRD §6 #4.
//!
//! When the LLM adds a new scenario / requirement / decision and detects that
//! existing scenarios are impacted, it MUST record a `DisruptionReview` row
//! enumerating those scenarios. A round cannot activate while any review on
//! that plan is still `pending` — the human must explicitly resolve each
//! review (retire / edit / keep) first.
//!
//! The daemon is intentionally policy-agnostic about *how* impacts are
//! detected: that is the LLM's job (semantic). The daemon's job is to hold
//! the queue, gate activation on it, and emit events when reviews move.
//!
//! Round-level `disruption_policy` (D9) does NOT bypass the gate — even
//! `auto` mode requires a human confirm. The policy only changes whether the
//! LLM auto-proposes a resolution at creation time.

use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub type DisruptionReviewId = Id;

/// What triggered the review (PRD §6 #4: scenario / requirement / decision).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisruptionSource {
    Scenario,
    Requirement,
    Decision,
}

impl fmt::Display for DisruptionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DisruptionSource::Scenario => "scenario",
            DisruptionSource::Requirement => "requirement",
            DisruptionSource::Decision => "decision",
        })
    }
}

impl FromStr for DisruptionSource {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "scenario" => Ok(DisruptionSource::Scenario),
            "requirement" => Ok(DisruptionSource::Requirement),
            "decision" => Ok(DisruptionSource::Decision),
            other => Err(DomainError::Validation(format!(
                "unknown disruption source: {other}",
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisruptionReviewStatus {
    Pending,
    Resolved,
}

impl fmt::Display for DisruptionReviewStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DisruptionReviewStatus::Pending => "pending",
            DisruptionReviewStatus::Resolved => "resolved",
        })
    }
}

impl FromStr for DisruptionReviewStatus {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "pending" => Ok(DisruptionReviewStatus::Pending),
            "resolved" => Ok(DisruptionReviewStatus::Resolved),
            other => Err(DomainError::Validation(format!(
                "unknown disruption review status: {other}",
            ))),
        }
    }
}

/// Human verdict on a disruption (PRD §4.3): retire impacted scenarios, edit
/// them, or keep them as-is. Optional; the human may resolve a review without
/// stamping a verdict if they simply acknowledged the impact list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisruptionResolution {
    Retire,
    Edit,
    Keep,
}

impl fmt::Display for DisruptionResolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DisruptionResolution::Retire => "retire",
            DisruptionResolution::Edit => "edit",
            DisruptionResolution::Keep => "keep",
        })
    }
}

impl FromStr for DisruptionResolution {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "retire" => Ok(DisruptionResolution::Retire),
            "edit" => Ok(DisruptionResolution::Edit),
            "keep" => Ok(DisruptionResolution::Keep),
            other => Err(DomainError::Validation(format!(
                "unknown disruption resolution: {other}",
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisruptionReview {
    pub id: DisruptionReviewId,
    pub plan_id: Id,
    pub source_kind: DisruptionSource,
    pub source_id: Id,
    /// IDs of existing scenarios the LLM flagged as impacted by `source_id`.
    /// MUST be non-empty (empty means "no impact" — the LLM should not record
    /// a review at all in that case).
    pub impacted_scenario_ids: Vec<Id>,
    pub status: DisruptionReviewStatus,
    pub resolution: Option<DisruptionResolution>,
    pub note: Option<String>,
    pub created_at: Timestamp,
    pub resolved_at: Option<Timestamp>,
}

impl DisruptionReview {
    pub fn validate_create(&self) -> DomainResult<()> {
        if self.impacted_scenario_ids.is_empty() {
            return Err(DomainError::Validation(
                "disruption review requires at least one impacted scenario".into(),
            ));
        }
        Ok(())
    }
}
