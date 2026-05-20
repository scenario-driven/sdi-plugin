use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub type KnowledgeId = Id;

/// PRD §5.4 — only `rag`-scoped knowledge is exposed via the MCP server.
/// `reference` (user-owned docs) and `archive` (deprecated) stay local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KnowledgeScope {
    #[default]
    Rag,
    Reference,
    Archive,
}

impl fmt::Display for KnowledgeScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            KnowledgeScope::Rag => "rag",
            KnowledgeScope::Reference => "reference",
            KnowledgeScope::Archive => "archive",
        })
    }
}

impl FromStr for KnowledgeScope {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "rag" => Ok(KnowledgeScope::Rag),
            "reference" => Ok(KnowledgeScope::Reference),
            "archive" => Ok(KnowledgeScope::Archive),
            other => Err(DomainError::Validation(format!("unknown knowledge scope: {other}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Knowledge {
    pub id: KnowledgeId,
    pub project_id: Id,
    pub scope: KnowledgeScope,
    /// Free-form classification: "decision" | "runbook" | "note" | "adr" | ...
    pub kind: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    /// Optional source pointer (file:line, URL, commit SHA).
    pub source_ref: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
