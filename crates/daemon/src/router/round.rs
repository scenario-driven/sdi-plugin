//! `/rounds` router. Lifecycle:
//! - POST `/rounds` — create (planning)
//! - POST `/rounds/:id/activate` — planning → active. If a prior completed
//!   round exists and `mode = strict-regression`, auto-populates
//!   `scenario_results` (D6).
//! - POST `/rounds/:id/complete` — active → completed
//! - POST `/rounds/:id/results` — upsert one scenario verdict
//!   (passing|failing|impacted|retired)

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use sdi_core::error::DomainError;
use sdi_core::ids::{now, Id, IdKind};
use sdi_core::round::{DisruptionPolicy, InFlightPolicy, Round, RoundMode, RoundStatus};
use sdi_core::scenario::ScenarioResult;
use sdi_core::task::TaskStatus;
use sdi_db::repo::disruption as disruption_repo;
use sdi_db::repo::round as repo;
use sdi_db::repo::task as task_repo;
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/rounds", post(create).get(list))
        .route("/rounds/:id", get(get_one))
        .route("/rounds/:id/activate", post(activate))
        .route("/rounds/:id/complete", post(complete))
        .route("/rounds/:id/results", get(list_results).post(post_result))
        .route("/plans/:plan_id/rounds/active", get(active_for_plan))
}

#[derive(Debug, Deserialize)]
struct CreateRoundBody {
    plan_id: String,
    short_code: String,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    in_flight_policy: Option<String>,
    #[serde(default)]
    disruption_policy: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    Json(b): Json<CreateRoundBody>,
) -> ApiResult<Json<Value>> {
    let mode = match b.mode.as_deref() {
        Some(m) => RoundMode::from_str(m)?,
        None => RoundMode::default(),
    };
    let in_flight_policy = match b.in_flight_policy.as_deref() {
        Some(p) => InFlightPolicy::from_str(p)?,
        None => InFlightPolicy::default(),
    };
    let disruption_policy = match b.disruption_policy.as_deref() {
        Some(p) => DisruptionPolicy::from_str(p)?,
        None => DisruptionPolicy::default(),
    };
    let conn = state.conn()?;
    let plan_id = Id::from(b.plan_id);
    let n = repo::next_round_number(&conn, &plan_id)?;
    let round = Round {
        id: Id::new(IdKind::Round),
        plan_id,
        short_code: b.short_code,
        round_number: n,
        mode,
        in_flight_policy,
        disruption_policy,
        status: RoundStatus::Planning,
        activated_at: None,
        completed_at: None,
        created_at: now(),
        updated_at: now(),
    };
    repo::insert(&conn, &round)?;
    let fresh = repo::get(&conn, &round.id)?;
    state.publish(EventEnvelope {
        kind: "round.created".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct ListRoundQuery {
    plan_id: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListRoundQuery>,
) -> ApiResult<Json<Value>> {
    let plan_id = q
        .plan_id
        .ok_or_else(|| DomainError::Validation("plan_id query parameter required".into()))?;
    let conn = state.conn()?;
    let rows = repo::list_by_plan(&conn, &Id::from(plan_id))?;
    Ok(Json(json!({ "rounds": rows })))
}

async fn get_one(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    Ok(Json(json!(repo::get(&conn, &Id::from(id))?)))
}

async fn activate(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rid = Id::from(id);
    let round = repo::get(&conn, &rid)?;
    round.check_can_activate()?;

    // PRD §6 #4 — Disruption needs-review gate.
    //
    // If the LLM has recorded any disruption review on this plan that is
    // still `pending`, refuse to promote the round. The human must resolve
    // each pending review (retire / edit / keep impacted scenarios) before
    // the next round can start. This applies regardless of the round's own
    // `disruption_policy` — `auto` only changes how the LLM proposes
    // resolutions at creation time; the human-confirm gate is universal.
    if disruption_repo::has_pending(&conn, &round.plan_id)? {
        return Err(DomainError::DisruptionPending.into());
    }

    // PRD §6 #5 — In-flight Task pause enforcement.
    //
    // Before promoting a Round to active, the daemon honors the Round's
    // `in_flight_policy`. The reaction is per-policy:
    //   - Pause                 → transition all in-progress tasks in the
    //                              plan to Blocked. Default.
    //   - Abort                 → transition them to Cancelled.
    //   - ContinueOnNoimpact    → leave them as in_progress.
    //
    // Identifying the actual disjoint set of scenarios for the third option
    // is the disruption analysis tracked under PRD §6 #4; this handler
    // currently leaves running tasks alone in that mode and surfaces the
    // count so the caller can confirm intent.
    let in_flight = task_repo::list_in_flight_for_plan(&conn, &round.plan_id)?;
    let mut affected: Vec<String> = Vec::with_capacity(in_flight.len());
    let action = match round.in_flight_policy {
        InFlightPolicy::Pause => {
            for t in &in_flight {
                task_repo::set_status(&conn, &t.id, TaskStatus::Blocked)?;
                affected.push(t.id.to_string());
            }
            "paused"
        }
        InFlightPolicy::Abort => {
            for t in &in_flight {
                task_repo::set_status(&conn, &t.id, TaskStatus::Cancelled)?;
                affected.push(t.id.to_string());
            }
            "aborted"
        }
        InFlightPolicy::ContinueOnNoimpact => {
            for t in &in_flight {
                affected.push(t.id.to_string());
            }
            "continued"
        }
    };

    let carried = if matches!(round.mode, RoundMode::StrictRegression) {
        if let Some(prev) = repo::latest_completed_for_plan(&conn, &round.plan_id)? {
            repo::carry_over_results(&conn, &round, &prev.id)?
        } else {
            0
        }
    } else {
        0
    };
    let activated_at = now();
    repo::set_status(&conn, &rid, RoundStatus::Active, Some(activated_at), None)?;
    let fresh = repo::get(&conn, &rid)?;
    let payload = json!({
        "round": fresh,
        "carried_results": carried,
        "in_flight": {
            "policy": round.in_flight_policy.to_string(),
            "action": action,
            "affected_task_ids": affected,
        },
    });
    state.publish(EventEnvelope {
        kind: "round.activated".into(),
        entity_id: Some(rid.to_string()),
        payload: payload.clone(),
    });
    Ok(Json(payload))
}

async fn complete(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rid = Id::from(id);
    let round = repo::get(&conn, &rid)?;
    round.check_can_complete()?;
    let completed_at = now();
    repo::set_status(
        &conn,
        &rid,
        RoundStatus::Completed,
        None,
        Some(completed_at),
    )?;
    let fresh = repo::get(&conn, &rid)?;
    state.publish(EventEnvelope {
        kind: "round.completed".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct PostResultBody {
    scenario_id: String,
    result: String,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    evidence_ref: Option<String>,
}

async fn post_result(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<PostResultBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rid = Id::from(id);
    let verdict = ScenarioResult::from_str(&b.result)?;
    repo::upsert_result(
        &conn,
        &rid,
        &Id::from(&b.scenario_id[..]),
        verdict,
        b.note.as_deref(),
        b.evidence_ref.as_deref(),
    )?;
    state.publish(EventEnvelope {
        kind: "round.result-updated".into(),
        entity_id: Some(rid.to_string()),
        payload: json!({
            "round_id": rid.to_string(),
            "scenario_id": b.scenario_id,
            "result": b.result,
        }),
    });
    Ok(Json(json!({ "ok": true })))
}

async fn list_results(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = repo::list_results(&conn, &Id::from(id))?;
    let payload: Vec<_> = rows
        .into_iter()
        .map(|r| {
            json!({
                "round_id": r.round_id.to_string(),
                "scenario_id": r.scenario_id.to_string(),
                "result": r.result.to_string(),
                "note": r.note,
                "evidence_ref": r.evidence_ref,
                "updated_at": r.updated_at,
            })
        })
        .collect();
    Ok(Json(json!({ "results": payload })))
}

async fn active_for_plan(
    State(state): State<AppState>,
    Path(plan_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let found = repo::find_active_for_plan(&conn, &Id::from(plan_id))?;
    Ok(Json(json!({ "round": found })))
}
