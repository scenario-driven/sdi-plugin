//! L1 UserFlow (PRD-v2 D33) — one persona × one purpose, the complete service
//! journey on a finished-service basis. This tier is the reference the outer
//! (spec-convergence) loop drives toward; L1 completeness (D34) = every
//! (Persona × Capability) covered by ≥1 confirmed flow.

use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub type UserFlowId = Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlowStatus {
    Draft,
    Confirmed,
}

impl fmt::Display for FlowStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FlowStatus::Draft => "draft",
            FlowStatus::Confirmed => "confirmed",
        })
    }
}

impl FromStr for FlowStatus {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "draft" => Ok(FlowStatus::Draft),
            "confirmed" => Ok(FlowStatus::Confirmed),
            other => Err(DomainError::Validation(format!(
                "unknown flow status: {other}"
            ))),
        }
    }
}

/// One ordered step of the journey.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowStep {
    pub idx: u32,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFlow {
    pub id: UserFlowId,
    pub project_id: Id,
    pub short_code: String,
    /// SsotNode (kind=Persona) this journey belongs to.
    pub persona_id: Id,
    pub purpose: String,
    /// `[FlowStep]` — the finished-service journey, raw JSON for additive evolution.
    #[serde(default = "default_json_array")]
    pub steps_json: String,
    /// `[ssot_node_id]` (kind=Capability) this flow covers — drives L1 coverage (D34).
    #[serde(default = "default_json_array")]
    pub covers_capabilities_json: String,
    pub status: FlowStatus,
    #[serde(default)]
    pub produced_via_pattern_id: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

fn default_json_array() -> String {
    "[]".to_string()
}

impl UserFlow {
    pub fn validate_purpose(purpose: &str) -> DomainResult<()> {
        if purpose.trim().is_empty() {
            return Err(DomainError::Validation(
                "user_flow purpose must be non-empty".into(),
            ));
        }
        Ok(())
    }

    pub fn parse_steps(s: &str) -> DomainResult<Vec<FlowStep>> {
        serde_json::from_str(s)
            .map_err(|e| DomainError::Validation(format!("steps_json parse error: {e}")))
    }

    pub fn parse_covers_capabilities(s: &str) -> DomainResult<Vec<String>> {
        serde_json::from_str(s).map_err(|e| {
            DomainError::Validation(format!("covers_capabilities_json parse error: {e}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_roundtrip() {
        for s in ["draft", "confirmed"] {
            let parsed: FlowStatus = s.parse().unwrap();
            assert_eq!(parsed.to_string(), s);
        }
        assert!("bogus".parse::<FlowStatus>().is_err());
    }

    #[test]
    fn purpose_must_be_non_empty() {
        UserFlow::validate_purpose("결제를 완료한다").unwrap();
        assert!(UserFlow::validate_purpose("   ").is_err());
    }

    #[test]
    fn steps_and_capabilities_parse() {
        let steps = UserFlow::parse_steps(
            r#"[{"idx":0,"description":"로그인"},{"idx":1,"description":"결제"}]"#,
        )
        .unwrap();
        assert_eq!(steps.len(), 2);
        let caps = UserFlow::parse_covers_capabilities(r#"["SN-abc","SN-def"]"#).unwrap();
        assert_eq!(caps.len(), 2);
    }
}
