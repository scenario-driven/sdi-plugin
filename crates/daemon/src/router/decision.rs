//! `/decisions` router. Append-only ADR log (D12). Body is never edited — a
//! follow-up decision uses `supersedes_id` to chain a replacement.
//!
//! v0.5 (D28) adds reversibility:
//! - `produced_via_pattern_id` carried at create.
//! - `reversal_plan` (validated JSON), `blast_radius_score` (0..=10), and
//!   `reversal_of` accepted on create.
//! - `POST /decisions/:id/rollback` writes an append-only consensus row whose
//!   `reversal_of` points at the original decision; the daemon's L5 unlock
//!   gate is enforced here.

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use sdi_core::agent_spec::AgentSpec;
use sdi_core::decision::{Decision, DecisionKind, DecisionStatus};
use sdi_core::error::DomainError;
use sdi_core::ids::{now, Id, IdKind};
use sdi_core::pattern::validate_reversal_plan_json;
use sdi_db::repo::decision as repo;
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/decisions", post(create).get(list))
        .route("/decisions/:id", get(get_one))
        .route("/decisions/:id/status", post(set_status))
        .route("/decisions/:id/rollback", post(rollback))
}

#[derive(Debug, Deserialize)]
struct CreateDecisionBody {
    plan_id: String,
    short_code: String,
    title: String,
    body: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    supersedes_id: Option<String>,
    /// D20 — M3 stage classifier. Defaults to `proposal` for compatibility.
    #[serde(default)]
    kind: Option<String>,
    /// D20 — required when kind ∈ {critique, consensus, dissensus}.
    #[serde(default)]
    proposal_id: Option<String>,
    /// D20 — Layer-2 specialist that emitted this row.
    #[serde(default)]
    agent_name: Option<String>,
    /// D23 — pattern this decision was produced under.
    #[serde(default)]
    produced_via_pattern_id: Option<String>,
    /// D28 — JSON-encoded reversal plan (discriminated on `type`).
    #[serde(default)]
    reversal_plan: Option<String>,
    /// D28 — recovery-cost score (0..=10). Defaults to 5 when omitted.
    #[serde(default)]
    blast_radius_score: Option<i32>,
    /// D28 — when this row IS a rollback of another decision, the original id.
    #[serde(default)]
    reversal_of: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    Json(b): Json<CreateDecisionBody>,
) -> ApiResult<Json<Value>> {
    let status = match b.status.as_deref() {
        Some(s) => DecisionStatus::from_str(s)?,
        None => DecisionStatus::Accepted,
    };
    let kind = match b.kind.as_deref() {
        Some(s) => DecisionKind::from_str(s)?,
        None => DecisionKind::Proposal,
    };
    let escalated_at = if matches!(kind, DecisionKind::Dissensus) {
        Some(now())
    } else {
        None
    };
    // Reject namespace-prefixed or otherwise unregistered Layer-2 specialist
    // identifiers at the gate — DB columns store the canonical bare name
    // (`STOCK_AGENTS` ∪ `STOCK_META_AGENTS`). Without this check a client that
    // reports itself as e.g. `sdi:impl-coder` would silently corrupt later
    // grouping / lookup against AgentSpec rows.
    if let Some(name) = b.agent_name.as_deref() {
        AgentSpec::validate_name(name)?;
    }
    // D28 — reject malformed reversal_plan JSON at the gate, before SQL.
    if let Some(plan) = b.reversal_plan.as_deref() {
        validate_reversal_plan_json(plan)?;
    }
    let decision = Decision {
        id: Id::new(IdKind::Decision),
        plan_id: Id::from(b.plan_id),
        short_code: b.short_code,
        title: b.title,
        body: b.body,
        status,
        supersedes_id: b.supersedes_id.map(Id::from),
        kind,
        proposal_id: b.proposal_id.map(Id::from),
        agent_name: b.agent_name,
        escalated_at,
        produced_via_pattern_id: b.produced_via_pattern_id,
        reversal_plan: b.reversal_plan,
        blast_radius_score: b.blast_radius_score.unwrap_or(5),
        reversal_of: b.reversal_of.map(Id::from),
        created_at: now(),
    };
    let conn = state.conn()?;
    repo::insert(&conn, &decision)?;
    let fresh = repo::get(&conn, &decision.id)?;
    let event_kind = match fresh.kind {
        DecisionKind::Consensus => "consensus.reached",
        DecisionKind::Dissensus => "dissensus.escalated",
        _ => "decision.created",
    };
    state.publish(EventEnvelope {
        kind: event_kind.into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct ListDecisionQuery {
    plan_id: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListDecisionQuery>,
) -> ApiResult<Json<Value>> {
    let plan_id = q
        .plan_id
        .ok_or_else(|| DomainError::Validation("plan_id query parameter required".into()))?;
    let conn = state.conn()?;
    let rows = repo::list_by_plan(&conn, &Id::from(plan_id))?;
    Ok(Json(json!({ "decisions": rows })))
}

async fn get_one(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    Ok(Json(json!(repo::get(&conn, &Id::from(id))?)))
}

#[derive(Debug, Deserialize)]
struct SetStatusBody {
    status: String,
}

async fn set_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<SetStatusBody>,
) -> ApiResult<Json<Value>> {
    let status = DecisionStatus::from_str(&b.status)?;
    let conn = state.conn()?;
    let did = Id::from(id);
    repo::set_status(&conn, &did, status)?;
    let fresh = repo::get(&conn, &did)?;
    state.publish(EventEnvelope {
        kind: "decision.status-changed".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct RollbackBody {
    short_code: String,
    title: String,
    body: String,
    /// D28 — required: the rollback decision's own reversal plan (in case the
    /// reversal itself needs to be reversed). Must validate.
    reversal_plan: String,
    /// D28 — defaults to 5; cannot exceed 10.
    #[serde(default)]
    blast_radius_score: Option<i32>,
    /// D28 — the rollback action must itself be produced under a pattern.
    /// When omitted, the daemon writes `direct` (anti-pattern marker).
    #[serde(default)]
    produced_via_pattern_id: Option<String>,
    /// Layer-2 specialist that authored the rollback. Conventionally
    /// `reversal-runner`.
    #[serde(default = "default_reversal_agent")]
    agent_name: String,
}

fn default_reversal_agent() -> String {
    "reversal-runner".into()
}

/// D28 — `POST /decisions/:id/rollback`. Appends a `consensus` decision whose
/// `reversal_of` points at the original. The original is never mutated
/// (D12 SNAPSHOT-ONLY); callers wanting to mark it superseded should follow
/// up with `POST /decisions/<original>/status {"status":"superseded"}`.
async fn rollback(
    State(state): State<AppState>,
    Path(original_id): Path<String>,
    Json(b): Json<RollbackBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let orig_id = Id::from(original_id.clone());
    // Pull the original to copy plan_id + emit a meaningful initiated event.
    let original = repo::get(&conn, &orig_id)?;
    // Same Layer-2 specialist contract as `create` — bare canonical name only.
    AgentSpec::validate_name(&b.agent_name)?;
    // D28 — validate the rollback plan JSON before any SQL.
    validate_reversal_plan_json(&b.reversal_plan)?;
    let score = b.blast_radius_score.unwrap_or(5);
    Decision::validate_blast_radius_score(score)?;
    state.publish(EventEnvelope {
        kind: "rollback_initiated".into(),
        entity_id: Some(orig_id.to_string()),
        payload: json!({
            "original_decision_id": orig_id.to_string(),
            "plan_id": original.plan_id.to_string(),
            "agent_name": b.agent_name,
        }),
    });
    let rollback_decision = Decision {
        id: Id::new(IdKind::Decision),
        plan_id: original.plan_id.clone(),
        short_code: b.short_code,
        title: b.title,
        body: b.body,
        status: DecisionStatus::Accepted,
        // SNAPSHOT-ONLY — do NOT chain via supersedes_id; rollback is its own
        // first-class row. The caller marks the original superseded via the
        // dedicated /status endpoint when appropriate.
        supersedes_id: None,
        kind: DecisionKind::Consensus,
        // D28 — rollback is structurally a consensus row for the audit log
        // but NOT an M3-gated multi-party consensus. The repo layer bypasses
        // the M3 stage gate when `reversal_of` is set, so this row needs no
        // proposal_id / synthetic critique.
        proposal_id: None,
        agent_name: Some(b.agent_name.clone()),
        escalated_at: None,
        produced_via_pattern_id: b.produced_via_pattern_id,
        reversal_plan: Some(b.reversal_plan),
        blast_radius_score: score,
        reversal_of: Some(orig_id.clone()),
        created_at: now(),
    };
    repo::insert(&conn, &rollback_decision)?;
    let fresh = repo::get(&conn, &rollback_decision.id)?;
    state.publish(EventEnvelope {
        kind: "rollback_completed".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}
