//! `sdi dashboard | handoff | summary | board | wiki | timeline | metrics | replay`.
//! Read-only views that pull pre-computed aggregates from the daemon.

use crate::cli::{
    BoardArgs, DashboardArgs, ReplayArgs, SummaryArgs, TimelineArgs, WikiArgs,
};
use crate::http::Client;
use crate::output::emit;
use anyhow::Result;
use serde_json::{json, Value};

pub async fn dashboard(cli: &Client, args: DashboardArgs) -> Result<()> {
    let path = build_dashboard_path("/dashboard", args.cwd.as_deref(), args.project.as_deref())?;
    let v: Value = cli.get_json(&path).await?;
    emit(&v, false)
}

pub async fn handoff(cli: &Client, project_id: &str) -> Result<()> {
    let v: Value = cli
        .get_json(&format!("/projects/{project_id}/handoff"))
        .await?;
    emit(&v, false)
}

pub async fn summary(cli: &Client, args: SummaryArgs) -> Result<()> {
    let path = build_dashboard_path("/dashboard", args.cwd.as_deref(), args.project.as_deref())?;
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
    let v: Value = cli
        .get_json(&format!("/projects/{}/handoff", args.project_id))
        .await?;
    let board = json!({
        "in_flight_tasks": v.get("in_flight_tasks").cloned().unwrap_or(Value::Array(vec![])),
        "backlog_tasks": v.get("backlog_tasks").cloned().unwrap_or(Value::Array(vec![])),
    });
    emit(&board, false)
}

pub async fn wiki(cli: &Client, args: WikiArgs) -> Result<()> {
    let v: Value = cli
        .get_json(&format!(
            "/knowledge/export?project_id={}&scope=rag",
            args.project_id
        ))
        .await?;
    emit(&v, false)
}

pub async fn timeline(cli: &Client, args: TimelineArgs) -> Result<()> {
    let v: Value = cli
        .get_json(&format!(
            "/activity?project_id={}&limit={}",
            args.project_id, args.limit
        ))
        .await?;
    emit(&v, false)
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

fn build_dashboard_path(
    base: &str,
    cwd: Option<&str>,
    project: Option<&str>,
) -> Result<String> {
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
