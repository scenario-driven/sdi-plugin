//! `sdi decision …` — D12 append-only ADR log client.

use crate::cli::{DecisionCmd, DecisionCreateArgs, DecisionSupersedeArgs};
use crate::http::Client;
use crate::output::emit;
use anyhow::Result;
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: DecisionCmd, quiet: bool) -> Result<()> {
    match cmd {
        DecisionCmd::Create(args) => create(cli, args, quiet).await,
        DecisionCmd::List { plan_id } => list(cli, &plan_id).await,
        DecisionCmd::View { id } => view(cli, &id, quiet).await,
        DecisionCmd::Supersede(args) => supersede(cli, args, quiet).await,
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
