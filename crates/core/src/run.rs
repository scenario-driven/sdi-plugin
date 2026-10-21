//! Task runtime entities: `Run` (execution attempt), `TaskRelation`
//! (decomposition/cross-references), `TaskLease` (single-writer guard).
//!
//! Run vs. status: a task carries a domain status (`todo` / `in_progress` /
//! `done`...) and emits one or more **Runs** during its lifetime — each Run
//! is a single execution attempt by an actor. Multiple Runs are normal
//! (retries, fan-out subagents). Status transitions are independent.

use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use crate::verification::{FailureClass, SpawnError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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

/// One execution attempt. A plain agent run carries only the task link and
/// the terminal result; a **verification run** (`sdi run verify`) is bound to
/// a scenario and additionally records what it executed (command, cwd), what
/// it consumed (input globs, tool versions → input fingerprint), how it ended
/// (exit code, signal, timeout, spawn error), the captured report and its
/// digest, and the failure classification + signature that feed the retry
/// budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: RunId,
    /// Task the run executed under. Optional for verification runs, which are
    /// scenario-scoped (a reuse check needs no task).
    #[serde(default)]
    pub task_id: Option<Id>,
    #[serde(default)]
    pub plan_id: Option<Id>,
    #[serde(default)]
    pub round_id: Option<Id>,
    #[serde(default)]
    pub scenario_id: Option<Id>,
    pub actor: String,
    pub session_id: Option<String>,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    pub result: Option<RunResult>,
    pub notes: Option<String>,
    pub payload: serde_json::Value,
    /// argv of the verification command.
    #[serde(default)]
    pub command: Option<Vec<String>>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// Declared input path globs (relative to `cwd`); empty = whole repository.
    #[serde(default)]
    pub input_globs: Vec<String>,
    /// Caller-declared tool versions (`name → version`), part of the fingerprint.
    #[serde(default)]
    pub tool_versions: BTreeMap<String, String>,
    /// `sha256:<hex>` over contract + command + tool versions + input contents,
    /// computed by the daemon when the run starts.
    #[serde(default)]
    pub input_fingerprint: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub signal: Option<i32>,
    #[serde(default)]
    pub timed_out: bool,
    #[serde(default)]
    pub spawn_error: Option<SpawnError>,
    /// Captured stdout+stderr, under the daemon's state directory.
    #[serde(default)]
    pub report_path: Option<String>,
    #[serde(default)]
    pub report_sha256: Option<String>,
    #[serde(default)]
    pub failure_class: Option<FailureClass>,
    #[serde(default)]
    pub failure_signature: Option<String>,
    #[serde(default)]
    pub error_class: Option<String>,
    #[serde(default)]
    pub failing_test: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<i64>,
}

impl Run {
    /// A finished run whose process exited 0 and that the daemon recorded as
    /// a success — the only kind that can evidence a `passing` verdict.
    pub fn is_clean_pass(&self) -> bool {
        self.finished_at.is_some()
            && self.result == Some(RunResult::Success)
            && self.exit_code == Some(0)
            && self.signal.is_none()
            && !self.timed_out
            && self.spawn_error.is_none()
    }
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
