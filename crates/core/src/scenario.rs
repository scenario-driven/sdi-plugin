use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use crate::pattern::ClaimStatus;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub type ScenarioId = Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScenarioStatus {
    Draft,
    Confirmed,
}

impl fmt::Display for ScenarioStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ScenarioStatus::Draft => "draft",
            ScenarioStatus::Confirmed => "confirmed",
        })
    }
}

impl FromStr for ScenarioStatus {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "draft" => Ok(ScenarioStatus::Draft),
            "confirmed" => Ok(ScenarioStatus::Confirmed),
            other => Err(DomainError::Validation(format!(
                "unknown scenario status: {other}"
            ))),
        }
    }
}

/// Per-round outcome of one scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScenarioResult {
    Passing,
    Failing,
    Impacted,
    Retired,
}

impl fmt::Display for ScenarioResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ScenarioResult::Passing => "passing",
            ScenarioResult::Failing => "failing",
            ScenarioResult::Impacted => "impacted",
            ScenarioResult::Retired => "retired",
        })
    }
}

impl FromStr for ScenarioResult {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "passing" => Ok(ScenarioResult::Passing),
            "failing" => Ok(ScenarioResult::Failing),
            "impacted" => Ok(ScenarioResult::Impacted),
            "retired" => Ok(ScenarioResult::Retired),
            other => Err(DomainError::Validation(format!(
                "unknown scenario result: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub id: ScenarioId,
    pub plan_id: Id,
    pub short_code: String,
    pub given: String,
    /// `when` is a Rust keyword; serialized as "when".
    #[serde(rename = "when")]
    pub when_clause: String,
    #[serde(rename = "then")]
    pub then_clause: String,
    pub tags: Vec<String>,
    pub origin_round_id: Option<Id>,
    pub status: ScenarioStatus,
    /// M4 contract — DAG predecessor edges (Scenario short_codes).
    pub depends_on: Vec<String>,
    /// M4 contract — agent responsible for satisfying this scenario.
    pub produced_by: Option<String>,
    /// M4 contract — agent responsible for verifying this scenario.
    pub verified_by: Option<String>,
    /// D29 — JSON array of path globs this scenario claims while active.
    /// Stored as raw JSON string for additive evolution; `validate_claimed_resources`
    /// parses + validates. Defaults to `"[]"`.
    #[serde(default = "default_claimed_resources_json")]
    pub claimed_resources_json: String,
    /// D29 — multi-session resource claim status.
    #[serde(default = "default_claim_status")]
    pub claim_status: ClaimStatus,
    /// D23 — pattern this scenario was produced under. NULL allowed for
    /// migrated rows; daemon write paths backfill `direct` at creation.
    #[serde(default)]
    pub produced_via_pattern_id: Option<String>,
    /// #8 — retirement marker. `Some` ⇒ the scenario is retired (excluded from
    /// verification / carry-over / approve count) while its history is kept.
    /// Reversible: un-retire sets it back to `None`. Orthogonal to `status`,
    /// which is preserved across retire/un-retire.
    #[serde(default)]
    pub retired_at: Option<Timestamp>,
    /// D33 (PRD-v2) — the UserFlow whose step this DetailScenario verifies.
    /// NULL for legacy / unanchored scenarios.
    #[serde(default)]
    pub belongs_to_flow_id: Option<String>,
    /// D33 — which step of that flow (FlowStep `idx` as a string) this covers.
    #[serde(default)]
    pub covers_flow_step: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

fn default_claimed_resources_json() -> String {
    "[]".to_string()
}

fn default_claim_status() -> ClaimStatus {
    ClaimStatus::None
}

impl Scenario {
    /// D5 — GWT strict. Every field non-empty after trimming.
    pub fn validate_gwt(given: &str, when_clause: &str, then_clause: &str) -> DomainResult<()> {
        if given.trim().is_empty() {
            return Err(DomainError::GwtEmpty { field: "given" });
        }
        if when_clause.trim().is_empty() {
            return Err(DomainError::GwtEmpty { field: "when" });
        }
        if then_clause.trim().is_empty() {
            return Err(DomainError::GwtEmpty { field: "then" });
        }
        Ok(())
    }

    /// M4 — depends_on must not reference the scenario's own short_code
    /// (immediate cycle). Full transitive cycle detection lives in the
    /// regression-runner where the per-plan DAG is materialized.
    pub fn validate_depends_on(short_code: &str, depends_on: &[String]) -> DomainResult<()> {
        if depends_on.iter().any(|d| d == short_code) {
            return Err(DomainError::Validation(format!(
                "scenario {short_code} depends_on cannot include itself"
            )));
        }
        Ok(())
    }
}

/// D29 — parse + validate a stored `claimed_resources_json` blob. Returns the
/// glob list. Validation rules: must parse to a JSON array of strings; every
/// entry must be non-empty after trim; entries that contain only path
/// separators / glob wildcards are rejected (must carry at least one literal
/// character so the daemon claim ledger can compare overlaps meaningfully).
pub fn validate_claimed_resources(s: &str) -> DomainResult<Vec<String>> {
    let parsed: Vec<String> = serde_json::from_str(s)
        .map_err(|e| DomainError::Validation(format!("claimed_resources parse error: {e}")))?;
    for entry in &parsed {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            return Err(DomainError::Validation(
                "claimed_resources entry must be non-empty".into(),
            ));
        }
        // Reject globs that have no literal anchor at all (e.g. "**", "*",
        // "**/*") — they would match every file and defeat overlap detection.
        let has_literal = trimmed.chars().any(|c| !matches!(c, '*' | '?' | '/' | '.'));
        if !has_literal {
            return Err(DomainError::Validation(format!(
                "claimed_resources entry has no literal path component: {entry}"
            )));
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gwt_empty_given_rejected() {
        let err = Scenario::validate_gwt("", "act", "result").unwrap_err();
        assert!(matches!(err, DomainError::GwtEmpty { field: "given" }));
    }

    #[test]
    fn gwt_whitespace_when_rejected() {
        let err = Scenario::validate_gwt("ctx", "   ", "result").unwrap_err();
        assert!(matches!(err, DomainError::GwtEmpty { field: "when" }));
    }

    #[test]
    fn gwt_all_present_passes() {
        Scenario::validate_gwt("ctx", "act", "result").unwrap();
    }

    #[test]
    fn depends_on_rejects_self_reference() {
        let err = Scenario::validate_depends_on("US-A-001", &["US-A-001".into()]).unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn depends_on_allows_other_short_codes() {
        Scenario::validate_depends_on("US-A-002", &["US-A-001".into()]).unwrap();
    }

    #[test]
    fn claimed_resources_parses_array_of_globs() {
        let v =
            validate_claimed_resources(r#"["crates/db/migrations/*.sql","plugin/agents/*.md"]"#)
                .unwrap();
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn claimed_resources_rejects_empty_entry() {
        let err = validate_claimed_resources(r#"["",""]"#).unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn claimed_resources_rejects_wildcard_only() {
        let err = validate_claimed_resources(r#"["**"]"#).unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn claimed_resources_rejects_non_array() {
        let err = validate_claimed_resources(r#"{"a":1}"#).unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }
}
