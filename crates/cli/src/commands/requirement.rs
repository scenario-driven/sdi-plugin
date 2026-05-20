//! `sdi req …` — D12 SNAPSHOT requirement editor client.

use crate::cli::{RequirementCmd, RequirementCreateArgs, RequirementUpdateArgs};
use crate::http::Client;
use crate::output::emit;
use anyhow::Result;
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: RequirementCmd, quiet: bool) -> Result<()> {
    match cmd {
        RequirementCmd::Create(args) => create(cli, args, quiet).await,
        RequirementCmd::List { plan_id } => list(cli, &plan_id).await,
        RequirementCmd::View { id } => view(cli, &id, quiet).await,
        RequirementCmd::Update(args) => update(cli, args, quiet).await,
        RequirementCmd::Delete { id } => delete(cli, &id).await,
    }
}

async fn create(cli: &Client, args: RequirementCreateArgs, quiet: bool) -> Result<()> {
    let mut body = json!({
        "plan_id": args.plan_id,
        "short_code": args.short_code,
        "title": args.title,
        "body": args.body,
    });
    if let Some(s) = args.source {
        body["source"] = Value::String(s);
    }
    let v: Value = cli.post_json("/requirements", &body).await?;
    emit(&v, quiet)
}

async fn list(cli: &Client, plan_id: &str) -> Result<()> {
    let v: Value = cli
        .get_json(&format!("/requirements?plan_id={}", plan_id))
        .await?;
    emit(&v, false)
}

async fn view(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli.get_json(&format!("/requirements/{}", id)).await?;
    emit(&v, quiet)
}

async fn update(cli: &Client, args: RequirementUpdateArgs, quiet: bool) -> Result<()> {
    if args.title.is_none() && args.body.is_none() && args.source.is_none() {
        anyhow::bail!("req update requires at least one of --title, --body, --source");
    }
    let current: Value = cli.get_json(&format!("/requirements/{}", args.id)).await?;
    let title = args
        .title
        .unwrap_or_else(|| current["title"].as_str().unwrap_or("").to_string());
    let body = args
        .body
        .unwrap_or_else(|| current["body"].as_str().unwrap_or("").to_string());
    let source = args
        .source
        .unwrap_or_else(|| current["source"].as_str().unwrap_or("snapshot").to_string());
    let v: Value = cli
        .put_json(
            &format!("/requirements/{}", args.id),
            &json!({ "title": title, "body": body, "source": source }),
        )
        .await?;
    emit(&v, quiet)
}

async fn delete(cli: &Client, id: &str) -> Result<()> {
    let v: Value = cli.delete_json(&format!("/requirements/{}", id)).await?;
    emit(&v, false)
}
