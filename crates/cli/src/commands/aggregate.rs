//! `sdi dashboard | handoff | summary | board | wiki | timeline | metrics | replay`.
//! Read-only views that pull pre-computed aggregates from the daemon.

use crate::cli::{
    BoardArgs, DashboardArgs, HandoffArgs, ProjectSelector, ReplayArgs, SummaryArgs, TimelineArgs,
    WikiArgs,
};
use crate::http::Client;
use crate::output::emit;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

pub async fn dashboard(cli: &Client, args: DashboardArgs) -> Result<()> {
    let path = build_dashboard_path(
        "/dashboard",
        args.selector.cwd.as_deref(),
        args.selector.explicit(),
    )?;
    let v: Value = cli.get_json(&path).await?;
    emit(&v, false)
}

pub async fn handoff(cli: &Client, args: HandoffArgs) -> Result<()> {
    let project_id = resolve_project_id(cli, &args.selector).await?;
    let v: Value = cli
        .get_json(&format!("/projects/{project_id}/handoff"))
        .await?;
    emit(&v, false)
}

pub async fn next(cli: &Client, args: HandoffArgs) -> Result<()> {
    let project_id = resolve_project_id(cli, &args.selector).await?;
    let v: Value = cli
        .get_json(&format!("/projects/{project_id}/next"))
        .await?;
    emit(&v, false)
}

pub async fn summary(cli: &Client, args: SummaryArgs) -> Result<()> {
    let path = build_dashboard_path(
        "/dashboard",
        args.selector.cwd.as_deref(),
        args.selector.explicit(),
    )?;
    let raw: Value = cli.get_json(&path).await?;
    // Reshape: counts + active plan only.
    let summary = json!({
        "project": raw.get("project").cloned().unwrap_or(Value::Null),
        "active_plan": raw.get("active_plan").cloned().unwrap_or(Value::Null),
        "counts": raw.get("counts").cloned().unwrap_or(Value::Null),
        "task_status": raw.get("task_status").cloned().unwrap_or(Value::Null),
    });
    emit(&summary, false)
}

pub async fn board(cli: &Client, args: BoardArgs) -> Result<()> {
    let project_id = resolve_project_id(cli, &args.selector).await?;
    let v: Value = cli
        .get_json(&format!("/projects/{project_id}/handoff"))
        .await?;
    let board = json!({
        "in_flight_tasks": v.get("in_flight_tasks").cloned().unwrap_or(Value::Array(vec![])),
        "backlog_tasks": v.get("backlog_tasks").cloned().unwrap_or(Value::Array(vec![])),
    });
    emit(&board, false)
}

pub async fn wiki(cli: &Client, args: WikiArgs) -> Result<()> {
    let project_id = resolve_project_id(cli, &args.selector).await?;
    let v: Value = cli
        .get_json(&format!(
            "/knowledge/export?project_id={project_id}&scope=rag"
        ))
        .await?;
    emit(&v, false)
}

pub async fn timeline(cli: &Client, args: TimelineArgs) -> Result<()> {
    let project_id = resolve_project_id(cli, &args.selector).await?;
    let v: Value = cli
        .get_json(&format!(
            "/activity?project_id={project_id}&limit={}",
            args.limit
        ))
        .await?;
    emit(&v, false)
}

/// Resolve a [`ProjectSelector`] to a concrete `PROJ-…` id by routing through
/// `/dashboard`, which already accepts a project id, a project key, or a cwd
/// path and returns the matched project. Centralizing resolution here lets the
/// per-project view endpoints (handoff / board / wiki / timeline / task stats)
/// accept the same id-or-key-or-cwd selector as the dashboard.
pub(crate) async fn resolve_project_id(cli: &Client, sel: &ProjectSelector) -> Result<String> {
    let path = build_dashboard_path("/dashboard", sel.cwd.as_deref(), sel.explicit())?;
    let v: Value = cli.get_json(&path).await?;
    v.get("project")
        .and_then(|p| p.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("no project matched the given selector (id / key / cwd)"))
}

pub async fn metrics(cli: &Client) -> Result<()> {
    // Fetch as plain text — bypass JSON decode.
    let url = format!("{}{}", cli.base(), "/metrics");
    let body = reqwest::get(&url).await?.text().await?;
    print!("{body}");
    Ok(())
}

pub async fn replay(cli: &Client, args: ReplayArgs) -> Result<()> {
    let mut path = format!("/events/replay?limit={}", args.limit);
    if let Some(p) = args.project_id {
        path.push_str(&format!("&project_id={p}"));
    }
    if let Some(s) = args.since {
        path.push_str(&format!("&since={s}"));
    }
    let v: Value = cli.get_json(&path).await?;
    emit(&v, false)
}

fn build_dashboard_path(base: &str, cwd: Option<&str>, project: Option<&str>) -> Result<String> {
    let mut path = base.to_string();
    let mut sep = '?';
    if let Some(c) = cwd {
        path.push(sep);
        path.push_str(&format!("cwd={}", urlencode(c)));
        sep = '&';
    } else if project.is_none() {
        // Default: current working directory.
        let here = std::env::current_dir()?.display().to_string();
        path.push(sep);
        path.push_str(&format!("cwd={}", urlencode(&here)));
        sep = '&';
    }
    if let Some(p) = project {
        path.push(sep);
        path.push_str(&format!("project={p}"));
    }
    Ok(path)
}

fn urlencode(s: &str) -> String {
    // Minimal: percent-encode only what's needed for query params.
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
