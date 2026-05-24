//! `sdi plan …` — D1/D8 plan lifecycle client.

use crate::cli::{PlanCmd, PlanCreateArgs, PlanUpdateArgs};
use crate::http::Client;
use crate::output::emit;
use anyhow::Result;
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: PlanCmd, quiet: bool) -> Result<()> {
    match cmd {
        PlanCmd::Create(args) => create(cli, args, quiet).await,
        PlanCmd::List { project_id } => list(cli, &project_id).await,
        PlanCmd::View { id } => view(cli, &id, quiet).await,
        PlanCmd::Update(args) => update(cli, args, quiet).await,
        PlanCmd::Approve { id } => transition(cli, &id, "approve", quiet).await,
        PlanCmd::Complete { id } => transition(cli, &id, "complete", quiet).await,
        PlanCmd::Active { project_id } => active(cli, &project_id).await,
        PlanCmd::Context { id } => context(cli, &id).await,
    }
}

async fn create(cli: &Client, args: PlanCreateArgs, quiet: bool) -> Result<()> {
    let body = json!({
        "project_id": args.project_id,
        "short_code": args.short_code,
        "title": args.title,
        "body": args.body,
    });
    let v: Value = cli.post_json("/plans", &body).await?;
    emit(&v, quiet)
}

async fn list(cli: &Client, project_id: &str) -> Result<()> {
    let v: Value = cli
        .get_json(&format!("/plans?project_id={}", project_id))
        .await?;
    emit(&v, false)
}

async fn view(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli.get_json(&format!("/plans/{}", id)).await?;
    emit(&v, quiet)
}

async fn update(cli: &Client, args: PlanUpdateArgs, quiet: bool) -> Result<()> {
    if args.title.is_none() && args.body.is_none() {
        anyhow::bail!("plan update requires at least one of --title or --body");
    }
    // PUT /plans/:id requires both title & body; fetch current values to fill
    // in whatever the caller omitted (SNAPSHOT-style overwrite stays atomic).
    let current: Value = cli.get_json(&format!("/plans/{}", args.id)).await?;
    let title = args
        .title
        .unwrap_or_else(|| current["title"].as_str().unwrap_or("").to_string());
    let body = args
        .body
        .unwrap_or_else(|| current["body"].as_str().unwrap_or("").to_string());
    let v: Value = cli
        .put_json(
            &format!("/plans/{}", args.id),
            &json!({ "title": title, "body": body }),
        )
        .await?;
    emit(&v, quiet)
}

async fn transition(cli: &Client, id: &str, action: &str, quiet: bool) -> Result<()> {
    let v: Value = cli.post_empty(&format!("/plans/{}/{}", id, action)).await?;
    emit(&v, quiet)
}

async fn active(cli: &Client, project_id: &str) -> Result<()> {
    let v: Value = cli
        .get_json(&format!("/projects/{}/plans/active", project_id))
        .await?;
    emit(&v, false)
}

/// Composite snapshot used by the MCP `get_plan_context` tool. The daemon
/// doesn't expose a single aggregate endpoint; we fan out and stitch the
/// four pieces so CLI consumers see the same shape an LLM consumer would.
async fn context(cli: &Client, plan_id: &str) -> Result<()> {
    let plan: Value = cli.get_json(&format!("/plans/{}", plan_id)).await?;
    let scenarios = cli
        .get_json(&format!("/scenarios?plan_id={}", plan_id))
        .await
        .ok()
        .and_then(|v: Value| v.get("scenarios").cloned())
        .unwrap_or_else(|| json!([]));
    let tasks_in_flight = cli
        .get_json(&format!("/plans/{}/tasks/in-flight", plan_id))
        .await
        .ok()
        .and_then(|v: Value| v.get("tasks").cloned())
        .unwrap_or_else(|| json!([]));
    let decisions = cli
        .get_json(&format!("/decisions?plan_id={}", plan_id))
        .await
        .ok()
        .and_then(|v: Value| v.get("decisions").cloned())
        .unwrap_or_else(|| json!([]));
    emit(
        &json!({
            "plan": plan,
            "scenarios": scenarios,
            "tasks_in_flight": tasks_in_flight,
            "decisions": decisions,
        }),
        false,
    )
}
