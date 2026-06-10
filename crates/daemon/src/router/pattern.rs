//! `/patterns` router. D22 — CollaborationPattern is the 7th first-class
//! entity. CRUD + lifecycle transition surface for the pattern repo, with
//! daemon-side gates layered on top of the storage layer.
//!
//! Gates enforced here:
//! - D24 — `parent_pattern_id` cycle detection + depth cap from the project's
//!   resolved AutonomyPolicy.pattern_depth_cap.
//! - D26/D27 — shape validation runs at `pending → active`; invalid manifests
//!   are rejected with HTTP 400.
//! - D27 — terminal lifecycles (`converged`/`dissensus`/`aborted`) stamp
//!   `decided_at` automatically (delegated to repo::update_lifecycle).
//!
//! SSE events emitted:
//! - `pattern_created` on POST /patterns
//! - `pattern_lifecycle` on PATCH /patterns/:id/lifecycle

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, patch, post},
    Json, Router,
};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id, IdKind};
use sdi_core::pattern::{
    validate_pattern_shape, AppliesTo, PatternKind, PatternLifecycle, PatternStep, PeerLink,
    Reviewer,
};
use sdi_db::repo::autonomy_policy as autonomy_repo;
use sdi_db::repo::pattern as repo;
use sdi_db::repo::pattern::PatternRow;
use sdi_db::repo::plan as plan_repo;
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/patterns", post(create).get(list))
        .route("/patterns/active", get(list_active))
        .route("/patterns/:id", get(get_one).delete(method_not_allowed))
        .route("/patterns/:id/lifecycle", patch(transition_lifecycle))
}

// ── POST /patterns ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CreatePatternBody {
    plan_id: String,
    short_code: String,
    /// `workflow` | `graph` | `swarm` | `agents-as-tools` | `direct`.
    kind: String,
    /// `plan` | `requirement` | `scenario` | `task` | `decision` | `round`.
    applies_to: String,
    /// The id of the work entity being produced (matches `applies_to`).
    scope_id: String,
    #[serde(default)]
    parent_pattern_id: Option<String>,
    /// Ordered workflow steps; required when kind = workflow.
    #[serde(default)]
    steps: Option<Vec<PatternStep>>,
    /// Graph reviewers; required when kind = graph.
    #[serde(default)]
    reviewers: Option<Vec<Reviewer>>,
    /// Swarm fan-out targets (agent names); required when kind = swarm.
    #[serde(default)]
    fan_out: Option<Vec<String>>,
    /// Peer registrations; required when kind = agents-as-tools.
    #[serde(default)]
    peer_registration: Option<Vec<PeerLink>>,
}

async fn create(
    State(state): State<AppState>,
    Json(b): Json<CreatePatternBody>,
) -> ApiResult<Json<Value>> {
    let kind = PatternKind::from_str(&b.kind)?;
    // applies_to validation surfaces a sane error before SQL CHECK.
    AppliesTo::from_str(&b.applies_to)?;
    let plan_id = Id::from(b.plan_id);

    let conn = state.conn()?;

    // D24 — resolve depth cap from the plan's project autonomy policy. Fall
    // back to the AutonomyPolicy default (3) when no policy row exists.
    let depth_cap = resolve_depth_cap(&conn, &plan_id, &b.kind)?;

    // D24 — depth + cycle. Cycles are blocked by `cycle_blocked` walk; depth
    // is parent.depth + 1 when parent is set.
    let (depth, parent_id_opt) = match &b.parent_pattern_id {
        Some(pid_str) => {
            let parent_id = Id::from(pid_str.clone());
            let parent = repo::get(&conn, &parent_id)?
                .ok_or_else(|| DomainError::NotFound(parent_id.to_string()))?;
            let next_depth = parent.depth + 1;
            if next_depth as i32 > depth_cap {
                return Err(DomainError::Validation(format!(
                    "pattern depth {next_depth} exceeds cap {depth_cap}"
                ))
                .into());
            }
            (next_depth, Some(parent_id))
        }
        None => (0_i64, None),
    };

    let steps_json = encode_opt(&b.steps)?;
    let reviewers_json = encode_opt(&b.reviewers)?;
    let fan_out_json = encode_opt(&b.fan_out)?;
    let peer_registration_json = encode_opt(&b.peer_registration)?;

    let row = PatternRow {
        id: Id::new(IdKind::Pattern).to_string(),
        short_code: b.short_code,
        plan_id: plan_id.to_string(),
        kind: kind.to_string(),
        applies_to: b.applies_to,
        scope_id: b.scope_id,
        parent_pattern_id: parent_id_opt.as_ref().map(|p| p.to_string()),
        depth,
        lifecycle: PatternLifecycle::Pending.to_string(),
        steps_json,
        reviewers_json,
        fan_out_json,
        peer_registration_json,
        decided_at: None,
        decided_reason: None,
        created_at: now(),
        updated_at: now(),
    };
    repo::insert(&conn, &row)?;
    let fresh = repo::get(&conn, &Id::from(row.id.clone()))?
        .ok_or_else(|| DomainError::NotFound(row.id.clone()))?;
    state.publish(EventEnvelope {
        kind: "pattern_created".into(),
        entity_id: Some(fresh.id.clone()),
        payload: pattern_payload(&fresh),
    });
    Ok(Json(pattern_payload(&fresh)))
}

// ── GET /patterns?plan_id=… ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ListQuery {
    plan_id: String,
}

async fn list(State(state): State<AppState>, Query(q): Query<ListQuery>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = repo::list_by_plan(&conn, &Id::from(q.plan_id))?;
    let payload: Vec<Value> = rows.iter().map(pattern_payload).collect();
    Ok(Json(json!({ "patterns": payload })))
}

// ── GET /patterns/active[?project_id=…] ────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ListActiveQuery {
    /// Scope to one project (through plans). The dashboard badge passes this
    /// so its count agrees with the project's Patterns view; hook gates and
    /// CLI status keep the unscoped daemon-wide form.
    #[serde(default)]
    project_id: Option<String>,
}

async fn list_active(
    State(state): State<AppState>,
    Query(q): Query<ListActiveQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = repo::list_active(&conn, q.project_id.as_deref())?;
    let payload: Vec<Value> = rows.iter().map(pattern_payload).collect();
    Ok(Json(json!({ "patterns": payload })))
}

// ── GET /patterns/:id ──────────────────────────────────────────────────────

async fn get_one(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let row = repo::get(&conn, &Id::from(id.clone()))?.ok_or(DomainError::NotFound(id))?;
    Ok(Json(pattern_payload(&row)))
}

// ── PATCH /patterns/:id/lifecycle ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct LifecycleBody {
    /// One of `pending`, `active`, `converged`, `dissensus`, `aborted`.
    to: String,
    #[serde(default)]
    reason: Option<String>,
}

async fn transition_lifecycle(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<LifecycleBody>,
) -> ApiResult<Json<Value>> {
    let next = PatternLifecycle::from_str(&b.to)?;
    let conn = state.conn()?;
    let pid = Id::from(id.clone());
    let current = repo::get(&conn, &pid)?.ok_or_else(|| DomainError::NotFound(id.clone()))?;
    let current_lifecycle = PatternLifecycle::from_str(&current.lifecycle)?;

    // Reject moves out of terminal states (converged/dissensus/aborted).
    if current_lifecycle.is_terminal() {
        return Err(DomainError::Validation(format!(
            "pattern {id} is terminal ({}); no transitions allowed",
            current.lifecycle
        ))
        .into());
    }

    // D27 — shape gate only at pending → active.
    if matches!(current_lifecycle, PatternLifecycle::Pending)
        && matches!(next, PatternLifecycle::Active)
    {
        let kind = PatternKind::from_str(&current.kind)?;
        let steps = decode_opt::<Vec<PatternStep>>(current.steps_json.as_deref())?;
        let reviewers = decode_opt::<Vec<Reviewer>>(current.reviewers_json.as_deref())?;
        let fan_out = decode_opt::<Vec<String>>(current.fan_out_json.as_deref())?;
        let peers = decode_opt::<Vec<PeerLink>>(current.peer_registration_json.as_deref())?;
        validate_pattern_shape(
            kind,
            steps.as_deref(),
            reviewers.as_deref(),
            fan_out.as_deref(),
            peers.as_deref(),
        )?;
    }

    repo::update_lifecycle(&conn, &pid, next.as_str(), b.reason.as_deref())?;
    let fresh = repo::get(&conn, &pid)?.ok_or(DomainError::NotFound(id))?;
    state.publish(EventEnvelope {
        kind: "pattern_lifecycle".into(),
        entity_id: Some(fresh.id.clone()),
        payload: json!({
            "id": fresh.id,
            "from": current.lifecycle,
            "to": fresh.lifecycle,
            "reason": fresh.decided_reason,
            "decided_at": fresh.decided_at.map(|t| t.to_rfc3339()),
        }),
    });
    Ok(Json(pattern_payload(&fresh)))
}

async fn method_not_allowed() -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    Err((
        StatusCode::METHOD_NOT_ALLOWED,
        Json(json!({"error": {"code": "METHOD_NOT_ALLOWED",
                              "message": "patterns are append-only; transition lifecycle instead",
                              "status": 405}})),
    ))
}

// ── helpers ────────────────────────────────────────────────────────────────

fn encode_opt<T: serde::Serialize>(v: &Option<T>) -> DomainResult<Option<String>> {
    match v {
        Some(x) => Ok(Some(serde_json::to_string(x).map_err(|e| {
            DomainError::Validation(format!("encode pattern field: {e}"))
        })?)),
        None => Ok(None),
    }
}

fn decode_opt<T: serde::de::DeserializeOwned>(raw: Option<&str>) -> DomainResult<Option<T>> {
    match raw {
        Some(s) => Ok(Some(serde_json::from_str(s).map_err(|e| {
            DomainError::Validation(format!("decode pattern field: {e}"))
        })?)),
        None => Ok(None),
    }
}

fn pattern_payload(row: &PatternRow) -> Value {
    json!({
        "id": row.id,
        "short_code": row.short_code,
        "plan_id": row.plan_id,
        "kind": row.kind,
        "applies_to": row.applies_to,
        "scope_id": row.scope_id,
        "parent_pattern_id": row.parent_pattern_id,
        "depth": row.depth,
        "lifecycle": row.lifecycle,
        "steps_json": row.steps_json,
        "reviewers_json": row.reviewers_json,
        "fan_out_json": row.fan_out_json,
        "peer_registration_json": row.peer_registration_json,
        "decided_at": row.decided_at.map(|t| t.to_rfc3339()),
        "decided_reason": row.decided_reason,
        "created_at": row.created_at.to_rfc3339(),
        "updated_at": row.updated_at.to_rfc3339(),
    })
}

/// Resolve `pattern_depth_cap` from autonomy policy, in specificity order:
/// pattern-kind-scoped → global. Falls back to 3 (the AutonomyPolicy default).
fn resolve_depth_cap(
    conn: &sdi_db::PooledConn,
    plan_id: &Id,
    pattern_kind: &str,
) -> DomainResult<i32> {
    // Walk plan → project to access the project id for autonomy resolution.
    let plan = plan_repo::get(conn, plan_id)?;
    let policy = autonomy_repo::resolve(
        conn,
        &plan.project_id,
        Some(plan_id),
        None,
        Some(pattern_kind),
    )?;
    Ok(policy.map(|p| p.pattern_depth_cap).unwrap_or(3))
}
