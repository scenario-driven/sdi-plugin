//! `sdi agent-note …` — M1 blackboard + M2 hand-off receipts client.

use crate::cli::{AgentNoteAppendArgs, AgentNoteCmd, AgentNoteListArgs, AgentNoteRetireArgs};
use crate::http::Client;
use crate::output::emit;
use anyhow::Result;
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: AgentNoteCmd, quiet: bool) -> Result<()> {
    match cmd {
        AgentNoteCmd::Append(args) => append(cli, args, quiet).await,
        AgentNoteCmd::List(args) => list(cli, args).await,
        AgentNoteCmd::Handoffs { to_agent } => handoffs(cli, &to_agent).await,
        AgentNoteCmd::Ack { id } => ack(cli, &id, quiet).await,
        AgentNoteCmd::Retire(args) => retire(cli, args, quiet).await,
    }
}

async fn append(cli: &Client, args: AgentNoteAppendArgs, quiet: bool) -> Result<()> {
    let mut body = json!({
        "project_id": args.project_id,
        "scope_kind": args.scope,
        "kind": args.kind,
        "from_agent": args.from,
        "body": args.body,
    });
    if let Some(p) = args.plan_id {
        body["plan_id"] = Value::String(p);
    }
    if let Some(r) = args.round_id {
        body["round_id"] = Value::String(r);
    }
    if let Some(s) = args.scenario_id {
        body["scenario_id"] = Value::String(s);
    }
    if let Some(t) = args.task_id {
        body["task_id"] = Value::String(t);
    }
    if let Some(to) = args.to {
        body["to_agent"] = Value::String(to);
    }
    let v: Value = cli.post_json("/agent_notes", &body).await?;
    emit(&v, quiet)
}

async fn list(cli: &Client, args: AgentNoteListArgs) -> Result<()> {
    let mut qs = format!(
        "scope_kind={}&anchor_id={}",
        urlencode(&args.scope),
        urlencode(&args.anchor)
    );
    if let Some(k) = args.kind {
        qs.push_str(&format!("&kind={}", urlencode(&k)));
    }
    let v: Value = cli.get_json(&format!("/agent_notes?{}", qs)).await?;
    emit(&v, false)
}

async fn handoffs(cli: &Client, to_agent: &str) -> Result<()> {
    let v: Value = cli
        .get_json(&format!(
            "/agent_notes/handoffs?to_agent={}",
            urlencode(to_agent)
        ))
        .await?;
    emit(&v, false)
}

async fn ack(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli
        .post_json(&format!("/agent_notes/{}/ack", id), &json!({}))
        .await?;
    emit(&v, quiet)
}

async fn retire(cli: &Client, args: AgentNoteRetireArgs, quiet: bool) -> Result<()> {
    let body = json!({ "reason": args.reason });
    let v: Value = cli
        .post_json(&format!("/agent_notes/{}/retire", args.id), &body)
        .await?;
    emit(&v, quiet)
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{:02X}", b)
            }
        })
        .collect()
}
