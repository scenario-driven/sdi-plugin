//! Decision-question engine (PRD-v2 D35). OPEN markers are promoted to
//! SA-exam-style blocking decision requests: a fully-contextualized stem,
//! explained options, an LLM recommendation, and a `+@` free-text fallback.
//! `qtype` encodes the elimination-first split — `fact` (1 survivor → auto-decided
//! with rationale) vs `preference` (2+ survivors → genuine user choice).

use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub type DecisionQuestionId = Id;
pub type QuestionOptionId = Id;
pub type QuestionAnswerId = Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionType {
    /// Best-practice / architecture — elimination leaves 1 survivor.
    Fact,
    /// UX / domain / business — 2+ genuine survivors, user's call.
    Preference,
}

impl fmt::Display for QuestionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            QuestionType::Fact => "fact",
            QuestionType::Preference => "preference",
        })
    }
}

impl FromStr for QuestionType {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "fact" => Ok(QuestionType::Fact),
            "preference" => Ok(QuestionType::Preference),
            other => Err(DomainError::Validation(format!("unknown question type: {other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionStatus {
    Open,
    Answered,
    /// fact-type resolved by the engine (1 survivor) — recorded with rationale.
    AutoDecided,
}

impl fmt::Display for QuestionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            QuestionStatus::Open => "open",
            QuestionStatus::Answered => "answered",
            QuestionStatus::AutoDecided => "auto_decided",
        })
    }
}

impl FromStr for QuestionStatus {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "open" => Ok(QuestionStatus::Open),
            "answered" => Ok(QuestionStatus::Answered),
            "auto_decided" => Ok(QuestionStatus::AutoDecided),
            other => Err(DomainError::Validation(format!(
                "unknown question status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionQuestion {
    pub id: DecisionQuestionId,
    pub project_id: Id,
    pub short_code: String,
    /// node / flow / open-marker this question fills.
    #[serde(default)]
    pub scope_ref: Option<String>,
    pub qtype: QuestionType,
    /// SA-exam stem — fully-contextualized scenario the decision sits in.
    pub context_md: String,
    /// Adaptive branching — an answer can unlock follow-up questions.
    #[serde(default)]
    pub parent_question_id: Option<String>,
    pub status: QuestionStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub id: QuestionOptionId,
    pub question_id: Id,
    pub label: String,
    #[serde(default)]
    pub body_md: String,
    /// Why this option is more / less correct — the elimination rationale.
    #[serde(default)]
    pub rationale_md: String,
    #[serde(default)]
    pub is_llm_recommended: bool,
    #[serde(default)]
    pub idx: i64,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionAnswer {
    pub id: QuestionAnswerId,
    pub question_id: Id,
    #[serde(default)]
    pub chosen_option_id: Option<String>,
    /// `+@` subjective fallback.
    #[serde(default)]
    pub free_text: Option<String>,
    #[serde(default = "default_answered_by")]
    pub answered_by: String,
    /// Entity ids this answer produced — provenance (D23 / D35).
    #[serde(default = "default_json_array")]
    pub generated_refs_json: String,
    pub created_at: Timestamp,
}

fn default_answered_by() -> String {
    "user".to_string()
}

fn default_json_array() -> String {
    "[]".to_string()
}

impl DecisionQuestion {
    pub fn validate_context(context_md: &str) -> DomainResult<()> {
        if context_md.trim().is_empty() {
            return Err(DomainError::Validation(
                "decision question context_md must be non-empty".into(),
            ));
        }
        Ok(())
    }
}

impl QuestionAnswer {
    /// D35 — an answer must either pick an option or supply free-text (`+@`).
    pub fn validate(
        chosen_option_id: &Option<String>,
        free_text: &Option<String>,
    ) -> DomainResult<()> {
        let has_choice = chosen_option_id
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty());
        let has_text = free_text.as_ref().is_some_and(|s| !s.trim().is_empty());
        if !has_choice && !has_text {
            return Err(DomainError::Validation(
                "answer must pick an option or provide free_text (+@)".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qtype_roundtrip() {
        for s in ["fact", "preference"] {
            let parsed: QuestionType = s.parse().unwrap();
            assert_eq!(parsed.to_string(), s);
        }
        assert!("bogus".parse::<QuestionType>().is_err());
    }

    #[test]
    fn status_roundtrip() {
        for s in ["open", "answered", "auto_decided"] {
            let parsed: QuestionStatus = s.parse().unwrap();
            assert_eq!(parsed.to_string(), s);
        }
        assert!("bogus".parse::<QuestionStatus>().is_err());
    }

    #[test]
    fn context_must_be_non_empty() {
        DecisionQuestion::validate_context("페르소나 X가 결제 실패 시 …").unwrap();
        assert!(DecisionQuestion::validate_context("  ").is_err());
    }

    #[test]
    fn answer_requires_choice_or_text() {
        QuestionAnswer::validate(&Some("QOPT-1".into()), &None).unwrap();
        QuestionAnswer::validate(&None, &Some("내 방식".into())).unwrap();
        assert!(QuestionAnswer::validate(&None, &None).is_err());
        assert!(QuestionAnswer::validate(&Some("  ".into()), &Some("  ".into())).is_err());
    }
}
