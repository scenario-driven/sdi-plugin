//! `sdi chore …` — the lightweight maintenance lane (#18).
//!
//! `sdi chore "<desc>"` creates a chore already `in_progress` under the cwd's
//! project (no scenario/round needed), so a trivial consistency edit satisfies
//! the active-task PreToolUse gate when no real plan is active. `sdi chore list`
//! shows in-flight chores; `sdi chore done <id> --note` completes one.

use crate::cli::{ChoreArgs, ChoreCmd, ChoreDoneArgs, ProjectSelector};
use crate::commands::aggregate::resolve_project_id;
use crate::http::Client;
use crate::output::emit;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

pub async fn run(cli: &Client, args: ChoreArgs, quiet: bool) -> Result<()> {
    match args.cmd {
        Some(ChoreCmd::List) => list(cli, args.cwd).await,
        Some(ChoreCmd::Done(done_args)) => done(cli, done_args, quiet).await,
        None => {
            let description = args
                .description
                .ok_or_else(|| anyhow!("provide a description (`sdi chore \"<desc>\"`) or a subcommand (`list` / `done`)"))?;
            create(cli, description, args.cwd, quiet).await
        }
    }
}

async fn create(cli: &Client, description: String, cwd: Option<String>, quiet: bool) -> Result<()> {
    let project_id = resolve(cli, cwd).await?;
    let body = json!({ "description": description });
    let v: Value = cli
        .post_json(&format!("/projects/{project_id}/chores"), &body)
        .await?;
    emit(&v, quiet)
}

async fn list(cli: &Client, cwd: Option<String>) -> Result<()> {
    let project_id = resolve(cli, cwd).await?;
    let v: Value = cli
        .get_json(&format!("/projects/{project_id}/chores"))
        .await?;
    emit(&v, false)
}

async fn done(cli: &Client, args: ChoreDoneArgs, quiet: bool) -> Result<()> {
    let mut body = json!({});
    if let Some(note) = args.note {
        body["note"] = Value::String(note);
    }
    let v: Value = cli
        .post_json(&format!("/chores/{}/done", args.id), &body)
        .await?;
    emit(&v, quiet)
}

/// Resolve the owning project from `--cwd` (or the current directory).
async fn resolve(cli: &Client, cwd: Option<String>) -> Result<String> {
    let selector = ProjectSelector {
        project_id: None,
        project: None,
        cwd,
    };
    resolve_project_id(cli, &selector).await
}
