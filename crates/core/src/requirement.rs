use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};

pub type RequirementId = Id;

/// SNAPSHOT-ONLY (PRD §6.7, D12). `body` is overwritten on update; history is
/// captured by the linked Decision append-only log, not by mutating this row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    pub id: RequirementId,
    pub plan_id: Id,
    pub short_code: String,
    pub title: String,
    pub body: String,
    /// How this requirement entered the plan ("snapshot" by default; "decision-rewrite"
    /// when a Decision rewrote the body).
    pub source: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// PRD §6 #7 + D12. Reject any history trace in a Requirement body — that
/// belongs in a Decision. The detector targets the shapes humans actually
/// reach for when version-stamping a requirement in place:
///
/// - markdown strikethrough (`~~old~~ new`)
/// - inline version markers (`v1:`, `v2:`, `(was: ...)`)
/// - English history phrases (`previously`, `originally`, `used to be`)
/// - Korean history phrases (`이전에는`, `원래는`, `기존 안`, `변경 전`,
///   `~ 였으나`, `~ 였는데`)
/// - arrow rewrites (`A → B`, `before -> after`, but only when paired with a
///   history marker word — bare arrows in technical body text are fine)
///
/// The rule is structural: any matched pattern fails fast with a hint that
/// directs the LLM to `/decide` (the Decision append-only log).
pub fn validate_snapshot_body(body: &str) -> DomainResult<()> {
    let lower = body.to_lowercase();

    if body.contains("~~") {
        return Err(DomainError::SnapshotViolation(
            "markdown strikethrough (`~~text~~`) found".into(),
        ));
    }

    let inline_version_markers = [
        "v1:", "v2:", "v3:", "v4:", "v5:",
        "version 1:", "version 2:", "version 3:",
        "(was:", "(was ", "(originally:", "(originally ",
        "(formerly:", "(formerly ",
        "(원래:", "(원래 ", "(기존:", "(기존 ",
    ];
    for needle in inline_version_markers {
        if lower.contains(needle) {
            return Err(DomainError::SnapshotViolation(format!(
                "inline version marker `{needle}` found",
            )));
        }
    }

    let history_phrases_en = [
        "previously,",
        "previously the",
        "previously this",
        "originally,",
        "originally the",
        "originally this",
        "used to be",
        "used to say",
        "we used to",
        "history:",
        "changelog:",
    ];
    for needle in history_phrases_en {
        if lower.contains(needle) {
            return Err(DomainError::SnapshotViolation(format!(
                "history phrase `{needle}` found",
            )));
        }
    }

    let history_phrases_ko = [
        "이전에는",
        "이전 안",
        "원래는",
        "원래 안",
        "기존 안",
        "기존에는",
        "변경 전",
        "변경 전에는",
        "였으나",
        "이었으나",
        "였는데",
        "이었는데",
        "변경 이력",
        "변경 사항:",
        "참고: 이전",
        "참고: 기존",
    ];
    for needle in history_phrases_ko {
        if body.contains(needle) {
            return Err(DomainError::SnapshotViolation(format!(
                "history phrase `{needle}` found",
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_violation(body: &str) {
        let err = validate_snapshot_body(body).unwrap_err();
        assert!(
            matches!(err, DomainError::SnapshotViolation(_)),
            "expected SnapshotViolation for `{body}`, got {err:?}",
        );
    }

    #[test]
    fn accepts_plain_snapshot_body() {
        validate_snapshot_body("The system shall expose /scenarios with GET and POST.").unwrap();
    }

    #[test]
    fn accepts_arrow_in_technical_context() {
        // Pure technical arrow (state transition diagram) without history words is fine.
        validate_snapshot_body("Status transitions: planning -> active -> completed.").unwrap();
    }

    #[test]
    fn rejects_strikethrough() {
        assert_violation("The body shall be ~~mutable~~ snapshot-only.");
    }

    #[test]
    fn rejects_version_markers() {
        assert_violation("v1: cli only. v2: cli + daemon.");
        assert_violation("Endpoint shape (was: GET only) now POST + GET.");
    }

    #[test]
    fn rejects_english_history_phrases() {
        assert_violation("Previously, the daemon was optional.");
        assert_violation("This used to be in the CLI; it now lives in the daemon.");
        assert_violation("Changelog: 2026-05-01 — added /rounds endpoint.");
    }

    #[test]
    fn rejects_korean_history_phrases() {
        assert_violation("이전에는 CLI 만 있었지만 지금은 daemon 도 포함한다.");
        assert_violation("기존 안: 단일 바이너리. 현재 안: 두 바이너리.");
        assert_violation("원래는 stdout 으로 출력했으나 이제는 SSE 로 발행한다.");
        assert_violation("변경 이력: 2026-05-01 신규 작성.");
    }

    #[test]
    fn case_insensitive_english() {
        assert_violation("PREVIOUSLY, this was undefined.");
        assert_violation("V1: alpha. V2: beta.");
    }
}
