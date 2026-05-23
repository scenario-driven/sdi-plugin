//! D22 — CollaborationPattern: the seventh first-class entity.
//!
//! A pattern describes how a group of agents will produce one work entity
//! (plan / requirement / scenario / task / decision / round). Five kinds:
//!
//! - `Workflow` — ordered steps; D26 shape requires `len(steps) ≥ 2`.
//! - `Graph` — peer reviewers; D26 shape requires distinct
//!   `(AgentSpec.name, stance)` tuples ≥ 2 (sybil-blocked).
//! - `Swarm` — fan-out spawning; D26 shape requires `len(fan_out) ≥ 2`.
//! - `AgentsAsTools` — caller→callee peer registration; D26 shape requires
//!   `len(peers) ≥ 1`.
//! - `Direct` — solo-flow marker (anti-pattern badge per D23); always L3.
//!
//! Lifecycle: `Pending → Active → Converged | Dissensus | Aborted`. The shape
//! gate runs on `Pending → Active` (D27). Reversibility (D28) lives next to
//! the consensus Decision, not the pattern itself, but the L5 unlock formula
//! depends on the pattern shape passing here.

use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

pub type CollaborationPatternId = Id;

// ─── enums ───────────────────────────────────────────────────────────────

/// D22 — pattern kind. `direct` is the solo-flow marker (D23 anti-pattern),
/// not an escape hatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PatternKind {
    Workflow,
    Graph,
    Swarm,
    AgentsAsTools,
    Direct,
}

impl PatternKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PatternKind::Workflow => "workflow",
            PatternKind::Graph => "graph",
            PatternKind::Swarm => "swarm",
            PatternKind::AgentsAsTools => "agents-as-tools",
            PatternKind::Direct => "direct",
        }
    }
}

impl fmt::Display for PatternKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PatternKind {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "workflow" => Ok(PatternKind::Workflow),
            "graph" => Ok(PatternKind::Graph),
            "swarm" => Ok(PatternKind::Swarm),
            "agents-as-tools" => Ok(PatternKind::AgentsAsTools),
            "direct" => Ok(PatternKind::Direct),
            other => Err(DomainError::Validation(format!(
                "unknown pattern kind: {other}"
            ))),
        }
    }
}

/// Which work entity the pattern produces. Mirrors the SQL CHECK in 007.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppliesTo {
    Plan,
    Requirement,
    Scenario,
    Task,
    Decision,
    Round,
}

impl AppliesTo {
    pub fn as_str(self) -> &'static str {
        match self {
            AppliesTo::Plan => "plan",
            AppliesTo::Requirement => "requirement",
            AppliesTo::Scenario => "scenario",
            AppliesTo::Task => "task",
            AppliesTo::Decision => "decision",
            AppliesTo::Round => "round",
        }
    }
}

impl fmt::Display for AppliesTo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AppliesTo {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "plan" => Ok(AppliesTo::Plan),
            "requirement" => Ok(AppliesTo::Requirement),
            "scenario" => Ok(AppliesTo::Scenario),
            "task" => Ok(AppliesTo::Task),
            "decision" => Ok(AppliesTo::Decision),
            "round" => Ok(AppliesTo::Round),
            other => Err(DomainError::Validation(format!(
                "unknown applies_to: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PatternLifecycle {
    Pending,
    Active,
    Converged,
    Dissensus,
    Aborted,
}

impl PatternLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            PatternLifecycle::Pending => "pending",
            PatternLifecycle::Active => "active",
            PatternLifecycle::Converged => "converged",
            PatternLifecycle::Dissensus => "dissensus",
            PatternLifecycle::Aborted => "aborted",
        }
    }

    /// `converged`, `dissensus`, `aborted` are terminal; the row's
    /// `decided_at` should be stamped on transition.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            PatternLifecycle::Converged
                | PatternLifecycle::Dissensus
                | PatternLifecycle::Aborted
        )
    }
}

impl fmt::Display for PatternLifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PatternLifecycle {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "pending" => Ok(PatternLifecycle::Pending),
            "active" => Ok(PatternLifecycle::Active),
            "converged" => Ok(PatternLifecycle::Converged),
            "dissensus" => Ok(PatternLifecycle::Dissensus),
            "aborted" => Ok(PatternLifecycle::Aborted),
            other => Err(DomainError::Validation(format!(
                "unknown pattern lifecycle: {other}"
            ))),
        }
    }
}

/// D26 — AgentSpec.stance enum. `Neutral` is the default for legacy / stock
/// rows; consensus distinctness counts `(name, stance)` tuples, so the same
/// `impl-coder/proposer` + `impl-coder/devil_advocate` pair satisfies the
/// graph gate while two `impl-coder/neutral` instances do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    Proposer,
    DevilAdvocate,
    SchemaGuardian,
    PerformanceReviewer,
    SecurityReviewer,
    Neutral,
}

impl Stance {
    pub fn as_str(self) -> &'static str {
        match self {
            Stance::Proposer => "proposer",
            Stance::DevilAdvocate => "devil_advocate",
            Stance::SchemaGuardian => "schema_guardian",
            Stance::PerformanceReviewer => "performance_reviewer",
            Stance::SecurityReviewer => "security_reviewer",
            Stance::Neutral => "neutral",
        }
    }
}

impl fmt::Display for Stance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Stance {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "proposer" => Ok(Stance::Proposer),
            "devil_advocate" => Ok(Stance::DevilAdvocate),
            "schema_guardian" => Ok(Stance::SchemaGuardian),
            "performance_reviewer" => Ok(Stance::PerformanceReviewer),
            "security_reviewer" => Ok(Stance::SecurityReviewer),
            "neutral" => Ok(Stance::Neutral),
            other => Err(DomainError::Validation(format!("unknown stance: {other}"))),
        }
    }
}

/// D29 — Scenario.claim_status enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaimStatus {
    None,
    Requested,
    Active,
    Released,
}

impl ClaimStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ClaimStatus::None => "none",
            ClaimStatus::Requested => "requested",
            ClaimStatus::Active => "active",
            ClaimStatus::Released => "released",
        }
    }
}

impl fmt::Display for ClaimStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ClaimStatus {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "none" => Ok(ClaimStatus::None),
            "requested" => Ok(ClaimStatus::Requested),
            "active" => Ok(ClaimStatus::Active),
            "released" => Ok(ClaimStatus::Released),
            other => Err(DomainError::Validation(format!(
                "unknown claim_status: {other}"
            ))),
        }
    }
}

// ─── manifest payloads ──────────────────────────────────────────────────

/// One workflow step. `idx` is 0-based ordinal; `agent` is the AgentSpec.name
/// (orchestrator resolves stance at dispatch time).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternStep {
    pub idx: usize,
    pub agent: String,
    pub action: String,
}

/// A graph reviewer entry. Distinctness for the D26 sybil-block is
/// `(name, stance)`; this struct carries both.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Reviewer {
    pub name: String,
    pub stance: Stance,
}

/// An agents-as-tools registration entry: `caller` exposes `callee` as a tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerLink {
    pub caller: String,
    pub callee: String,
}

// ─── core entity ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationPattern {
    pub id: CollaborationPatternId,
    pub short_code: String,
    pub plan_id: Id,
    pub kind: PatternKind,
    pub applies_to: AppliesTo,
    pub scope_id: String,
    pub parent_pattern_id: Option<Id>,
    pub depth: i64,
    pub lifecycle: PatternLifecycle,
    pub steps: Option<Vec<PatternStep>>,
    pub reviewers: Option<Vec<Reviewer>>,
    pub fan_out: Option<Vec<String>>,
    pub peer_registration: Option<Vec<PeerLink>>,
    pub decided_at: Option<Timestamp>,
    pub decided_reason: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

// ─── shape validation (D26) ──────────────────────────────────────────────

/// D26 — shape gate run at `Pending → Active`. Each non-direct kind has a
/// minimum manifest size; `Direct` always passes (its cost is the L3 cap, not
/// a shape check).
///
/// `steps`, `reviewers`, `fan_out`, `peers` may be `None` when the caller has
/// not populated the relevant field for the kind — the gate treats `None` as
/// empty for the kind that owns it, and ignores the rest.
pub fn validate_pattern_shape(
    kind: PatternKind,
    steps: Option<&[PatternStep]>,
    reviewers: Option<&[Reviewer]>,
    fan_out: Option<&[String]>,
    peers: Option<&[PeerLink]>,
) -> DomainResult<()> {
    match kind {
        PatternKind::Workflow => {
            let n = steps.map(|s| s.len()).unwrap_or(0);
            if n < 2 {
                return Err(DomainError::Validation(format!(
                    "workflow shape gate: steps ≥ 2 required (got {n})"
                )));
            }
        }
        PatternKind::Graph => {
            let rs = reviewers.unwrap_or(&[]);
            // (name, stance) tuple distinctness; same name with same stance is sybil.
            let mut seen: HashSet<(String, Stance)> = HashSet::new();
            for r in rs {
                seen.insert((r.name.clone(), r.stance));
            }
            if seen.len() < 2 {
                return Err(DomainError::Validation(format!(
                    "graph shape gate: distinct (name, stance) tuples ≥ 2 required (got {})",
                    seen.len()
                )));
            }
        }
        PatternKind::Swarm => {
            let n = fan_out.map(|f| f.len()).unwrap_or(0);
            if n < 2 {
                return Err(DomainError::Validation(format!(
                    "swarm shape gate: fan_out ≥ 2 required (got {n})"
                )));
            }
        }
        PatternKind::AgentsAsTools => {
            let n = peers.map(|p| p.len()).unwrap_or(0);
            if n < 1 {
                return Err(DomainError::Validation(format!(
                    "agents-as-tools shape gate: peer registration ≥ 1 required (got {n})"
                )));
            }
        }
        PatternKind::Direct => {}
    }
    Ok(())
}

// ─── D28 reversal plan ──────────────────────────────────────────────────

/// D28 — Decision.reversal_plan discriminated union. Per-type required fields
/// enforced by `validate_reversal_plan_json`; the daemon's reversal-runner
/// specialist dispatches by `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReversalPlan {
    /// Inverse SQL migration + DAG of migration files this depends on.
    MigrationSql {
        sql: String,
        dependencies: Vec<String>,
    },
    /// `git revert <sha>` SHA pointing at the commit to undo.
    GitRevert { sha: String },
    /// Filesystem snapshot reference (e.g. tarball path or repo state ref).
    FsSnapshot { snapshot_ref: String },
    /// Compensating action against an external system (free-form JSON spec).
    CompensatingAction {
        action_spec: serde_json::Value,
    },
}

/// D28 — parse + validate a stored reversal_plan JSON blob. Returns the typed
/// enum or a domain error describing which required field is missing /
/// malformed. The L5 auto-apply gate is allowed only when this returns Ok and
/// `blast_radius_score ≤ AutonomyPolicy.l5_threshold`.
pub fn validate_reversal_plan_json(s: &str) -> DomainResult<ReversalPlan> {
    let plan: ReversalPlan = serde_json::from_str(s).map_err(|e| {
        DomainError::Validation(format!("reversal_plan parse error: {e}"))
    })?;
    match &plan {
        ReversalPlan::MigrationSql { sql, .. } if sql.trim().is_empty() => {
            return Err(DomainError::Validation(
                "reversal_plan migration_sql.sql must be non-empty".into(),
            ));
        }
        ReversalPlan::GitRevert { sha } if sha.trim().is_empty() => {
            return Err(DomainError::Validation(
                "reversal_plan git_revert.sha must be non-empty".into(),
            ));
        }
        ReversalPlan::FsSnapshot { snapshot_ref } if snapshot_ref.trim().is_empty() => {
            return Err(DomainError::Validation(
                "reversal_plan fs_snapshot.snapshot_ref must be non-empty".into(),
            ));
        }
        ReversalPlan::CompensatingAction { action_spec }
            if action_spec.is_null() || matches!(action_spec, serde_json::Value::Object(o) if o.is_empty()) =>
        {
            return Err(DomainError::Validation(
                "reversal_plan compensating_action.action_spec must be non-empty".into(),
            ));
        }
        _ => {}
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reviewer(name: &str, stance: Stance) -> Reviewer {
        Reviewer {
            name: name.into(),
            stance,
        }
    }

    fn step(idx: usize, agent: &str) -> PatternStep {
        PatternStep {
            idx,
            agent: agent.into(),
            action: "act".into(),
        }
    }

    #[test]
    fn workflow_requires_two_steps() {
        let one = vec![step(0, "a")];
        let err = validate_pattern_shape(PatternKind::Workflow, Some(&one), None, None, None)
            .unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
        let two = vec![step(0, "a"), step(1, "b")];
        validate_pattern_shape(PatternKind::Workflow, Some(&two), None, None, None).unwrap();
    }

    #[test]
    fn graph_rejects_same_name_stance_pair() {
        let sybil = vec![
            reviewer("impl-coder", Stance::Neutral),
            reviewer("impl-coder", Stance::Neutral),
        ];
        let err = validate_pattern_shape(PatternKind::Graph, None, Some(&sybil), None, None)
            .unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn graph_accepts_same_name_distinct_stance() {
        let diverse = vec![
            reviewer("impl-coder", Stance::Proposer),
            reviewer("impl-coder", Stance::DevilAdvocate),
        ];
        validate_pattern_shape(PatternKind::Graph, None, Some(&diverse), None, None).unwrap();
    }

    #[test]
    fn swarm_requires_two_fan_out() {
        let one = vec!["a".to_string()];
        let err = validate_pattern_shape(PatternKind::Swarm, None, None, Some(&one), None)
            .unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
        let two = vec!["a".to_string(), "b".to_string()];
        validate_pattern_shape(PatternKind::Swarm, None, None, Some(&two), None).unwrap();
    }

    #[test]
    fn agents_as_tools_requires_one_peer() {
        let none: Vec<PeerLink> = vec![];
        let err = validate_pattern_shape(
            PatternKind::AgentsAsTools,
            None,
            None,
            None,
            Some(&none),
        )
        .unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
        let one = vec![PeerLink {
            caller: "a".into(),
            callee: "b".into(),
        }];
        validate_pattern_shape(PatternKind::AgentsAsTools, None, None, None, Some(&one)).unwrap();
    }

    #[test]
    fn direct_always_passes() {
        validate_pattern_shape(PatternKind::Direct, None, None, None, None).unwrap();
    }

    #[test]
    fn reversal_plan_migration_sql_round_trip() {
        let json = r#"{"type":"migration_sql","sql":"DROP TABLE x;","dependencies":["001"]}"#;
        let p = validate_reversal_plan_json(json).unwrap();
        match p {
            ReversalPlan::MigrationSql { sql, dependencies } => {
                assert_eq!(sql, "DROP TABLE x;");
                assert_eq!(dependencies, vec!["001"]);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn reversal_plan_git_revert_requires_non_empty_sha() {
        let err = validate_reversal_plan_json(r#"{"type":"git_revert","sha":""}"#).unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn reversal_plan_compensating_action_rejects_empty_object() {
        let err = validate_reversal_plan_json(
            r#"{"type":"compensating_action","action_spec":{}}"#,
        )
        .unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }

    #[test]
    fn pattern_kind_round_trip() {
        for k in [
            PatternKind::Workflow,
            PatternKind::Graph,
            PatternKind::Swarm,
            PatternKind::AgentsAsTools,
            PatternKind::Direct,
        ] {
            let s = k.as_str();
            assert_eq!(s.parse::<PatternKind>().unwrap(), k);
        }
    }

    #[test]
    fn lifecycle_terminal_predicate() {
        assert!(PatternLifecycle::Converged.is_terminal());
        assert!(PatternLifecycle::Dissensus.is_terminal());
        assert!(PatternLifecycle::Aborted.is_terminal());
        assert!(!PatternLifecycle::Pending.is_terminal());
        assert!(!PatternLifecycle::Active.is_terminal());
    }
}
