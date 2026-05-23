//! `sdi decision …` — D12 append-only ADR log client.
//!
//! D28 adds a `rollback` action that appends a consensus row whose
//! `reversal_of` points at the original. The original decision is never
//! mutated — D12 SNAPSHOT-ONLY remains intact — and the daemon emits
//! `rollback_initiated` + `rollback_completed` SSE events around the write.

use crate::cli::{DecisionCmd, DecisionCreateArgs, DecisionRollbackArgs, DecisionSupersedeArgs};
use crate::http::Client;
use crate::output::emit;
use anyhow::{Context, Result};
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: DecisionCmd, quiet: bool) -> Result<()> {
    match cmd {
        DecisionCmd::Create(args) => create(cli, args, quiet).await,
        DecisionCmd::List { plan_id } => list(cli, &plan_id).await,
        DecisionCmd::View { id } => view(cli, &id, quiet).await,
        DecisionCmd::Supersede(args) => supersede(cli, args, quiet).await,
        DecisionCmd::Rollback(args) => rollback(cli, args, quiet).await,
    }
}

async fn create(cli: &Client, args: DecisionCreateArgs, quiet: bool) -> Result<()> {
    let body = json!({
        "plan_id": args.plan_id,
        "short_code": args.short_code,
        "title": args.title,
        "body": args.body,
    });
    let v: Value = cli.post_json("/decisions", &body).await?;
    emit(&v, quiet)
}

async fn list(cli: &Client, plan_id: &str) -> Result<()> {
    let v: Value = cli
        .get_json(&format!("/decisions?plan_id={}", plan_id))
        .await?;
    emit(&v, false)
}

async fn view(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli.get_json(&format!("/decisions/{}", id)).await?;
    emit(&v, quiet)
}

async fn supersede(cli: &Client, args: DecisionSupersedeArgs, quiet: bool) -> Result<()> {
    // The daemon does the auto-flip of the predecessor to `superseded` when
    // `supersedes_id` is present on the create body.
    let body = json!({
        "plan_id": args.plan_id,
        "short_code": args.short_code,
        "title": args.title,
        "body": args.body,
        "supersedes_id": args.prior_id,
    });
    let v: Value = cli.post_json("/decisions", &body).await?;
    emit(&v, quiet)
}

async fn rollback(cli: &Client, args: DecisionRollbackArgs, quiet: bool) -> Result<()> {
    let reversal_plan = match (args.reversal_plan, args.reversal_plan_from_file) {
        (Some(s), _) => s,
        (None, Some(path)) => std::fs::read_to_string(&path)
            .with_context(|| format!("read --reversal-plan-from-file: {}", path))?
            .trim()
            .to_string(),
        (None, None) => {
            anyhow::bail!(
                "`sdi decision rollback` requires --reversal-plan or --reversal-plan-from-file"
            )
        }
    };
    let mut body = json!({
        "short_code": args.short_code,
        "title": args.title,
        "body": args.body,
        "reversal_plan": reversal_plan,
        "agent_name": args.agent_name,
    });
    if let Some(score) = args.blast_radius_score {
        body["blast_radius_score"] = json!(score);
    }
    if let Some(pid) = args.produced_via_pattern_id {
        body["produced_via_pattern_id"] = Value::String(pid);
    }
    let v: Value = cli
        .post_json(&format!("/decisions/{}/rollback", args.id), &body)
        .await?;
    emit(&v, quiet)
}
