//! `sdi autonomy …` — D14/D17/D18 policy client. Routes through the daemon's
//! `/autonomy_policies` surface so the L3/L4/L5 invariants stay enforced in
//! one place (the repo's `upsert` rejects L5 on forced-L4 decision kinds).

use crate::cli::{AutonomyCircuitBreakerArgs, AutonomyCmd, AutonomyGetArgs, AutonomySetArgs};
use crate::http::Client;
use crate::output::emit;
use anyhow::Result;
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: AutonomyCmd, quiet: bool) -> Result<()> {
    match cmd {
        AutonomyCmd::Set(args) => set(cli, args, quiet).await,
        AutonomyCmd::Get(args) => get(cli, args).await,
        AutonomyCmd::List { project_id } => list(cli, &project_id).await,
        AutonomyCmd::CircuitBreaker(args) => circuit_breaker(cli, args).await,
    }
}

async fn set(cli: &Client, args: AutonomySetArgs, quiet: bool) -> Result<()> {
    let mut body = json!({
        "project_id": args.project_id,
        "scope_kind": args.scope,
        "mode": args.mode,
        "set_by": args.set_by,
    });
    if let Some(p) = args.plan_id {
        body["plan_id"] = Value::String(p);
    }
    if let Some(k) = args.decision_kind {
        body["decision_kind"] = Value::String(k);
    }
    if let Some(r) = args.reason {
        body["reason"] = Value::String(r);
    }
    let v: Value = cli.post_json("/autonomy_policies", &body).await?;
    emit(&v, quiet)
}

async fn get(cli: &Client, args: AutonomyGetArgs) -> Result<()> {
    let mut qs = format!("project_id={}", urlencode(&args.project_id));
    if let Some(p) = args.plan_id {
        qs.push_str(&format!("&plan_id={}", urlencode(&p)));
    }
    if let Some(k) = args.decision_kind {
        qs.push_str(&format!("&decision_kind={}", urlencode(&k)));
    }
    let v: Value = cli
        .get_json(&format!("/autonomy_policies/resolve?{}", qs))
        .await?;
    emit(&v, false)
}

async fn list(cli: &Client, project_id: &str) -> Result<()> {
    let v: Value = cli
        .get_json(&format!(
            "/autonomy_policies?project_id={}",
            urlencode(project_id)
        ))
        .await?;
    emit(&v, false)
}

async fn circuit_breaker(cli: &Client, args: AutonomyCircuitBreakerArgs) -> Result<()> {
    let body = json!({
        "project_id": args.project_id,
        "actor": args.actor,
        "reason": args.reason,
    });
    let v: Value = cli
        .post_json("/autonomy_policies/circuit_breaker", &body)
        .await?;
    emit(&v, false)
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
