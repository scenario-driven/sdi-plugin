//! Task runtime entities: `Run` (execution attempt), `TaskRelation`
//! (decomposition/cross-references), `TaskLease` (single-writer guard).
//!
//! Run vs. status: a task carries a domain status (`todo` / `in_progress` /
//! `done`...) and emits one or more **Runs** during its lifetime — each Run
//! is a single execution attempt by an actor. Multiple Runs are normal
//! (retries, fan-out subagents). Status transitions are independent.

use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

// ---------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------

pub type RunId = Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunResult {
    Success,
    Failure,
    Aborted,
}

impl fmt::Display for RunResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RunResult::Success => "success",
            RunResult::Failure => "failure",
            RunResult::Aborted => "aborted",
        })
    }
}

impl FromStr for RunResult {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "success" => Ok(Self::Success),
            "failure" => Ok(Self::Failure),
            "aborted" => Ok(Self::Aborted),
            other => Err(DomainError::Validation(format!(
                "unknown run result: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: RunId,
    pub task_id: Id,
    pub actor: String,
    pub session_id: Option<String>,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    pub result: Option<RunResult>,
    pub notes: Option<String>,
    pub payload: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Task relations (decomposition + advisory cross-links)
// ---------------------------------------------------------------------------

pub type TaskRelationId = Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationKind {
    Parent,
    Blocks,
    DependsOn,
    Duplicates,
    Related,
}

impl fmt::Display for RelationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RelationKind::Parent => "parent",
            RelationKind::Blocks => "blocks",
            RelationKind::DependsOn => "depends-on",
            RelationKind::Duplicates => "duplicates",
            RelationKind::Related => "related",
        })
    }
}

impl FromStr for RelationKind {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "parent" => Ok(Self::Parent),
            "blocks" => Ok(Self::Blocks),
            "depends-on" => Ok(Self::DependsOn),
            "duplicates" => Ok(Self::Duplicates),
            "related" => Ok(Self::Related),
            other => Err(DomainError::Validation(format!(
                "unknown relation kind: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRelation {
    pub id: TaskRelationId,
    pub parent_id: Id,
    pub child_id: Id,
    pub kind: RelationKind,
    pub created_at: Timestamp,
}

// ---------------------------------------------------------------------------
// Lease (single-writer guard)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskLease {
    pub task_id: Id,
    pub holder: String,
    pub acquired_at: Timestamp,
    pub heartbeat_at: Timestamp,
    pub expires_at: Timestamp,
}
