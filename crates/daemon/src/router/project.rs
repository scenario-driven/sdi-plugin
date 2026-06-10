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
use sdi_core::error::DomainError;
use sdi_core::ids::{now, Id, IdKind};
use sdi_core::project::{default_wiki_paths, Project};
use sdi_db::repo::project as repo;
use serde::Deserialize;
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects", post(create).get(list))
        .route("/projects/by-cwd", get(by_cwd))
        .route("/projects/by-key/:key", get(by_key))
        .route("/projects/:id", get(get_one).put(update).delete(delete_one))
        .route("/projects/:id/disable", post(disable))
        .route("/projects/:id/enable", post(enable))
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
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    wiki_paths: Option<Vec<String>>,
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
        description: body.description,
        enabled: true,
        wiki_paths: body.wiki_paths.unwrap_or_else(default_wiki_paths),
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

/// PATCH-style update body. Every field is optional; `description` accepts
/// JSON `null` to clear, distinct from omitting the key entirely.
async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    let obj = body
        .as_object()
        .ok_or_else(|| DomainError::Validation("request body must be a JSON object".into()))?;

    let mut fields = repo::UpdateFields::default();

    if let Some(v) = obj.get("name") {
        let s = v
            .as_str()
            .ok_or_else(|| DomainError::Validation("`name` must be a string".into()))?;
        if s.trim().is_empty() {
            return Err(DomainError::Validation("`name` must not be empty".into()).into());
        }
        fields.name = Some(s.to_string());
    }
    if let Some(v) = obj.get("description") {
        // null → clear, string → set.
        if v.is_null() {
            fields.description = Some(None);
        } else if let Some(s) = v.as_str() {
            fields.description = Some(Some(s.to_string()));
        } else {
            return Err(
                DomainError::Validation("`description` must be string or null".into()).into(),
            );
        }
    }
    if let Some(v) = obj.get("enabled") {
        let b = if let Some(b) = v.as_bool() {
            b
        } else if let Some(i) = v.as_i64() {
            i != 0
        } else {
            return Err(
                DomainError::Validation("`enabled` must be boolean or integer".into()).into(),
            );
        };
        fields.enabled = Some(b);
    }
    if let Some(v) = obj.get("wiki_paths") {
        let arr = v.as_array().ok_or_else(|| {
            DomainError::Validation("`wiki_paths` must be an array of strings".into())
        })?;
        let mut paths = Vec::with_capacity(arr.len());
        for item in arr {
            let s = item.as_str().ok_or_else(|| {
                DomainError::Validation("`wiki_paths` must be an array of strings".into())
            })?;
            paths.push(s.to_string());
        }
        fields.wiki_paths = Some(paths);
    }

    let conn = state.conn()?;
    let pid = Id::from(id);
    repo::update_fields(&conn, &pid, &fields)?;
    let fresh = repo::get(&conn, &pid)?;
    state.publish(EventEnvelope {
        kind: "project.updated".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

async fn disable(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let pid = Id::from(id);
    repo::set_enabled(&conn, &pid, false)?;
    let fresh = repo::get(&conn, &pid)?;
    state.publish(EventEnvelope {
        kind: "project.disabled".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

async fn enable(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let conn = state.conn()?;
    let pid = Id::from(id);
    repo::set_enabled(&conn, &pid, true)?;
    let fresh = repo::get(&conn, &pid)?;
    state.publish(EventEnvelope {
        kind: "project.enabled".into(),
        entity_id: Some(fresh.id.to_string()),
        payload: serde_json::to_value(&fresh).unwrap_or(Value::Null),
    });
    Ok(Json(json!(fresh)))
}

#[derive(Debug, Deserialize, Default)]
struct DeleteQuery {
    /// `?force=true` skips the in-flight task guard.
    #[serde(default)]
    force: bool,
}

async fn delete_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<DeleteQuery>,
) -> ApiResult<Json<Value>> {
    let mut conn = state.conn()?;
    let pid = Id::from(id);
    // Confirm the row exists up front so we return NotFound instead of a
    // misleading "no in-flight tasks" success.
    let project = repo::get(&conn, &pid)?;
    if !q.force {
        let in_flight = repo::count_in_flight_tasks(&conn, &pid)?;
        if in_flight > 0 {
            return Err(DomainError::Conflict(format!(
                "{in_flight} in-flight task(s) anchored to {} — pass ?force=true to delete anyway",
                pid
            ))
            .into());
        }
    }
    repo::delete_cascade(&mut conn, &pid)?;
    state.publish(EventEnvelope {
        kind: "project.deleted".into(),
        entity_id: Some(pid.to_string()),
        payload: serde_json::to_value(&project).unwrap_or(Value::Null),
    });
    Ok(Json(
        json!({ "ok": true, "deleted": pid.to_string(), "forced": q.force }),
    ))
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
