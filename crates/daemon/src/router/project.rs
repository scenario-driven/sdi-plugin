//! `/projects` router. Projects own a `key` (ticket prefix), a `slug`, and
//! one-or-more anchored `cwds`. Daemon-side handlers are thin wrappers over
//! `sdi_db::repo::project`; all error mapping flows through [`crate::ApiError`].

use crate::state::{AppState, EventEnvelope};
use crate::ApiResult;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use sdi_core::ids::{now, Id, IdKind};
use sdi_core::project::Project;
use sdi_db::repo::project as repo;
use serde::Deserialize;
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects", post(create).get(list))
        .route("/projects/by-cwd", get(by_cwd))
        .route("/projects/by-key/:key", get(by_key))
        .route("/projects/:id", get(get_one).put(update))
        .route(
            "/projects/:id/cwds",
            post(attach_cwd).delete(detach_cwd).get(list_cwds),
        )
}

#[derive(Debug, Deserialize)]
struct CreateProjectBody {
    key: String,
    name: String,
    slug: Option<String>,
    #[serde(default)]
    cwds: Vec<String>,
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateProjectBody>,
) -> ApiResult<Json<Value>> {
    let id = Id::new(IdKind::Project);
    let slug = body.slug.unwrap_or_else(|| slug::slugify(&body.name));
    let project = Project {
        id: id.clone(),
        key: body.key,
        name: body.name,
        slug,
        cwds: body.cwds,
        created_at: now(),
        updated_at: now(),
    };
    let conn = state.conn()?;
    repo::insert(&conn, &project)?;
    let fresh = repo::get(&conn, &project.id)?;
    state.publish(EventEnvelope {
        kind: "project.created".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

async fn list(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let rows = repo::list(&conn)?;
    Ok(Json(json!({ "projects": rows })))
}

async fn get_one(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let p = repo::get(&conn, &Id::from(id))?;
    Ok(Json(json!(p)))
}

#[derive(Debug, Deserialize)]
struct UpdateProjectBody {
    name: String,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateProjectBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let pid = Id::from(id);
    repo::update_name(&conn, &pid, &body.name)?;
    let fresh = repo::get(&conn, &pid)?;
    state.publish(EventEnvelope {
        kind: "project.updated".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize)]
struct CwdBody {
    cwd: String,
}

async fn attach_cwd(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CwdBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let pid = Id::from(id);
    repo::attach_cwd(&conn, &pid, &body.cwd)?;
    let cwds = repo::list_cwds(&conn, &pid)?;
    state.publish(EventEnvelope {
        kind: "project.cwd-attached".into(),
        entity_id: Some(pid.to_string()),
        payload: json!({ "cwd": body.cwd }),
    });
    Ok(Json(json!({ "cwds": cwds })))
}

async fn detach_cwd(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CwdBody>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let pid = Id::from(id);
    let removed = repo::detach_cwd(&conn, &pid, &body.cwd)?;
    if removed {
        state.publish(EventEnvelope {
            kind: "project.cwd-detached".into(),
            entity_id: Some(pid.to_string()),
            payload: json!({ "cwd": body.cwd }),
        });
    }
    let cwds = repo::list_cwds(&conn, &pid)?;
    Ok(Json(json!({ "cwds": cwds, "removed": removed })))
}

async fn list_cwds(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let cwds = repo::list_cwds(&conn, &Id::from(id))?;
    Ok(Json(json!({ "cwds": cwds })))
}

#[derive(Debug, Deserialize)]
struct ByCwdQuery {
    cwd: String,
}

async fn by_cwd(
    State(state): State<AppState>,
    Query(q): Query<ByCwdQuery>,
) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let found = repo::find_by_cwd(&conn, &q.cwd)?;
    Ok(Json(json!({ "project": found })))
}

async fn by_key(State(state): State<AppState>, Path(key): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let found = repo::find_by_key(&conn, &key)?;
    Ok(Json(json!({ "project": found })))
}
