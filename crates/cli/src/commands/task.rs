//! `sdi task …` — D3 runtime task client. `task complete` requires evidence
//! (PRD §6.6); the evidence format is `SCN-ID=result@evidence_ref` repeated.

use crate::cli::{
    TaskCmd, TaskCompleteArgs, TaskCreateArgs, TaskDecomposeArgs, TaskLeaseArgs,
    TaskLeaseReleaseArgs, TaskPreflightArgs, TaskRelateArgs,
};
use crate::http::Client;
use crate::output::emit;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: TaskCmd, quiet: bool) -> Result<()> {
    match cmd {
        TaskCmd::Create(args) => create(cli, args, quiet).await,
        TaskCmd::List { round_id } => list(cli, &round_id).await,
        TaskCmd::View { id } => view(cli, &id, quiet).await,
        TaskCmd::Start { id } => set_status(cli, &id, "in_progress", quiet).await,
        TaskCmd::Block { id } => set_status(cli, &id, "blocked", quiet).await,
        TaskCmd::Cancel { id } => set_status(cli, &id, "cancelled", quiet).await,
        TaskCmd::Complete(args) => complete(cli, args, quiet).await,
        TaskCmd::Decompose(args) => decompose(cli, args).await,
        TaskCmd::Ancestors { id } => simple_get(cli, &format!("/tasks/{id}/ancestors")).await,
        TaskCmd::Descendants { id } => simple_get(cli, &format!("/tasks/{id}/descendants")).await,
        TaskCmd::Subtree { id } => simple_get(cli, &format!("/tasks/{id}/subtree")).await,
        TaskCmd::Relations { id } => simple_get(cli, &format!("/tasks/{id}/relations")).await,
        TaskCmd::Relate(args) => relate(cli, args, quiet).await,
        TaskCmd::Unrelate { relation_id } => {
            let v: Value = cli
                .delete_json(&format!("/relations/{relation_id}"))
                .await?;
            emit(&v, quiet)
        }
        TaskCmd::Lease(args) => lease_action(cli, args, "/lease").await,
        TaskCmd::Heartbeat(args) => lease_action(cli, args, "/lease/heartbeat").await,
        TaskCmd::Release(args) => release(cli, args).await,
        TaskCmd::LeaseInfo { id } => simple_get(cli, &format!("/tasks/{id}/lease")).await,
        TaskCmd::Stats(sel) => {
            let project_id = crate::commands::aggregate::resolve_project_id(cli, &sel).await?;
            simple_get(cli, &format!("/tasks/stats?project_id={project_id}")).await
        }
        TaskCmd::Preflight(args) => preflight(cli, args).await,
    }
}

async fn create(cli: &Client, args: TaskCreateArgs, quiet: bool) -> Result<()> {
    let body = json!({
        "round_id": args.round_id,
        "short_code": args.short_code,
        "description": args.description,
        "parent_scenario_ids": args.scenarios,
        "parent_requirement_ids": args.requirements,
    });
    let v: Value = cli.post_json("/tasks", &body).await?;
    emit(&v, quiet)
}

async fn list(cli: &Client, round_id: &str) -> Result<()> {
    let v: Value = cli
        .get_json(&format!("/tasks?round_id={}", round_id))
        .await?;
    emit(&v, false)
}

async fn view(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli.get_json(&format!("/tasks/{}", id)).await?;
    emit(&v, quiet)
}

async fn set_status(cli: &Client, id: &str, status: &str, quiet: bool) -> Result<()> {
    let v: Value = cli
        .post_json(
            &format!("/tasks/{}/status", id),
            &json!({ "status": status }),
        )
        .await?;
    emit(&v, quiet)
}

async fn complete(cli: &Client, args: TaskCompleteArgs, quiet: bool) -> Result<()> {
    let mut scenarios = Vec::with_capacity(args.evidence.len());
    for spec in &args.evidence {
        scenarios.push(parse_evidence(spec)?);
    }
    let mut body = json!({ "evidence": { "scenarios": scenarios } });
    if let Some(s) = args.summary {
        body["evidence"]["summary"] = Value::String(s);
    }
    let v: Value = cli
        .post_json(&format!("/tasks/{}/complete", args.id), &body)
        .await?;
    emit(&v, quiet)
}

async fn decompose(cli: &Client, args: TaskDecomposeArgs) -> Result<()> {
    let mut subtasks = Vec::with_capacity(args.subtasks.len());
    for spec in &args.subtasks {
        let (short, desc) = spec
            .split_once("::")
            .ok_or_else(|| anyhow!("--subtask must be SHORTCODE::description, got: {spec}"))?;
        subtasks.push(json!({
            "short_code": short.trim(),
            "description": desc.trim(),
        }));
    }
    let mut body = json!({ "subtasks": subtasks });
    if let Some(r) = args.round_id {
        body["round_id"] = Value::String(r);
    }
    let v: Value = cli
        .post_json(&format!("/tasks/{}/decompose", args.id), &body)
        .await?;
    emit(&v, false)
}

async fn relate(cli: &Client, args: TaskRelateArgs, quiet: bool) -> Result<()> {
    let body = json!({ "child_id": args.child, "kind": args.kind });
    let v: Value = cli
        .post_json(&format!("/tasks/{}/relations", args.id), &body)
        .await?;
    emit(&v, quiet)
}

async fn lease_action(cli: &Client, args: TaskLeaseArgs, suffix: &str) -> Result<()> {
    let body = json!({ "holder": args.holder, "ttl_seconds": args.ttl_seconds });
    let v: Value = cli
        .post_json(&format!("/tasks/{}{suffix}", args.id), &body)
        .await?;
    emit(&v, false)
}

async fn release(cli: &Client, args: TaskLeaseReleaseArgs) -> Result<()> {
    let body = json!({ "holder": args.holder });
    let v: Value = cli
        .post_json(&format!("/tasks/{}/lease/release", args.id), &body)
        .await?;
    emit(&v, false)
}

async fn preflight(cli: &Client, args: TaskPreflightArgs) -> Result<()> {
    let qs = args
        .tier
        .map(|t| format!("?tier={}", t))
        .unwrap_or_default();
    let v: Value = cli
        .get_json(&format!("/tasks/{}/usage/preflight{qs}", args.id))
        .await?;
    emit(&v, false)
}

async fn simple_get(cli: &Client, path: &str) -> Result<()> {
    let v: Value = cli.get_json(path).await?;
    emit(&v, false)
}

/// Parse `SCN-ID=result@evidence_ref` into the JSON shape the daemon wants.
fn parse_evidence(spec: &str) -> Result<Value> {
    let (head, evidence_ref) = spec.split_once('@').ok_or_else(|| {
        anyhow::anyhow!("--evidence must be SCN-ID=result@evidence_ref, got: {spec}")
    })?;
    let (scn, result) = head.split_once('=').ok_or_else(|| {
        anyhow::anyhow!("--evidence must be SCN-ID=result@evidence_ref, got: {spec}")
    })?;
    Ok(json!({
        "scenario_id": scn,
        "result": result,
        "evidence_ref": evidence_ref,
    }))
}
