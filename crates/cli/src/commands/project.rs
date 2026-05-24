//! `sdi project …` — thin client over the `/projects` HTTP router.

use crate::cli::{ProjectCmd, ProjectCreateArgs, ProjectUpdateArgs};
use crate::http::Client;
use crate::output::emit;
use anyhow::Result;
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: ProjectCmd, quiet: bool) -> Result<()> {
    match cmd {
        ProjectCmd::Create(args) => create(cli, args, quiet).await,
        ProjectCmd::List => list(cli).await,
        ProjectCmd::View { id } => view(cli, &id, quiet).await,
        ProjectCmd::ByCwd { cwd } => by_cwd(cli, &cwd).await,
        ProjectCmd::Update(args) => update(cli, args, quiet).await,
        ProjectCmd::CwdAttach { project_id, cwd } => attach_cwd(cli, &project_id, &cwd).await,
        ProjectCmd::CwdDetach { project_id, cwd } => detach_cwd(cli, &project_id, &cwd).await,
    }
}

async fn create(cli: &Client, args: ProjectCreateArgs, quiet: bool) -> Result<()> {
    let mut body = json!({
        "key": args.key,
        "name": args.name,
    });
    if let Some(s) = args.slug {
        body["slug"] = Value::String(s);
    }
    if !args.cwds.is_empty() {
        body["cwds"] = Value::Array(args.cwds.into_iter().map(Value::String).collect());
    }
    let v: Value = cli.post_json("/projects", &body).await?;
    emit(&v, quiet)
}

async fn list(cli: &Client) -> Result<()> {
    let v: Value = cli.get_json("/projects").await?;
    emit(&v, false)
}

async fn view(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli.get_json(&format!("/projects/{}", id)).await?;
    emit(&v, quiet)
}

async fn by_cwd(cli: &Client, cwd: &str) -> Result<()> {
    let path = format!("/projects/by-cwd?cwd={}", urlencode(cwd));
    let v: Value = cli.get_json(&path).await?;
    emit(&v, false)
}

async fn update(cli: &Client, args: ProjectUpdateArgs, quiet: bool) -> Result<()> {
    let name = args
        .name
        .ok_or_else(|| anyhow::anyhow!("project update requires --name"))?;
    let v: Value = cli
        .put_json(&format!("/projects/{}", args.id), &json!({ "name": name }))
        .await?;
    emit(&v, quiet)
}

async fn attach_cwd(cli: &Client, project_id: &str, cwd: &str) -> Result<()> {
    let v: Value = cli
        .post_json(
            &format!("/projects/{}/cwds", project_id),
            &json!({ "cwd": cwd }),
        )
        .await?;
    emit(&v, false)
}

async fn detach_cwd(cli: &Client, project_id: &str, cwd: &str) -> Result<()> {
    // axum's `.delete()` route on `/projects/:id/cwds` takes a JSON body; we
    // can't use a GET-style query, so emulate with reqwest.delete().
    let v: Value = cli
        .delete_with_body(
            &format!("/projects/{}/cwds", project_id),
            &json!({ "cwd": cwd }),
        )
        .await?;
    emit(&v, false)
}

fn urlencode(s: &str) -> String {
    // RFC 3986 unreserved characters left as-is; everything else %-encoded.
    s.bytes()
        .map(|b| {
            if matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/') {
                (b as char).to_string()
            } else {
                format!("%{:02X}", b)
            }
        })
        .collect()
}
