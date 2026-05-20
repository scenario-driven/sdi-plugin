//! `sdi usage …` — token/tool-call accounting.

use crate::cli::{UsageCmd, UsageListArgs, UsageRecordArgs};
use crate::http::Client;
use crate::output::emit;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: UsageCmd, quiet: bool) -> Result<()> {
    match cmd {
        UsageCmd::Record(args) => record(cli, args, quiet).await,
        UsageCmd::List(args) => list(cli, args).await,
        UsageCmd::Plan { plan_id } => {
            let v: Value = cli.get_json(&format!("/plans/{plan_id}/usage")).await?;
            emit(&v, false)
        }
    }
}

async fn record(cli: &Client, args: UsageRecordArgs, quiet: bool) -> Result<()> {
    let mut body = json!({
        "project_id": args.project_id,
        "model": args.model,
        "input_tokens": args.input_tokens,
        "output_tokens": args.output_tokens,
        "cache_read": args.cache_read,
        "cache_write": args.cache_write,
        "tool_calls": args.tool_calls,
        "cost_usd": args.cost_usd,
    });
    if let Some(p) = args.plan_id {
        body["plan_id"] = Value::String(p);
    }
    if let Some(t) = args.task_id {
        body["task_id"] = Value::String(t);
    }
    if let Some(r) = args.run_id {
        body["run_id"] = Value::String(r);
    }
    if let Some(t) = args.tier {
        body["tier"] = Value::String(t);
    }
    let v: Value = cli.post_json("/usage", &body).await?;
    emit(&v, quiet)
}

async fn list(cli: &Client, args: UsageListArgs) -> Result<()> {
    let path = if let Some(t) = args.task_id {
        format!("/usage?task_id={t}")
    } else if let Some(p) = args.plan_id {
        format!("/usage?plan_id={p}")
    } else if let Some(p) = args.project_id {
        format!("/usage?project_id={p}")
    } else {
        return Err(anyhow!("--project-id, --plan-id, or --task-id required"));
    };
    let v: Value = cli.get_json(&path).await?;
    emit(&v, false)
}
