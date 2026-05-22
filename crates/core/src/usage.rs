//! Token / tool-call accounting. Each `UsageRecord` is a run-level emission
//! (typically one per `runs` row). Aggregation lives in the daemon (read-side
//! rollup) — this module only defines the shape and validation.

use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub type UsageRecordId = Id;

/// Cost tier the run was charged at. Mirrors `tasks.tier`; advisory in v3,
/// hard-enforced in v4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UsageTier {
    Low,
    #[default]
    Med,
    High,
}

impl fmt::Display for UsageTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UsageTier::Low => "low",
            UsageTier::Med => "med",
            UsageTier::High => "high",
        })
    }
}

impl FromStr for UsageTier {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "low" => Ok(Self::Low),
            "med" => Ok(Self::Med),
            "high" => Ok(Self::High),
            other => Err(DomainError::Validation(format!("unknown usage tier: {other}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub id: UsageRecordId,
    pub project_id: Id,
    pub plan_id: Option<Id>,
    pub task_id: Option<Id>,
    pub run_id: Option<Id>,
    pub model: String,
    pub tier: UsageTier,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    pub tool_calls: i64,
    pub cost_usd: f64,
    pub created_at: Timestamp,
}

/// Roll-up over a set of records (plan-scoped, task-scoped, etc.). Returned by
/// the daemon's `/plans/:id/usage` and `/tasks/:id/usage/preflight`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageRollup {
    pub records: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    pub tool_calls: i64,
    pub cost_usd: f64,
}

impl UsageRollup {
    pub fn add(&mut self, r: &UsageRecord) {
        self.records += 1;
        self.input_tokens += r.input_tokens;
        self.output_tokens += r.output_tokens;
        self.cache_read += r.cache_read;
        self.cache_write += r.cache_write;
        self.tool_calls += r.tool_calls;
        self.cost_usd += r.cost_usd;
    }
}
