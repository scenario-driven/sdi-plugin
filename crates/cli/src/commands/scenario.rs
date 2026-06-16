//! `sdi scenario …` — D5 GWT-strict authoring + confirm client.

use crate::cli::{ScenarioCmd, ScenarioCreateArgs, ScenarioSearchArgs, ScenarioUpdateArgs};
use crate::http::Client;
use crate::output::emit;
use anyhow::Result;
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: ScenarioCmd, quiet: bool) -> Result<()> {
    match cmd {
        ScenarioCmd::Create(args) => create(cli, args, quiet).await,
        ScenarioCmd::List { plan_id } => list(cli, &plan_id).await,
        ScenarioCmd::View { id } => view(cli, &id, quiet).await,
        ScenarioCmd::Update(args) => update(cli, args, quiet).await,
        ScenarioCmd::Confirm { id } => confirm(cli, &id, quiet).await,
        ScenarioCmd::Retire { id } => retire(cli, &id, quiet).await,
        ScenarioCmd::Unretire { id } => unretire(cli, &id, quiet).await,
        ScenarioCmd::Search(args) => search(cli, args).await,
        ScenarioCmd::Claim { id } => claim(cli, &id, quiet).await,
        ScenarioCmd::Release { id } => release(cli, &id, quiet).await,
    }
}

async fn create(cli: &Client, args: ScenarioCreateArgs, quiet: bool) -> Result<()> {
    let body = json!({
        "plan_id": args.plan_id,
        "short_code": args.short_code,
        "given": args.given,
        "when": args.when_,
        "then": args.then_,
        "confirmed": args.confirmed,
    });
    let v: Value = cli.post_json("/scenarios", &body).await?;
    emit(&v, quiet)
}

async fn list(cli: &Client, plan_id: &str) -> Result<()> {
    let v: Value = cli
        .get_json(&format!("/scenarios?plan_id={}", plan_id))
        .await?;
    emit(&v, false)
}

async fn view(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli.get_json(&format!("/scenarios/{}", id)).await?;
    emit(&v, quiet)
}

async fn update(cli: &Client, args: ScenarioUpdateArgs, quiet: bool) -> Result<()> {
    if args.given.is_none() && args.when_.is_none() && args.then_.is_none() {
        anyhow::bail!("scenario update requires at least one of --given, --when, --then");
    }
    let current: Value = cli.get_json(&format!("/scenarios/{}", args.id)).await?;
    let given = args
        .given
        .unwrap_or_else(|| current["given"].as_str().unwrap_or("").to_string());
    let when_ = args
        .when_
        .unwrap_or_else(|| current["when"].as_str().unwrap_or("").to_string());
    let then_ = args
        .then_
        .unwrap_or_else(|| current["then"].as_str().unwrap_or("").to_string());
    let v: Value = cli
        .put_json(
            &format!("/scenarios/{}", args.id),
            &json!({ "given": given, "when": when_, "then": then_ }),
        )
        .await?;
    emit(&v, quiet)
}

async fn confirm(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli
        .post_empty(&format!("/scenarios/{}/confirm", id))
        .await?;
    emit(&v, quiet)
}

async fn retire(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli.post_empty(&format!("/scenarios/{}/retire", id)).await?;
    emit(&v, quiet)
}

async fn unretire(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli
        .post_empty(&format!("/scenarios/{}/unretire", id))
        .await?;
    emit(&v, quiet)
}

async fn claim(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    // D29 — caller is expected to have first checked `/scenarios/active-claims`
    // for overlap. The daemon does not yet compute glob intersection itself,
    // so the PreToolUse hook (sdi-hooks.cjs) is the enforcement surface that
    // blocks Edit/Write against paths held by another scenario.
    let v: Value = cli.post_empty(&format!("/scenarios/{}/claim", id)).await?;
    emit(&v, quiet)
}

async fn release(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli
        .post_empty(&format!("/scenarios/{}/release", id))
        .await?;
    emit(&v, quiet)
}

async fn search(cli: &Client, args: ScenarioSearchArgs) -> Result<()> {
    // Daemon's /scenarios/search uses FTS5 MATCH. URL-encode the query so
    // operators (AND/OR/quotes) survive the wire intact.
    let path = format!(
        "/scenarios/search?plan_id={}&q={}&limit={}",
        args.plan_id,
        urlencode(&args.query),
        args.limit,
    );
    let v: Value = cli.get_json(&path).await?;
    emit(&v, false)
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
