//! `sdi pattern …` — D22 CollaborationPattern client. Routes through the
//! daemon's `/patterns` surface so D24 (depth/cycle), D26/D27 (shape gate at
//! `pending → active`), and parent-chained DAG semantics stay enforced in one
//! place. Tree rendering is client-side (parent→child indented walk over the
//! daemon's flat list).
//!
//! All write paths emit JSON payloads from the daemon; `tree` and filtered
//! `list` are read-only aggregations the caller asked for, so they print
//! pretty JSON regardless of `--quiet`.

use crate::cli::{
    PatternAbortArgs, PatternCmd, PatternCreateArgs, PatternListArgs, PatternTransitionArgs,
    PatternTreeArgs,
};
use crate::http::Client;
use crate::output::emit;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

pub async fn run(cli: &Client, cmd: PatternCmd, quiet: bool) -> Result<()> {
    match cmd {
        PatternCmd::Create(args) => create(cli, *args, quiet).await,
        PatternCmd::List(args) => list(cli, args).await,
        PatternCmd::Show { id } => show(cli, &id, quiet).await,
        PatternCmd::Transition(args) => transition(cli, args, quiet).await,
        PatternCmd::Abort(args) => abort(cli, args, quiet).await,
        PatternCmd::Tree(args) => tree(cli, args).await,
    }
}

// ── create ────────────────────────────────────────────────────────────────

async fn create(cli: &Client, args: PatternCreateArgs, quiet: bool) -> Result<()> {
    let mut body = json!({
        "plan_id": args.plan,
        "short_code": args.short_code,
        "kind": args.kind,
        "applies_to": args.applies_to,
        "scope_id": args.scope_id,
    });
    if let Some(p) = args.parent_pattern_id {
        body["parent_pattern_id"] = Value::String(p);
    }
    if let Some(v) = read_or_inline(args.steps_json, args.steps_from_file, "steps")? {
        body["steps"] = v;
    }
    if let Some(v) = read_or_inline(args.reviewers_json, args.reviewers_from_file, "reviewers")? {
        body["reviewers"] = v;
    }
    if let Some(v) = read_or_inline(args.fan_out_json, args.fan_out_from_file, "fan_out")? {
        body["fan_out"] = v;
    }
    if let Some(v) = read_or_inline(args.peers_json, args.peers_from_file, "peers")? {
        body["peer_registration"] = v;
    }
    let v: Value = cli.post_json("/patterns", &body).await?;
    emit(&v, quiet)
}

// ── list ──────────────────────────────────────────────────────────────────

async fn list(cli: &Client, args: PatternListArgs) -> Result<()> {
    let v: Value = if args.active {
        cli.get_json("/patterns/active").await?
    } else {
        let plan_id = args
            .plan
            .as_deref()
            .ok_or_else(|| anyhow!("`sdi pattern list` requires --plan <PLAN-ID> or --active"))?;
        cli.get_json(&format!("/patterns?plan_id={}", urlencode(plan_id)))
            .await?
    };
    let filtered = match args.kind.as_deref() {
        Some(k) => filter_by_kind(&v, k),
        None => v,
    };
    println!("{}", serde_json::to_string_pretty(&filtered)?);
    Ok(())
}

// ── show ──────────────────────────────────────────────────────────────────

async fn show(cli: &Client, id: &str, quiet: bool) -> Result<()> {
    let v: Value = cli.get_json(&format!("/patterns/{}", id)).await?;
    emit(&v, quiet)
}

// ── transition / abort ───────────────────────────────────────────────────

async fn transition(cli: &Client, args: PatternTransitionArgs, quiet: bool) -> Result<()> {
    let mut body = json!({ "to": args.to });
    if let Some(r) = args.reason {
        body["reason"] = Value::String(r);
    }
    let v: Value = patch_lifecycle(cli, &args.id, &body).await?;
    emit(&v, quiet)
}

async fn abort(cli: &Client, args: PatternAbortArgs, quiet: bool) -> Result<()> {
    let mut body = json!({ "to": "aborted" });
    if let Some(r) = args.reason {
        body["reason"] = Value::String(r);
    }
    let v: Value = patch_lifecycle(cli, &args.id, &body).await?;
    emit(&v, quiet)
}

// ── tree ──────────────────────────────────────────────────────────────────

async fn tree(cli: &Client, args: PatternTreeArgs) -> Result<()> {
    let v: Value = cli
        .get_json(&format!("/patterns?plan_id={}", urlencode(&args.plan)))
        .await?;
    let patterns = v
        .get("patterns")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    let rendered = render_tree(&patterns);
    println!("{}", rendered);
    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────

/// Choose the JSON-bearing argument (inline string vs. file path) and parse
/// it into a [`Value`]. `None` is returned when neither side is supplied —
/// the caller skips the field in that case.
fn read_or_inline(
    inline: Option<String>,
    from_file: Option<String>,
    field: &str,
) -> Result<Option<Value>> {
    let raw = match (inline, from_file) {
        (None, None) => return Ok(None),
        (Some(s), _) => s,
        (None, Some(path)) => std::fs::read_to_string(&path)
            .with_context(|| format!("read --{}-from-file: {}", field, path))?,
    };
    let v: Value =
        serde_json::from_str(raw.trim()).with_context(|| format!("parse --{}-json", field))?;
    Ok(Some(v))
}

/// Filter the `{"patterns": [...]}` envelope by `kind` server-side equivalent.
fn filter_by_kind(envelope: &Value, kind: &str) -> Value {
    let patterns = envelope
        .get("patterns")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    let filtered: Vec<Value> = patterns
        .into_iter()
        .filter(|p| p.get("kind").and_then(|k| k.as_str()) == Some(kind))
        .collect();
    json!({ "patterns": filtered })
}

/// PATCH `/patterns/:id/lifecycle`. The shared HTTP client doesn't expose a
/// PATCH verb (POST/GET/PUT/DELETE only), but the daemon registers the route
/// strictly on PATCH; we issue it inline via reqwest reusing the same base.
async fn patch_lifecycle(cli: &Client, id: &str, body: &Value) -> Result<Value> {
    let url = format!("{}/patterns/{}/lifecycle", cli.base(), id);
    let resp = reqwest::Client::new().patch(&url).json(body).send().await?;
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
            if let Some(err) = v.get("error") {
                let code = err.get("code").and_then(|c| c.as_str()).unwrap_or("ERROR");
                let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("");
                return Err(anyhow!("HTTP {} {}: {}", status.as_u16(), code, msg));
            }
        }
        return Err(anyhow!(
            "HTTP {}: {}",
            status.as_u16(),
            String::from_utf8_lossy(&bytes)
        ));
    }
    let v: Value = serde_json::from_slice(&bytes)?;
    Ok(v)
}

/// Build an indented parent→child tree from a flat pattern list. Roots are
/// rows with `parent_pattern_id == null`; children are sorted by `created_at`
/// to keep output stable. Orphans (parent missing from this plan) render as
/// extra roots prefixed with "(orphan)".
fn render_tree(patterns: &[Value]) -> String {
    use std::collections::BTreeMap;

    let mut by_parent: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    let mut by_id: BTreeMap<String, &Value> = BTreeMap::new();
    for p in patterns {
        let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if !id.is_empty() {
            by_id.insert(id.to_string(), p);
        }
        let parent = p
            .get("parent_pattern_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        by_parent.entry(parent.to_string()).or_default().push(p);
    }
    for v in by_parent.values_mut() {
        v.sort_by(|a, b| {
            let ka = a.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
            let kb = b.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
            ka.cmp(kb)
        });
    }

    let mut out = String::new();
    let roots = by_parent.get("").cloned().unwrap_or_default();
    for root in roots {
        walk(root, 0, &by_parent, &mut out, false);
    }
    // Orphans: parent_pattern_id points at an id outside this plan.
    let mut printed_orphan_header = false;
    for (parent_id, children) in &by_parent {
        if parent_id.is_empty() || by_id.contains_key(parent_id) {
            continue;
        }
        if !printed_orphan_header {
            out.push_str("(orphans — parent not in this plan)\n");
            printed_orphan_header = true;
        }
        for c in children {
            walk(c, 0, &by_parent, &mut out, true);
        }
    }
    if out.is_empty() {
        out.push_str("(no patterns in this plan)\n");
    }
    out
}

fn walk(
    node: &Value,
    depth: usize,
    by_parent: &std::collections::BTreeMap<String, Vec<&Value>>,
    out: &mut String,
    orphan: bool,
) {
    for _ in 0..depth {
        out.push_str("  ");
    }
    let id = node.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    let short = node
        .get("short_code")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let kind = node.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
    let scope = node
        .get("applies_to")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let lifecycle = node
        .get("lifecycle")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let depth_field = node
        .get("depth")
        .and_then(|v| v.as_i64())
        .unwrap_or_default();
    let prefix = if orphan { "(orphan) " } else { "- " };
    out.push_str(&format!(
        "{}{} {} [{}/{}] depth={} lifecycle={}\n",
        prefix, short, id, kind, scope, depth_field, lifecycle
    ));
    if let Some(children) = by_parent.get(id) {
        for c in children {
            walk(c, depth + 1, by_parent, out, false);
        }
    }
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
