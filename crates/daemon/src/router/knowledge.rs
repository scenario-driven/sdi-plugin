//! `/knowledge` router. PRD §5.4 — only `scope=rag` is exposed to the MCP
//! server. The HTTP daemon serves all three scopes (rag, reference, archive)
//! so CLI/web can curate them; the MCP layer is what gates LLM visibility.

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use sdi_core::error::DomainError;
use sdi_core::ids::{now, Id, IdKind};
use sdi_core::knowledge::{Knowledge, KnowledgeScope};
use sdi_db::repo::knowledge as repo;
use serde::Deserialize;
use serde_json::{json, Value};
use std::str::FromStr;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/knowledge", post(create).get(list))
        .route("/knowledge/search", get(search))
        .route(
            "/knowledge/:id",
            get(get_one).put(update).delete(delete_one),
        )
}

#[derive(Debug, Deserialize)]
struct CreateKnowledgeBody {
    project_id: String,
    #[serde(default)]
    scope: Option<String>,
    kind: String,
    title: String,
    body: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    source_ref: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    Json(b): Json<CreateKnowledgeBody>,
) -> ApiResult<Json<Value>> {
    let scope = match b.scope.as_deref() {
        Some(s) => KnowledgeScope::from_str(s)?,
        None => KnowledgeScope::default(),
    };
    let k = Knowledge {
        id: Id::new(IdKind::Knowledge),
        project_id: Id::from(b.project_id),
        scope,
        kind: b.kind,
        title: b.title,
        body: b.body,
        tags: b.tags,
        source_ref: b.source_ref,
        created_at: now(),
        updated_at: now(),
    };
    let conn = state.conn()?;
    repo::insert(&conn, &k)?;
    let fresh = repo::get(&conn, &k.id)?;
    state.publish(EventEnvelope {
        kind: "knowledge.created".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct ListKnowledgeQuery {
    project_id: String,
    #[serde(default)]
    scope: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListKnowledgeQuery>,
) -> ApiResult<Json<Value>> {
    let scope = match q.scope.as_deref() {
        Some(s) => Some(KnowledgeScope::from_str(s)?),
        None => None,
    };
    let conn = state.conn()?;
    let rows = repo::list_by_project(&conn, &Id::from(q.project_id), scope)?;
    Ok(Json(json!({ "knowledge": rows })))
}

async fn get_one(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    Ok(Json(json!(repo::get(&conn, &Id::from(id))?)))
}

#[derive(Debug, Deserialize)]
struct UpdateKnowledgeBody {
    title: String,
    body: String,
    #[serde(default)]
    tags: Vec<String>,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<UpdateKnowledgeBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let kid = Id::from(id);
    repo::update_body(&conn, &kid, &b.title, &b.body, &b.tags)?;
    let fresh = repo::get(&conn, &kid)?;
    state.publish(EventEnvelope {
        kind: "knowledge.updated".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

async fn delete_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let kid = Id::from(id);
    repo::delete(&conn, &kid)?;
    state.publish(EventEnvelope {
        kind: "knowledge.deleted".into(),
        entity_id: Some(kid.to_string()),
        payload: json!({ "id": kid.to_string() }),
    });
    Ok(Json(json!({ "deleted": true, "id": kid.to_string() })))
}

#[derive(Debug, Deserialize)]
struct SearchKnowledgeQuery {
    project_id: String,
    q: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    20
}

async fn search(
    State(state): State<AppState>,
    Query(qq): Query<SearchKnowledgeQuery>,
) -> ApiResult<Json<Value>> {
    let scope = match qq.scope.as_deref() {
        Some(s) => KnowledgeScope::from_str(s)?,
        None => KnowledgeScope::default(),
    };
    let conn = state.conn()?;
    // FTS5 forbids empty MATCH strings — surface as a clear 400.
    if qq.q.trim().is_empty() {
        return Err(DomainError::Validation("search query `q` is empty".into()).into());
    }
    let hits = repo::search(&conn, &Id::from(qq.project_id), scope, &qq.q, qq.limit)?;
    Ok(Json(json!({ "knowledge": hits })))
}
