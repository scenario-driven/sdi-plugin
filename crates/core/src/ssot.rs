//! L0 product-definition graph (PRD-v2 D32). The SSoT `kind` is open-ended
//! (Persona / Capability / Domain / Concept / Invariant / …) over one uniform
//! node table. Completeness (D34) has two axes the deterministic verify judges:
//! facet completeness (no unresolved OPEN marker) and link completeness (no
//! dangling edge — representable because `SsotEdge.to_ref` is a logical ref).

use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub type SsotNodeId = Id;
pub type SsotEdgeId = Id;

/// Governance-axis trust level carried from the ssot-studio model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    #[default]
    Unverified,
    Inferred,
    High,
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Confidence::Unverified => "unverified",
            Confidence::Inferred => "inferred",
            Confidence::High => "high",
        })
    }
}

impl FromStr for Confidence {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "unverified" => Ok(Confidence::Unverified),
            "inferred" => Ok(Confidence::Inferred),
            "high" => Ok(Confidence::High),
            other => Err(DomainError::Validation(format!(
                "unknown confidence: {other}"
            ))),
        }
    }
}

/// One unresolved blank in a node's facets. Promoted to a blocking decision
/// request (D35) — `question_id` links the `DecisionQuestion` that fills it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenMarker {
    pub id: String,
    pub field: String,
    pub description: String,
    #[serde(default)]
    pub question_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsotNode {
    pub id: SsotNodeId,
    pub project_id: Id,
    pub short_code: String,
    /// Persona | Capability | Domain | Concept | Invariant | … (open-ended).
    pub kind: String,
    pub title: String,
    /// 4-axis facets (business / domain / system / governance) as raw JSON for
    /// additive evolution; `parse_facets` validates it parses to an object.
    #[serde(default = "default_facets_json")]
    pub facets_json: String,
    /// `[OpenMarker]` — non-empty means facet-incomplete (D34).
    #[serde(default = "default_json_array")]
    pub open_markers_json: String,
    #[serde(default)]
    pub confidence: Confidence,
    /// D23 — provenance. NULL for migrated rows; write paths backfill `direct`.
    #[serde(default)]
    pub produced_via_pattern_id: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// L0 graph edge. `to_ref` is a logical reference (node id or short_code), NOT a
/// hard FK, so an unresolved (dangling) edge is representable and verify can flag it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsotEdge {
    pub id: SsotEdgeId,
    pub project_id: Id,
    pub from_node: Id,
    pub to_ref: String,
    pub rel: String,
    pub created_at: Timestamp,
}

fn default_facets_json() -> String {
    "{}".to_string()
}

fn default_json_array() -> String {
    "[]".to_string()
}

/// A facet value "counts" as filled when it carries content: a non-blank
/// string, a non-empty array/object, or any non-null scalar.
fn facet_value_filled(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => false,
        serde_json::Value::String(s) => !s.trim().is_empty(),
        serde_json::Value::Array(a) => !a.is_empty(),
        serde_json::Value::Object(o) => !o.is_empty(),
        _ => true,
    }
}

impl SsotNode {
    /// `kind` and `title` must be non-empty.
    pub fn validate_header(kind: &str, title: &str) -> DomainResult<()> {
        if kind.trim().is_empty() {
            return Err(DomainError::Validation("ssot node kind must be non-empty".into()));
        }
        if title.trim().is_empty() {
            return Err(DomainError::Validation(
                "ssot node title must be non-empty".into(),
            ));
        }
        Ok(())
    }

    /// `facets_json` must parse to a JSON object.
    pub fn parse_facets(s: &str) -> DomainResult<serde_json::Map<String, serde_json::Value>> {
        let v: serde_json::Value = serde_json::from_str(s)
            .map_err(|e| DomainError::Validation(format!("facets_json parse error: {e}")))?;
        match v {
            serde_json::Value::Object(m) => Ok(m),
            _ => Err(DomainError::Validation("facets_json must be an object".into())),
        }
    }

    /// `open_markers_json` must parse to an array of `OpenMarker`.
    pub fn parse_open_markers(s: &str) -> DomainResult<Vec<OpenMarker>> {
        serde_json::from_str(s)
            .map_err(|e| DomainError::Validation(format!("open_markers_json parse error: {e}")))
    }

    /// Per-kind required facets as `axis.field` dotted paths (PRD-v2 D32, the
    /// four facet axes business / domain / system / governance). A node is not
    /// facet-complete until each is present and non-empty — the L0 "sufficiency"
    /// floor, so a title-only node that states nothing about why it exists is
    /// caught instead of passing as complete. Intentionally a primary-axis
    /// minimum per kind; extend as the oracle model deepens. An unknown kind
    /// carries no floor (open-ended `kind`, see module header).
    pub fn required_facets(kind: &str) -> &'static [&'static str] {
        match kind {
            "Persona" | "Capability" | "Screen" | "Endpoint" | "SystemComponent"
            | "Integration" | "Platform" => &["business.purpose"],
            "Domain" | "Concept" | "Invariant" | "Decision" => &["domain.definition"],
            _ => &[],
        }
    }

    /// The required `axis.field` paths this node is still missing (absent or
    /// empty). Empty ⇒ the node meets its kind's facet floor. Malformed
    /// `facets_json` counts every requirement as missing.
    pub fn missing_required_facets(&self) -> Vec<String> {
        let map = Self::parse_facets(&self.facets_json).ok();
        Self::required_facets(&self.kind)
            .iter()
            .filter(|path| {
                let Some((axis, field)) = path.split_once('.') else {
                    return true;
                };
                let filled = map
                    .as_ref()
                    .and_then(|m| m.get(axis))
                    .and_then(|v| v.get(field))
                    .map(facet_value_filled)
                    .unwrap_or(false);
                !filled
            })
            .map(|s| s.to_string())
            .collect()
    }

    /// D34 facet completeness — no unresolved OPEN marker AND every required
    /// facet for the node's kind is filled.
    pub fn is_facet_complete(&self) -> bool {
        Self::parse_open_markers(&self.open_markers_json)
            .map(|m| m.is_empty())
            .unwrap_or(false)
            && self.missing_required_facets().is_empty()
    }

    /// D35 answer→compile — remove the OPEN marker `marker_id`, returning the new
    /// markers JSON. Idempotent: removing an absent id leaves the set unchanged.
    pub fn remove_open_marker(open_markers_json: &str, marker_id: &str) -> DomainResult<String> {
        let mut markers = Self::parse_open_markers(open_markers_json)?;
        markers.retain(|m| m.id != marker_id);
        serde_json::to_string(&markers)
            .map_err(|e| DomainError::Validation(format!("encode open_markers: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{now, Id, IdKind};

    fn node(kind: &str, facets_json: &str, markers_json: &str) -> SsotNode {
        SsotNode {
            id: Id::new(IdKind::SsotNode),
            project_id: Id::new(IdKind::SsotNode),
            short_code: "SN-x".into(),
            kind: kind.into(),
            title: "t".into(),
            facets_json: facets_json.into(),
            open_markers_json: markers_json.into(),
            confidence: Confidence::Unverified,
            produced_via_pattern_id: None,
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn confidence_roundtrip() {
        for c in ["unverified", "inferred", "high"] {
            let parsed: Confidence = c.parse().unwrap();
            assert_eq!(parsed.to_string(), c);
        }
        assert!("bogus".parse::<Confidence>().is_err());
        assert_eq!(Confidence::default(), Confidence::Unverified);
    }

    #[test]
    fn header_validation() {
        SsotNode::validate_header("Persona", "Power user").unwrap();
        assert!(SsotNode::validate_header("", "t").is_err());
        assert!(SsotNode::validate_header("Persona", "  ").is_err());
    }

    #[test]
    fn facets_must_be_object() {
        SsotNode::parse_facets(r#"{"business":{"purpose":"x"}}"#).unwrap();
        assert!(SsotNode::parse_facets(r#"[1,2]"#).is_err());
    }

    #[test]
    fn open_markers_drive_facet_completeness() {
        let markers = SsotNode::parse_open_markers(
            r#"[{"id":"m1","field":"purpose","description":"확정 필요"}]"#,
        )
        .unwrap();
        assert_eq!(markers.len(), 1);
        assert!(SsotNode::parse_open_markers("[]").unwrap().is_empty());
    }

    #[test]
    fn remove_open_marker_closes_and_is_idempotent() {
        let src = r#"[{"id":"m1","field":"purpose","description":"a"},{"id":"m2","field":"def","description":"b"}]"#;
        let after = SsotNode::remove_open_marker(src, "m1").unwrap();
        let left = SsotNode::parse_open_markers(&after).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, "m2");
        // removing an absent id is a no-op
        let again = SsotNode::remove_open_marker(&after, "ghost").unwrap();
        assert_eq!(SsotNode::parse_open_markers(&again).unwrap().len(), 1);
    }

    #[test]
    fn required_facets_per_kind() {
        assert_eq!(SsotNode::required_facets("Persona"), ["business.purpose"]);
        assert_eq!(SsotNode::required_facets("Capability"), ["business.purpose"]);
        assert_eq!(SsotNode::required_facets("Domain"), ["domain.definition"]);
        assert_eq!(SsotNode::required_facets("Invariant"), ["domain.definition"]);
        // open-ended / unknown kind carries no facet floor
        assert!(SsotNode::required_facets("Gizmo").is_empty());
    }

    #[test]
    fn facet_floor_drives_completeness() {
        // title-only persona is missing its business.purpose floor
        let bare = node("Persona", "{}", "[]");
        assert_eq!(bare.missing_required_facets(), vec!["business.purpose"]);
        assert!(!bare.is_facet_complete());

        // a blank string does not count as filled
        let blank = node("Persona", r#"{"business":{"purpose":"  "}}"#, "[]");
        assert_eq!(blank.missing_required_facets(), vec!["business.purpose"]);

        // filled purpose + no open markers ⇒ complete
        let filled = node("Persona", r#"{"business":{"purpose":"결제하려는 사용자"}}"#, "[]");
        assert!(filled.missing_required_facets().is_empty());
        assert!(filled.is_facet_complete());

        // still incomplete while an OPEN marker is unresolved, even with facets
        let marked = node(
            "Persona",
            r#"{"business":{"purpose":"x"}}"#,
            r#"[{"id":"m1","field":"purpose","description":"확인"}]"#,
        );
        assert!(!marked.is_facet_complete());

        // unknown kind has no floor, so a bare node is complete
        assert!(node("Gizmo", "{}", "[]").is_facet_complete());
    }
}
