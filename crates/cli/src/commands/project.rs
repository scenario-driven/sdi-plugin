//! `sdi project …` — thin client over the `/projects` HTTP router.

use crate::cli::{ProjectCmd, ProjectCreateArgs, ProjectDeleteArgs, ProjectUpdateArgs};
use crate::http::Client;
use crate::output::emit;
use anyhow::Result;
use serde_json::{json, Map, Value};

pub async fn run(cli: &Client, cmd: ProjectCmd, quiet: bool) -> Result<()> {
    match cmd {
        ProjectCmd::Create(args) => create(cli, args, quiet).await,
        ProjectCmd::List => list(cli).await,
        ProjectCmd::View { id } => view(cli, &id, quiet).await,
        ProjectCmd::ByCwd { cwd } => by_cwd(cli, &cwd).await,
        ProjectCmd::Update(args) => update(cli, args, quiet).await,
        ProjectCmd::Disable { id } => disable(cli, &id, quiet).await,
        ProjectCmd::Enable { id } => enable(cli, &id, quiet).await,
        ProjectCmd::Delete(args) => delete(cli, args, quiet).await,
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
    let mut patch = Map::new();
    if let Some(name) = args.name {
        patch.insert("name".into(), Value::String(name));
    }
    // `--description ""` clears the field (daemon treats null = clear);
    // `--description "<text>"` sets it.
    if let Some(desc) = args.description {
        if desc.is_empty() {
            patch.insert("description".into(), Value::Null);
        } else {
            patch.insert("description".into(), Value::String(desc));
        }
    }
    if !args.wiki_paths.is_empty() {
        patch.insert(
            "wiki_paths".into(),
            Value::Array(args.wiki_paths.into_iter().map(Value::String).collect()),
        );
    }
    if patch.is_empty() {
        return Err(anyhow::anyhow!(
            "project update requires at least one of --name, --description, --wiki-path"
        ));
    }
    let v: Value = cli
        .put_json(&format!("/projects/{}", args.id), &Value::Object(patch))
        .await?;
    emit(&v, quiet)
}

async fn disable(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli
        .post_json(&format!("/projects/{}/disable", id), &json!({}))
        .await?;
    emit(&v, quiet)
}

async fn enable(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli
        .post_json(&format!("/projects/{}/enable", id), &json!({}))
        .await?;
    emit(&v, quiet)
}

async fn delete(cli: &Client, args: ProjectDeleteArgs, quiet: bool) -> Result<()> {
    let path = if args.force {
        format!("/projects/{}?force=true", args.id)
    } else {
        format!("/projects/{}", args.id)
    };
    let v: Value = cli.delete_json(&path).await?;
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
