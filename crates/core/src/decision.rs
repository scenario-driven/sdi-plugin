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

/// D20 — M3 four-stage negotiation. A consensus row may only land after at
/// least one critique row has been written against the same `proposal_id`;
/// dissensus escalates mode-agnostically (always 4xx user gate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecisionKind {
    Proposal,
    Critique,
    Consensus,
    Dissensus,
}

impl fmt::Display for DecisionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DecisionKind::Proposal => "proposal",
            DecisionKind::Critique => "critique",
            DecisionKind::Consensus => "consensus",
            DecisionKind::Dissensus => "dissensus",
        })
    }
}

impl FromStr for DecisionKind {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "proposal" => Ok(DecisionKind::Proposal),
            "critique" => Ok(DecisionKind::Critique),
            "consensus" => Ok(DecisionKind::Consensus),
            "dissensus" => Ok(DecisionKind::Dissensus),
            other => Err(DomainError::Validation(format!(
                "unknown decision kind: {other}"
            ))),
        }
    }
}

/// Append-only ADR (D12). Decisions are immutable; later decisions may set
/// `supersedes_id` to chain a replacement and the prior row flips to
/// `superseded` status — the original body is preserved as written.
///
/// v0.4 extensions (D20):
/// - `kind` classifies the stage in the M3 negotiation flow.
/// - `proposal_id` links critique / consensus / dissensus rows back to the
///   originating proposal so the daemon can enforce the M3 ordering invariant.
/// - `agent_name` records which Layer-2 specialist emitted the row.
/// - `escalated_at` is set when dissensus surfaces to a human user gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub id: DecisionId,
    pub plan_id: Id,
    pub short_code: String,
    pub title: String,
    pub body: String,
    pub status: DecisionStatus,
    pub supersedes_id: Option<Id>,
    pub kind: DecisionKind,
    pub proposal_id: Option<Id>,
    pub agent_name: Option<String>,
    pub escalated_at: Option<Timestamp>,
    pub created_at: Timestamp,
}

impl Decision {
    /// D20 — M3 enforcement. `existing_kinds` lists every Decision.kind already
    /// recorded against `proposal_id` (in any order). Returns Err when the
    /// next stage would violate `proposal → critique → consensus | dissensus`.
    pub fn validate_stage_transition(
        next: DecisionKind,
        existing_kinds: &[DecisionKind],
        proposal_id: Option<&Id>,
    ) -> DomainResult<()> {
        match next {
            DecisionKind::Proposal => {
                if proposal_id.is_some() {
                    return Err(DomainError::Validation(
                        "proposal decision must not carry proposal_id".into(),
                    ));
                }
                Ok(())
            }
            DecisionKind::Critique | DecisionKind::Consensus | DecisionKind::Dissensus => {
                if proposal_id.is_none() {
                    return Err(DomainError::Validation(format!(
                        "{next} decision requires proposal_id"
                    )));
                }
                if matches!(next, DecisionKind::Consensus) {
                    let has_critique = existing_kinds
                        .iter()
                        .any(|k| matches!(k, DecisionKind::Critique));
                    if !has_critique {
                        return Err(DomainError::ConsensusMissingCritique {
                            proposal_id: proposal_id.unwrap().to_string(),
                        });
                    }
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pid() -> Id {
        "DEC-proposal".into()
    }

    #[test]
    fn proposal_must_not_have_proposal_id() {
        let err =
            Decision::validate_stage_transition(DecisionKind::Proposal, &[], Some(&pid()))
                .unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn critique_requires_proposal_id() {
        let err =
            Decision::validate_stage_transition(DecisionKind::Critique, &[], None).unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn consensus_blocked_without_critique() {
        let err =
            Decision::validate_stage_transition(DecisionKind::Consensus, &[], Some(&pid()))
                .unwrap_err();
        assert!(matches!(err, DomainError::ConsensusMissingCritique { .. }));
    }

    #[test]
    fn consensus_allowed_after_critique() {
        Decision::validate_stage_transition(
            DecisionKind::Consensus,
            &[DecisionKind::Critique],
            Some(&pid()),
        )
        .unwrap();
    }

    #[test]
    fn dissensus_allowed_with_proposal_id() {
        Decision::validate_stage_transition(
            DecisionKind::Dissensus,
            &[DecisionKind::Critique],
            Some(&pid()),
        )
        .unwrap();
    }
}
