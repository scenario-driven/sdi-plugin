use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub type RoundId = Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoundStatus {
    Planning,
    Active,
    Completed,
}

impl fmt::Display for RoundStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RoundStatus::Planning => "planning",
            RoundStatus::Active => "active",
            RoundStatus::Completed => "completed",
        })
    }
}

impl FromStr for RoundStatus {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "planning" => Ok(RoundStatus::Planning),
            "active" => Ok(RoundStatus::Active),
            "completed" => Ok(RoundStatus::Completed),
            other => Err(DomainError::Validation(format!(
                "unknown round status: {other}"
            ))),
        }
    }
}

/// D6 — strict-regression is the default; forward-only must be opted into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoundMode {
    #[default]
    StrictRegression,
    ForwardOnly,
}

impl fmt::Display for RoundMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RoundMode::StrictRegression => "strict-regression",
            RoundMode::ForwardOnly => "forward-only",
        })
    }
}

impl FromStr for RoundMode {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "strict-regression" => Ok(RoundMode::StrictRegression),
            // `additive` is the documented R1 word for "skip carry-over, only
            // new scenarios verify" — exactly forward-only's semantics. The
            // skill docs and CLI help advertised `additive`, but the daemon
            // enum only had `forward-only`, so a doc-following session was
            // rejected at R1 (#5). Accept it as an alias; canonical
            // serialization stays `forward-only`.
            "forward-only" | "additive" => Ok(RoundMode::ForwardOnly),
            other => Err(DomainError::InvalidRoundMode(other.to_string())),
        }
    }
}

/// D10 — In-flight task policy on round start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InFlightPolicy {
    /// Default: freeze in-flight tasks until user resolves.
    #[default]
    Pause,
    /// Explicit: cancel in-flight tasks immediately.
    Abort,
    /// Explicit: keep in-flight task running iff disjoint with new round's scenarios.
    ContinueOnNoimpact,
}

impl fmt::Display for InFlightPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            InFlightPolicy::Pause => "pause",
            InFlightPolicy::Abort => "abort",
            InFlightPolicy::ContinueOnNoimpact => "continue-on-noimpact",
        })
    }
}

impl FromStr for InFlightPolicy {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "pause" => Ok(InFlightPolicy::Pause),
            "abort" => Ok(InFlightPolicy::Abort),
            "continue-on-noimpact" => Ok(InFlightPolicy::ContinueOnNoimpact),
            other => Err(DomainError::Validation(format!(
                "unknown in-flight policy: {other}"
            ))),
        }
    }
}

/// D9 — disruption policy. `NeedsReview` is the safe default: a disrupted plan
/// cannot auto-promote scenario results; the user must confirm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DisruptionPolicy {
    #[default]
    NeedsReview,
    Auto,
}

impl fmt::Display for DisruptionPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DisruptionPolicy::NeedsReview => "needs-review",
            DisruptionPolicy::Auto => "auto",
        })
    }
}

impl FromStr for DisruptionPolicy {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "needs-review" => Ok(DisruptionPolicy::NeedsReview),
            "auto" => Ok(DisruptionPolicy::Auto),
            other => Err(DomainError::Validation(format!(
                "unknown disruption policy: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Round {
    pub id: RoundId,
    pub plan_id: Id,
    pub short_code: String,
    pub round_number: i64,
    pub mode: RoundMode,
    pub in_flight_policy: InFlightPolicy,
    pub disruption_policy: DisruptionPolicy,
    pub status: RoundStatus,
    /// D23 — pattern this round was produced under. NULL allowed for migrated
    /// rows; daemon write paths backfill `direct` at creation.
    #[serde(default)]
    pub produced_via_pattern_id: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub activated_at: Option<Timestamp>,
    pub completed_at: Option<Timestamp>,
}

impl Round {
    pub fn check_can_activate(&self) -> DomainResult<()> {
        if self.status != RoundStatus::Planning {
            return Err(DomainError::InvalidTransition {
                entity: "round".into(),
                from: self.status.to_string(),
                to: "active".into(),
            });
        }
        Ok(())
    }

    pub fn check_can_complete(&self) -> DomainResult<()> {
        if self.status != RoundStatus::Active {
            return Err(DomainError::InvalidTransition {
                entity: "round".into(),
                from: self.status.to_string(),
                to: "completed".into(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_mode_parses_canonical_and_additive_alias() {
        // Canonical values.
        assert_eq!(
            RoundMode::from_str("strict-regression").unwrap(),
            RoundMode::StrictRegression
        );
        assert_eq!(
            RoundMode::from_str("forward-only").unwrap(),
            RoundMode::ForwardOnly
        );
        // #5 — `additive` is the documented alias for forward-only; the daemon
        // used to reject it, breaking doc-following sessions at R1.
        assert_eq!(
            RoundMode::from_str("additive").unwrap(),
            RoundMode::ForwardOnly
        );
        // Serialization stays canonical (alias is input-only).
        assert_eq!(RoundMode::ForwardOnly.to_string(), "forward-only");
        // Unknown values still error.
        assert!(RoundMode::from_str("disruption").is_err());
    }
}
