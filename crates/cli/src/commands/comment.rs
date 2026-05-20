//! `sdi comment …` — polymorphic-anchor comments (plan/task/scenario/round).

use crate::cli::{CommentCmd, CommentCreateArgs, CommentListArgs, CommentUpdateArgs};
use crate::http::Client;
use crate::output::emit;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: CommentCmd, quiet: bool) -> Result<()> {
    match cmd {
        CommentCmd::Create(args) => create(cli, args, quiet).await,
        CommentCmd::List(args) => list(cli, args).await,
        CommentCmd::View { id } => {
            let v: Value = cli.get_json(&format!("/comments/{id}")).await?;
            emit(&v, quiet)
        }
        CommentCmd::Update(args) => update(cli, args, quiet).await,
        CommentCmd::Delete { id } => {
            let v: Value = cli.delete_json(&format!("/comments/{id}")).await?;
            emit(&v, quiet)
        }
    }
}

fn parse_anchor(spec: &str) -> Result<(&str, &str)> {
    let (kind, id) = spec
        .split_once('=')
        .ok_or_else(|| anyhow!("--on must be kind=ID, got: {spec}"))?;
    match kind {
        "plan" | "task" | "scenario" | "round" => Ok((kind, id)),
        other => Err(anyhow!(
            "anchor kind must be plan|task|scenario|round, got: {other}"
        )),
    }
}

async fn create(cli: &Client, args: CommentCreateArgs, quiet: bool) -> Result<()> {
    let (kind, id) = parse_anchor(&args.on)?;
    let mut body = json!({
        "project_id": args.project_id,
        "author": args.author,
        "body": args.body,
    });
    body[format!("{kind}_id")] = Value::String(id.to_string());
    let v: Value = cli.post_json("/comments", &body).await?;
    emit(&v, quiet)
}

async fn list(cli: &Client, args: CommentListArgs) -> Result<()> {
    let path = if let Some(on) = &args.on {
        let (kind, id) = parse_anchor(on)?;
        format!("/comments?{kind}_id={id}")
    } else if let Some(pid) = &args.project_id {
        format!("/comments?project_id={pid}")
    } else {
        return Err(anyhow!("--on or --project-id required"));
    };
    let v: Value = cli.get_json(&path).await?;
    emit(&v, false)
}

async fn update(cli: &Client, args: CommentUpdateArgs, quiet: bool) -> Result<()> {
    let v: Value = cli
        .put_json(
            &format!("/comments/{}", args.id),
            &json!({ "body": args.body }),
        )
        .await?;
    emit(&v, quiet)
}
