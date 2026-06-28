//! `/oracle` router (PRD-v2 D32/D33/D35) — the product-definition graph (SSoT
//! nodes + edges), the UserFlow tier, the decision-question engine, and the
//! deterministic completeness `verify` that the D34 gates consume.
//!
//! Scope (Phase 1 unit 4a): authoring CRUD + verify of L0 facet completeness,
//! L0 link completeness, open-question count, and L1 (Persona × Capability)
//! coverage. L2 (flow-step → scenario) coverage and the plan-approve gate
//! rewrite land in 4b once plan↔flow scoping is decided.

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use sdi_core::decision_question::{
    DecisionQuestion, QuestionAnswer, QuestionOption, QuestionStatus, QuestionType,
};
use sdi_core::ids::{now, Id, IdKind};
use sdi_core::ssot::{Confidence, SsotEdge, SsotNode};
use sdi_core::user_flow::{FlowStatus, UserFlow};
use sdi_db::map_sqlite_err;
use sdi_db::repo::{
    decision_question as dq_repo, plan_flow as plan_flow_repo, ssot as ssot_repo,
    user_flow as uf_repo,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::str::FromStr;

pub fn router() -> Router<AppState> {
    Router::new()
        // L0 graph
        .route(
            "/projects/:project_id/ssot-nodes",
            post(create_node).get(list_nodes),
        )
        .route("/ssot-nodes/:id/facets", post(update_node_facets))
        .route(
            "/projects/:project_id/ssot-edges",
            post(create_edge).get(list_edges),
        )
        // L1 flows
        .route(
            "/projects/:project_id/user-flows",
            post(create_flow).get(list_flows),
        )
        .route("/user-flows/:id/confirm", post(confirm_flow))
        // D34 plan↔flow targeting (param `:id` matches the plan router's convention
        // — axum panics at router-build if the same path prefix uses a different name)
        .route(
            "/plans/:id/target-flows/:flow_id",
            post(link_flow).delete(unlink_flow),
        )
        // D35 decision questions (own `/decision-questions` namespace — the legacy
        // collab router owns `/questions`)
        .route(
            "/projects/:project_id/decision-questions",
            post(create_question).get(list_questions),
        )
        .route(
            "/decision-questions/:id/options",
            post(add_option).get(list_options),
        )
        .route("/decision-questions/:id/answer", post(answer_question))
        // D34 deterministic verify
        .route("/projects/:project_id/oracle/verify", get(verify))
}

// ------------------------------------------------------------------ L0 nodes

#[derive(Debug, Deserialize)]
struct CreateNodeBody {
    short_code: String,
    kind: String,
    title: String,
    #[serde(default)]
    facets_json: Option<String>,
    #[serde(default)]
    open_markers_json: Option<String>,
    #[serde(default)]
    confidence: Option<String>,
    #[serde(default)]
    produced_via_pattern_id: Option<String>,
}

async fn create_node(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(b): Json<CreateNodeBody>,
) -> ApiResult<Json<Value>> {
    let confidence = match b.confidence.as_deref() {
        Some(s) => Confidence::from_str(s)?,
        None => Confidence::Unverified,
    };
    let node = SsotNode {
        id: Id::new(IdKind::SsotNode),
        project_id: Id::from(project_id),
        short_code: b.short_code,
        kind: b.kind,
        title: b.title,
        facets_json: b.facets_json.unwrap_or_else(|| "{}".into()),
        open_markers_json: b.open_markers_json.unwrap_or_else(|| "[]".into()),
        confidence,
        produced_via_pattern_id: b.produced_via_pattern_id,
        created_at: now(),
        updated_at: now(),
    };
    let conn = state.conn()?;
    ssot_repo::insert_node(&conn, &node)?;
    let fresh = ssot_repo::get_node(&conn, &node.id)?;
    state.publish(EventEnvelope {
        kind: "ssot_node.created".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

async fn list_nodes(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = ssot_repo::list_nodes_by_project(&conn, &Id::from(project_id))?;
    Ok(Json(json!({ "nodes": rows })))
}

#[derive(Debug, Deserialize)]
struct UpdateFacetsBody {
    facets_json: String,
    open_markers_json: String,
}

async fn update_node_facets(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<UpdateFacetsBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let nid = Id::from(id);
    ssot_repo::update_node_facets(&conn, &nid, &b.facets_json, &b.open_markers_json)?;
    let fresh = ssot_repo::get_node(&conn, &nid)?;
    state.publish(EventEnvelope {
        kind: "ssot_node.updated".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

// ------------------------------------------------------------------ L0 edges

#[derive(Debug, Deserialize)]
struct CreateEdgeBody {
    from_node: String,
    to_ref: String,
    rel: String,
}

async fn create_edge(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(b): Json<CreateEdgeBody>,
) -> ApiResult<Json<Value>> {
    let edge = SsotEdge {
        id: Id::new(IdKind::SsotEdge),
        project_id: Id::from(project_id),
        from_node: Id::from(b.from_node),
        to_ref: b.to_ref,
        rel: b.rel,
        created_at: now(),
    };
    let conn = state.conn()?;
    ssot_repo::insert_edge(&conn, &edge)?;
    Ok(Json(json!(edge)))
}

async fn list_edges(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = ssot_repo::list_edges_by_project(&conn, &Id::from(project_id))?;
    Ok(Json(json!({ "edges": rows })))
}

// ------------------------------------------------------------------ L1 flows

#[derive(Debug, Deserialize)]
struct CreateFlowBody {
    short_code: String,
    persona_id: String,
    purpose: String,
    #[serde(default)]
    steps_json: Option<String>,
    #[serde(default)]
    covers_capabilities_json: Option<String>,
    #[serde(default)]
    produced_via_pattern_id: Option<String>,
}

async fn create_flow(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(b): Json<CreateFlowBody>,
) -> ApiResult<Json<Value>> {
    let flow = UserFlow {
        id: Id::new(IdKind::UserFlow),
        project_id: Id::from(project_id),
        short_code: b.short_code,
        persona_id: Id::from(b.persona_id),
        purpose: b.purpose,
        steps_json: b.steps_json.unwrap_or_else(|| "[]".into()),
        covers_capabilities_json: b.covers_capabilities_json.unwrap_or_else(|| "[]".into()),
        status: FlowStatus::Draft,
        produced_via_pattern_id: b.produced_via_pattern_id,
        created_at: now(),
        updated_at: now(),
    };
    let conn = state.conn()?;
    uf_repo::insert(&conn, &flow)?;
    let fresh = uf_repo::get(&conn, &flow.id)?;
    state.publish(EventEnvelope {
        kind: "user_flow.created".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

async fn list_flows(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = uf_repo::list_by_project(&conn, &Id::from(project_id))?;
    Ok(Json(json!({ "flows": rows })))
}

async fn confirm_flow(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let fid = Id::from(id);
    uf_repo::set_status(&conn, &fid, FlowStatus::Confirmed)?;
    let fresh = uf_repo::get(&conn, &fid)?;
    state.publish(EventEnvelope {
        kind: "user_flow.confirmed".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

// ----------------------------------------------------- D34 plan↔flow targeting

async fn link_flow(
    State(state): State<AppState>,
    Path((plan_id, flow_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    plan_flow_repo::link(
        &conn,
        &Id::from(plan_id.clone()),
        &Id::from(flow_id.clone()),
    )?;
    Ok(Json(
        json!({ "plan_id": plan_id, "flow_id": flow_id, "linked": true }),
    ))
}

async fn unlink_flow(
    State(state): State<AppState>,
    Path((plan_id, flow_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    plan_flow_repo::unlink(
        &conn,
        &Id::from(plan_id.clone()),
        &Id::from(flow_id.clone()),
    )?;
    Ok(Json(
        json!({ "plan_id": plan_id, "flow_id": flow_id, "linked": false }),
    ))
}

// ------------------------------------------------------------ D35 questions

#[derive(Debug, Deserialize)]
struct CreateQuestionBody {
    short_code: String,
    qtype: String,
    context_md: String,
    #[serde(default)]
    scope_ref: Option<String>,
    #[serde(default)]
    parent_question_id: Option<String>,
}

async fn create_question(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(b): Json<CreateQuestionBody>,
) -> ApiResult<Json<Value>> {
    let q = DecisionQuestion {
        id: Id::new(IdKind::DecisionQuestion),
        project_id: Id::from(project_id),
        short_code: b.short_code,
        scope_ref: b.scope_ref,
        qtype: QuestionType::from_str(&b.qtype)?,
        context_md: b.context_md,
        parent_question_id: b.parent_question_id,
        status: QuestionStatus::Open,
        created_at: now(),
        updated_at: now(),
    };
    let conn = state.conn()?;
    dq_repo::insert_question(&conn, &q)?;
    let fresh = dq_repo::get_question(&conn, &q.id)?;
    state.publish(EventEnvelope {
        kind: "decision_question.created".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

async fn list_questions(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = dq_repo::list_questions_by_project(&conn, &Id::from(project_id))?;
    Ok(Json(json!({ "questions": rows })))
}

#[derive(Debug, Deserialize)]
struct AddOptionBody {
    label: String,
    #[serde(default)]
    body_md: String,
    #[serde(default)]
    rationale_md: String,
    #[serde(default)]
    is_llm_recommended: bool,
    #[serde(default)]
    idx: i64,
}

async fn add_option(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<AddOptionBody>,
) -> ApiResult<Json<Value>> {
    let opt = QuestionOption {
        id: Id::new(IdKind::QuestionOption),
        question_id: Id::from(id),
        label: b.label,
        body_md: b.body_md,
        rationale_md: b.rationale_md,
        is_llm_recommended: b.is_llm_recommended,
        idx: b.idx,
        created_at: now(),
    };
    let conn = state.conn()?;
    dq_repo::insert_option(&conn, &opt)?;
    Ok(Json(json!(opt)))
}

async fn list_options(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = dq_repo::list_options(&conn, &Id::from(id))?;
    Ok(Json(json!({ "options": rows })))
}

#[derive(Debug, Deserialize)]
struct AnswerBody {
    #[serde(default)]
    chosen_option_id: Option<String>,
    #[serde(default)]
    free_text: Option<String>,
    #[serde(default = "default_answered_by")]
    answered_by: String,
    /// `true` for a fact-type 1-survivor auto-decision → status `auto_decided`.
    #[serde(default)]
    auto: bool,
    // D35 answer→compile (deterministic): atomically apply the decision to the
    // scoped SSoT node — close an OPEN marker and/or set facets — so answering a
    // question moves the oracle toward completeness in one transaction.
    #[serde(default)]
    apply_node_id: Option<String>,
    /// OPEN marker on `apply_node_id` this answer resolves (closes).
    #[serde(default)]
    resolve_marker_id: Option<String>,
    /// Optional full replacement of the node's facets (the decided value).
    #[serde(default)]
    apply_facets_json: Option<String>,
}

fn default_answered_by() -> String {
    "user".into()
}

async fn answer_question(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<AnswerBody>,
) -> ApiResult<Json<Value>> {
    let qid = Id::from(id);
    // Provenance — record the node this answer compiled into (D23/D35).
    let generated_refs_json = match &b.apply_node_id {
        Some(node_id) => format!("[\"{node_id}\"]"),
        None => "[]".into(),
    };
    let ans = QuestionAnswer {
        id: Id::new(IdKind::QuestionAnswer),
        question_id: qid.clone(),
        chosen_option_id: b.chosen_option_id,
        free_text: b.free_text,
        answered_by: b.answered_by,
        generated_refs_json,
        created_at: now(),
    };

    let mut conn = state.conn()?;
    {
        let tx = conn.transaction().map_err(map_sqlite_err)?;
        // record answer + flip question status (answered | auto_decided)
        dq_repo::insert_answer(&tx, &ans, b.auto)?;
        // deterministic compile into the oracle node, if targeted
        if let Some(node_id) = b.apply_node_id.as_deref() {
            let nid = Id::from(node_id);
            let node = ssot_repo::get_node(&tx, &nid)?;
            let facets = b.apply_facets_json.as_deref().unwrap_or(&node.facets_json);
            let new_markers = match b.resolve_marker_id.as_deref() {
                Some(mid) => SsotNode::remove_open_marker(&node.open_markers_json, mid)?,
                None => node.open_markers_json.clone(),
            };
            ssot_repo::update_node_facets(&tx, &nid, facets, &new_markers)?;
        }
        tx.commit().map_err(map_sqlite_err)?;
    }

    let q = dq_repo::get_question(&conn, &qid)?;
    state.publish(EventEnvelope {
        kind: "decision_question.answered".into(),
        entity_id: Some(qid.to_string()),
        payload: json!({ "answer": ans, "question": q }),
    });
    Ok(Json(json!({ "answer": ans, "question": q })))
}

// ------------------------------------------------------------ D34 verify

async fn verify(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let pid = Id::from(project_id);

    // L0 — facet + link completeness. A node is facet-incomplete when it has an
    // unresolved OPEN marker OR is missing a required facet for its kind (the
    // sufficiency floor, PRD-v2 D32), so the count is computed in-process rather
    // than in SQL (which cannot inspect the per-kind facet shape).
    let facet_incomplete = ssot_repo::list_nodes_by_project(&conn, &pid)?
        .iter()
        .filter(|n| !n.is_facet_complete())
        .count() as i64;
    let dangling = ssot_repo::count_dangling_edges(&conn, &pid)?;
    // D35 — open (unanswered) decision questions.
    let open_questions = dq_repo::count_open_questions(&conn, &pid)?;

    // L1 — every (Persona × Capability) covered by ≥1 confirmed flow.
    let personas = ssot_repo::list_nodes_by_kind(&conn, &pid, "Persona")?;
    let capabilities = ssot_repo::list_nodes_by_kind(&conn, &pid, "Capability")?;
    let flows = uf_repo::list_by_project(&conn, &pid)?;
    let mut covered: HashSet<(String, String)> = HashSet::new();
    for f in &flows {
        if f.status != FlowStatus::Confirmed {
            continue;
        }
        let caps =
            UserFlow::parse_covers_capabilities(&f.covers_capabilities_json).unwrap_or_default();
        for c in caps {
            covered.insert((f.persona_id.as_str().to_string(), c));
        }
    }
    let mut l1_uncovered: Vec<Value> = Vec::new();
    for p in &personas {
        for c in &capabilities {
            let key = (p.id.as_str().to_string(), c.id.as_str().to_string());
            if !covered.contains(&key) {
                l1_uncovered.push(json!({
                    "persona": p.id, "persona_title": p.title,
                    "capability": c.id, "capability_title": c.title,
                }));
            }
        }
    }

    let l0_complete = facet_incomplete == 0 && dangling == 0;
    let l1_complete = l1_uncovered.is_empty();
    let questions_clear = open_questions == 0;
    // A non-vacuous backbone is a precondition for completeness. An empty (or
    // persona-less / capability-less) graph would otherwise read
    // `oracle_complete:true` vacuously — 0 personas × 0 capabilities = 0
    // uncovered pairs, 0 incomplete facets, 0 open questions — letting a brand-new
    // project with *no product definition at all* pass the D34 approve gate. An
    // unstarted oracle is "incomplete", not "complete": the spec-convergence loop
    // (sdi-init → sdi-converge) must first author the backbone the gate measures.
    let has_backbone = !personas.is_empty() && !capabilities.is_empty();
    // L2 (flow-step → scenario) coverage is computed in 4b; flagged here so the
    // verdict never reads "complete" while L2 is unenforced.
    let l2_enforced = false;

    Ok(Json(json!({
        "project_id": pid,
        "l0": {
            "facet_incomplete_nodes": facet_incomplete,
            "dangling_edges": dangling,
            "complete": l0_complete,
        },
        "l1": {
            "uncovered_persona_capability_pairs": l1_uncovered,
            "complete": l1_complete,
            "has_backbone": has_backbone,
            "persona_count": personas.len(),
            "capability_count": capabilities.len(),
        },
        "questions": { "open": open_questions, "clear": questions_clear },
        "l2": { "enforced": l2_enforced },
        "oracle_complete": has_backbone && l0_complete && l1_complete && questions_clear,
    })))
}
