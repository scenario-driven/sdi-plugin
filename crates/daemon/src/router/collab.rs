//! Collaboration HTTP routers: comments, questions, activity.
//!
//! - `/comments` — polymorphic anchor (?plan_id|task_id|scenario_id|round_id)
//! - `/questions` — single-answer Q&A on plans
//! - `/activity`, `/activity/stats`, `/projects/:id/timeline` — append-only feed

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, Query, State},
    routing::{get, patch, post},
    Json, Router,
};
use sdi_core::collab::{Activity, Comment, Question, QuestionStatus};
use sdi_core::error::DomainError;
use sdi_core::ids::{now, Id, IdKind};
use sdi_db::repo::collab as repo;
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;

pub fn router() -> Router<AppState> {
    Router::new()
        // comments
        .route("/comments", post(create_comment).get(list_comments))
        .route(
            "/comments/:id",
            get(get_comment)
                .patch(update_comment)
                .delete(delete_comment),
        )
        // questions
        .route("/questions", post(create_question).get(list_questions))
        .route("/questions/:id", get(get_question))
        .route("/questions/:id/answer", post(answer_question))
        // activity / timeline
        .route("/activity", get(list_activity).post(record_activity))
        .route("/activity/stats", get(activity_stats))
        .route("/projects/:project_id/timeline", get(project_timeline))
        // method-not-allowed guards on append-only resources
        .route(
            "/activity/:id",
            patch(method_not_allowed).delete(method_not_allowed),
        )
}

async fn method_not_allowed() -> ApiResult<Json<Value>> {
    Err(DomainError::Validation("activity is append-only".into()).into())
}

// ---------------------------------------------------------------------------
// comments
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CreateCommentBody {
    project_id: String,
    #[serde(default)]
    plan_id: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    scenario_id: Option<String>,
    #[serde(default)]
    round_id: Option<String>,
    #[serde(default = "default_author")]
    author: String,
    body: String,
}

fn default_author() -> String {
    "agent".into()
}

async fn create_comment(
    State(state): State<AppState>,
    Json(b): Json<CreateCommentBody>,
) -> ApiResult<Json<Value>> {
    let c = Comment {
        id: Id::new(IdKind::Comment),
        project_id: Id::from(b.project_id),
        plan_id: b.plan_id.map(Id::from),
        task_id: b.task_id.map(Id::from),
        scenario_id: b.scenario_id.map(Id::from),
        round_id: b.round_id.map(Id::from),
        author: b.author,
        body: b.body,
        created_at: now(),
        updated_at: now(),
    };
    c.validate_anchor()?;
    let conn = state.conn()?;
    repo::insert_comment(&conn, &c)?;
    let fresh = repo::get_comment(&conn, &c.id)?;
    state.publish(EventEnvelope {
        kind: "comment.created".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct ListCommentsQuery {
    #[serde(default)]
    plan_id: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    scenario_id: Option<String>,
    #[serde(default)]
    round_id: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
}

async fn list_comments(
    State(state): State<AppState>,
    Query(q): Query<ListCommentsQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = if let Some(id) = q.plan_id {
        repo::list_comments_by(&conn, "plan_id", &Id::from(id))?
    } else if let Some(id) = q.task_id {
        repo::list_comments_by(&conn, "task_id", &Id::from(id))?
    } else if let Some(id) = q.scenario_id {
        repo::list_comments_by(&conn, "scenario_id", &Id::from(id))?
    } else if let Some(id) = q.round_id {
        repo::list_comments_by(&conn, "round_id", &Id::from(id))?
    } else if let Some(id) = q.project_id {
        repo::list_comments_by_project(&conn, &Id::from(id))?
    } else {
        return Err(DomainError::Validation(
            "one of plan_id|task_id|scenario_id|round_id|project_id required".into(),
        )
        .into());
    };
    Ok(Json(json!({ "comments": rows })))
}

async fn get_comment(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    Ok(Json(json!(repo::get_comment(&conn, &Id::from(id))?)))
}

#[derive(Debug, Deserialize)]
struct UpdateCommentBody {
    body: String,
}

async fn update_comment(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<UpdateCommentBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let cid = Id::from(id);
    repo::update_comment_body(&conn, &cid, &b.body)?;
    let fresh = repo::get_comment(&conn, &cid)?;
    state.publish(EventEnvelope {
        kind: "comment.updated".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

async fn delete_comment(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let cid = Id::from(id);
    repo::delete_comment(&conn, &cid)?;
    state.publish(EventEnvelope {
        kind: "comment.deleted".into(),
        entity_id: Some(cid.to_string()),
        payload: json!({ "id": cid.to_string() }),
    });
    Ok(Json(json!({ "deleted": cid.to_string() })))
}

// ---------------------------------------------------------------------------
// questions
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CreateQuestionBody {
    plan_id: String,
    body: String,
    #[serde(default = "default_author")]
    asker: String,
}

async fn create_question(
    State(state): State<AppState>,
    Json(b): Json<CreateQuestionBody>,
) -> ApiResult<Json<Value>> {
    let q = Question {
        id: Id::new(IdKind::Question),
        plan_id: Id::from(b.plan_id),
        asker: b.asker,
        body: b.body,
        answer: None,
        answered_by: None,
        answered_at: None,
        status: QuestionStatus::Open,
        created_at: now(),
        updated_at: now(),
    };
    let conn = state.conn()?;
    repo::insert_question(&conn, &q)?;
    let fresh = repo::get_question(&conn, &q.id)?;
    state.publish(EventEnvelope {
        kind: "question.created".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct ListQuestionsQuery {
    plan_id: String,
    #[serde(default)]
    status: Option<String>,
}

async fn list_questions(
    State(state): State<AppState>,
    Query(q): Query<ListQuestionsQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let status = q
        .status
        .as_deref()
        .map(QuestionStatus::from_str)
        .transpose()?;
    let rows = repo::list_questions_by_plan(&conn, &Id::from(q.plan_id), status)?;
    Ok(Json(json!({ "questions": rows })))
}

async fn get_question(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    Ok(Json(json!(repo::get_question(&conn, &Id::from(id))?)))
}

#[derive(Debug, Deserialize)]
struct AnswerQuestionBody {
    answer: String,
    #[serde(default = "default_author")]
    by: String,
}

async fn answer_question(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<AnswerQuestionBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let qid = Id::from(id);
    repo::answer_question(&conn, &qid, &b.answer, &b.by)?;
    let fresh = repo::get_question(&conn, &qid)?;
    state.publish(EventEnvelope {
        kind: "question.answered".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

// ---------------------------------------------------------------------------
// activity
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RecordActivityBody {
    project_id: String,
    kind: String,
    summary: String,
    #[serde(default = "default_author")]
    actor: String,
    #[serde(default)]
    entity_id: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
}

async fn record_activity(
    State(state): State<AppState>,
    Json(b): Json<RecordActivityBody>,
) -> ApiResult<Json<Value>> {
    let a = Activity {
        id: Id::new(IdKind::Activity),
        project_id: Id::from(b.project_id),
        actor: b.actor,
        kind: b.kind,
        entity_id: b.entity_id.map(Id::from),
        summary: b.summary,
        payload: b.payload.unwrap_or(Value::Null),
        created_at: now(),
    };
    let conn = state.conn()?;
    repo::insert_activity(&conn, &a)?;
    Ok(Json(json!(a)))
}

#[derive(Debug, Deserialize)]
struct ListActivityQuery {
    project_id: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    100
}

async fn list_activity(
    State(state): State<AppState>,
    Query(q): Query<ListActivityQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = repo::list_activity_by_project(&conn, &Id::from(q.project_id), q.limit)?;
    Ok(Json(json!({ "activity": rows })))
}

#[derive(Debug, Deserialize)]
struct ActivityStatsQuery {
    project_id: String,
}

async fn activity_stats(
    State(state): State<AppState>,
    Query(q): Query<ActivityStatsQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = repo::list_activity_stats(&conn, &Id::from(q.project_id))?;
    let items: Vec<_> = rows
        .into_iter()
        .map(|(k, n)| json!({ "kind": k, "count": n }))
        .collect();
    Ok(Json(json!({ "stats": items })))
}

async fn project_timeline(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Query(q): Query<TimelineQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows =
        repo::list_activity_by_project(&conn, &Id::from(project_id), q.limit.unwrap_or(200))?;
    Ok(Json(json!({ "items": rows })))
}

#[derive(Debug, Deserialize)]
struct TimelineQuery {
    #[serde(default)]
    limit: Option<usize>,
}
