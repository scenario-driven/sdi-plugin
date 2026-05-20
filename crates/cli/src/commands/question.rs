//! `sdi question …` — open/answered Q&A anchored to a plan.

use crate::cli::{QuestionAnswerArgs, QuestionCmd, QuestionCreateArgs, QuestionListArgs};
use crate::http::Client;
use crate::output::emit;
use anyhow::Result;
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: QuestionCmd, quiet: bool) -> Result<()> {
    match cmd {
        QuestionCmd::Create(args) => create(cli, args, quiet).await,
        QuestionCmd::List(args) => list(cli, args).await,
        QuestionCmd::View { id } => {
            let v: Value = cli.get_json(&format!("/questions/{id}")).await?;
            emit(&v, quiet)
        }
        QuestionCmd::Answer(args) => answer(cli, args, quiet).await,
    }
}

async fn create(cli: &Client, args: QuestionCreateArgs, quiet: bool) -> Result<()> {
    let body = json!({
        "project_id": args.project_id,
        "plan_id": args.plan_id,
        "author": args.author,
        "body": args.body,
    });
    let v: Value = cli.post_json("/questions", &body).await?;
    emit(&v, quiet)
}

async fn list(cli: &Client, args: QuestionListArgs) -> Result<()> {
    let mut path = format!("/questions?plan_id={}", args.plan_id);
    if let Some(s) = args.status {
        path.push_str(&format!("&status={s}"));
    }
    let v: Value = cli.get_json(&path).await?;
    emit(&v, false)
}

async fn answer(cli: &Client, args: QuestionAnswerArgs, quiet: bool) -> Result<()> {
    let body = json!({ "author": args.author, "answer": args.answer });
    let v: Value = cli
        .post_json(&format!("/questions/{}/answer", args.id), &body)
        .await?;
    emit(&v, quiet)
}
