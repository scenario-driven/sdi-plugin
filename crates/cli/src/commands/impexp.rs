//! `sdi export | import` — JSON round-trip for plan + knowledge bundles.

use crate::cli::{
    ExportCmd, ExportKnowledgeArgs, ImportCmd, ImportKnowledgeArgs, ImportPlansArgs,
};
use crate::http::Client;
use crate::output::emit;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::Read;

pub async fn export(cli: &Client, cmd: ExportCmd) -> Result<()> {
    match cmd {
        ExportCmd::Plans { project_id } => {
            let v: Value = cli
                .get_json(&format!("/plans/export?project_id={project_id}"))
                .await?;
            emit(&v, false)
        }
        ExportCmd::Knowledge(args) => export_knowledge(cli, args).await,
    }
}

async fn export_knowledge(cli: &Client, args: ExportKnowledgeArgs) -> Result<()> {
    let mut path = format!("/knowledge/export?project_id={}", args.project_id);
    if let Some(s) = args.scope {
        path.push_str(&format!("&scope={s}"));
    }
    let v: Value = cli.get_json(&path).await?;
    emit(&v, false)
}

pub async fn import(cli: &Client, cmd: ImportCmd) -> Result<()> {
    match cmd {
        ImportCmd::Plans(args) => import_plans(cli, args).await,
        ImportCmd::Knowledge(args) => import_knowledge(cli, args).await,
    }
}

fn read_input(path: &str) -> Result<String> {
    if path == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        Ok(buf)
    } else {
        std::fs::read_to_string(path).with_context(|| format!("read {path}"))
    }
}

async fn import_plans(cli: &Client, args: ImportPlansArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let payload: Value = serde_json::from_str(&raw).context("parse import JSON")?;
    let plans = payload
        .get("plans")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    let mut body = json!({ "plans": plans });
    if let Some(p) = args.project_id {
        body["project_id"] = Value::String(p);
    }
    let v: Value = cli.post_json("/plans/import", &body).await?;
    emit(&v, false)
}

async fn import_knowledge(cli: &Client, args: ImportKnowledgeArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let payload: Value = serde_json::from_str(&raw).context("parse import JSON")?;
    let knowledge = payload
        .get("knowledge")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    let mut body = json!({ "knowledge": knowledge });
    if let Some(p) = args.project_id {
        body["project_id"] = Value::String(p);
    }
    let v: Value = cli.post_json("/knowledge/import", &body).await?;
    emit(&v, false)
}
