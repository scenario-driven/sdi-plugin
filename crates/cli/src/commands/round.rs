//! `sdi round …` — D6 round/regression client.

use crate::cli::{RoundCmd, RoundCreateArgs, RoundResultArgs};
use crate::http::Client;
use crate::output::emit;
use anyhow::Result;
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: RoundCmd, quiet: bool) -> Result<()> {
    match cmd {
        RoundCmd::Create(args) => create(cli, args, quiet).await,
        RoundCmd::List { plan_id } => list(cli, &plan_id).await,
        RoundCmd::View { id } => view(cli, &id, quiet).await,
        RoundCmd::Activate { id } => activate(cli, &id, quiet).await,
        RoundCmd::Complete { id } => transition(cli, &id, "complete", quiet).await,
        RoundCmd::Result(args) => post_result(cli, args).await,
        RoundCmd::Results { id } => list_results(cli, &id).await,
        RoundCmd::Active { plan_id } => active(cli, &plan_id).await,
    }
}

async fn create(cli: &Client, args: RoundCreateArgs, quiet: bool) -> Result<()> {
    let mut body = json!({
        "plan_id": args.plan_id,
        "short_code": args.short_code,
    });
    if let Some(m) = args.mode {
        body["mode"] = Value::String(m);
    }
    if let Some(p) = args.in_flight {
        body["in_flight_policy"] = Value::String(p);
    }
    if let Some(p) = args.disruption {
        body["disruption_policy"] = Value::String(p);
    }
    let v: Value = cli.post_json("/rounds", &body).await?;
    emit(&v, quiet)
}

async fn list(cli: &Client, plan_id: &str) -> Result<()> {
    let v: Value = cli
        .get_json(&format!("/rounds?plan_id={}", plan_id))
        .await?;
    emit(&v, false)
}

async fn view(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli.get_json(&format!("/rounds/{}", id)).await?;
    emit(&v, quiet)
}

async fn activate(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    // Daemon returns { round, carried_results }. We surface the wrapper as-is
    // so scripts can observe the carry-over count.
    let v: Value = cli.post_empty(&format!("/rounds/{}/activate", id)).await?;
    if quiet {
        if let Some(rid) = v["round"]["id"].as_str() {
            println!("{}", rid);
            return Ok(());
        }
    }
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(())
}

async fn transition(cli: &Client, id: &str, action: &str, quiet: bool) -> Result<()> {
    let v: Value = cli
        .post_empty(&format!("/rounds/{}/{}", id, action))
        .await?;
    emit(&v, quiet)
}

async fn post_result(cli: &Client, args: RoundResultArgs) -> Result<()> {
    let mut body = json!({
        "scenario_id": args.scenario,
        "result": args.result,
    });
    if let Some(e) = args.evidence {
        body["evidence_ref"] = Value::String(e);
    }
    let v: Value = cli
        .post_json(&format!("/rounds/{}/results", args.id), &body)
        .await?;
    emit(&v, false)
}

async fn list_results(cli: &Client, id: &str) -> Result<()> {
    let v: Value = cli.get_json(&format!("/rounds/{}/results", id)).await?;
    emit(&v, false)
}

async fn active(cli: &Client, plan_id: &str) -> Result<()> {
    let v: Value = cli
        .get_json(&format!("/plans/{}/rounds/active", plan_id))
        .await?;
    emit(&v, false)
}
