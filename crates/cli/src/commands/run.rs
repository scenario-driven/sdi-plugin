//! `sdi run …` — task execution attempts (start/finish/list).

use crate::cli::{RunCmd, RunFinishArgs, RunListArgs, RunStartArgs};
use crate::http::Client;
use crate::output::emit;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: RunCmd, quiet: bool) -> Result<()> {
    match cmd {
        RunCmd::Start(args) => start(cli, args, quiet).await,
        RunCmd::Finish(args) => finish(cli, args, quiet).await,
        RunCmd::View { id } => {
            let v: Value = cli.get_json(&format!("/runs/{id}")).await?;
            emit(&v, quiet)
        }
        RunCmd::List(args) => list(cli, args).await,
    }
}

async fn start(cli: &Client, args: RunStartArgs, quiet: bool) -> Result<()> {
    let mut body = json!({
        "task_id": args.task_id,
        "actor": args.actor,
    });
    if let Some(s) = args.session_id {
        body["session_id"] = Value::String(s);
    }
    let v: Value = cli.post_json("/runs", &body).await?;
    emit(&v, quiet)
}

async fn finish(cli: &Client, args: RunFinishArgs, quiet: bool) -> Result<()> {
    let mut body = json!({ "result": args.result });
    if let Some(n) = args.notes {
        body["notes"] = Value::String(n);
    }
    let v: Value = cli
        .post_json(&format!("/runs/{}/finish", args.id), &body)
        .await?;
    emit(&v, quiet)
}

async fn list(cli: &Client, args: RunListArgs) -> Result<()> {
    let path = if let Some(t) = args.task_id {
        format!("/runs?task_id={t}")
    } else if let Some(s) = args.session_id {
        format!("/runs?session_id={s}")
    } else {
        return Err(anyhow!("--task-id or --session-id required"));
    };
    let v: Value = cli.get_json(&path).await?;
    emit(&v, false)
}
