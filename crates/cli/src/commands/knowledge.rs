//! `sdi knowledge …` — PRD §5.4 scope-aware knowledge client.

use crate::cli::{
    KnowledgeCmd, KnowledgeCreateArgs, KnowledgeListArgs, KnowledgeSearchArgs,
    KnowledgeUpdateArgs,
};
use crate::http::Client;
use crate::output::emit;
use anyhow::Result;
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: KnowledgeCmd, quiet: bool) -> Result<()> {
    match cmd {
        KnowledgeCmd::Create(args) => create(cli, args, quiet).await,
        KnowledgeCmd::List(args) => list(cli, args).await,
        KnowledgeCmd::View { id } => view(cli, &id, quiet).await,
        KnowledgeCmd::Update(args) => update(cli, args, quiet).await,
        KnowledgeCmd::Delete { id } => delete(cli, &id).await,
        KnowledgeCmd::Search(args) => search(cli, args).await,
    }
}

async fn create(cli: &Client, args: KnowledgeCreateArgs, quiet: bool) -> Result<()> {
    let mut body = json!({
        "project_id": args.project_id,
        "scope": args.scope,
        "kind": args.kind,
        "title": args.title,
        "body": args.body,
        "tags": args.tags,
    });
    if let Some(s) = args.source {
        body["source_ref"] = Value::String(s);
    }
    let v: Value = cli.post_json("/knowledge", &body).await?;
    emit(&v, quiet)
}

async fn list(cli: &Client, args: KnowledgeListArgs) -> Result<()> {
    let mut q = format!("/knowledge?project_id={}", args.project_id);
    if let Some(s) = args.scope {
        q.push_str(&format!("&scope={}", s));
    }
    let v: Value = cli.get_json(&q).await?;
    emit(&v, false)
}

async fn view(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli.get_json(&format!("/knowledge/{}", id)).await?;
    emit(&v, quiet)
}

async fn update(cli: &Client, args: KnowledgeUpdateArgs, quiet: bool) -> Result<()> {
    let body = json!({
        "title": args.title,
        "body": args.body,
        "tags": args.tags,
    });
    let v: Value = cli
        .put_json(&format!("/knowledge/{}", args.id), &body)
        .await?;
    emit(&v, quiet)
}

async fn delete(cli: &Client, id: &str) -> Result<()> {
    let v: Value = cli.delete_json(&format!("/knowledge/{}", id)).await?;
    emit(&v, false)
}

async fn search(cli: &Client, args: KnowledgeSearchArgs) -> Result<()> {
    // The daemon's /knowledge/search uses FTS5 MATCH; encode the query so
    // operators like AND/OR survive transport.
    let mut q = format!(
        "/knowledge/search?project_id={}&q={}&limit={}",
        args.project_id,
        urlencode(&args.q),
        args.limit
    );
    if let Some(s) = args.scope {
        q.push_str(&format!("&scope={}", s));
    }
    let v: Value = cli.get_json(&q).await?;
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
