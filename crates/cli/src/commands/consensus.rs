//! `sdi consensus status` — D20 M3 4-stage view. Reads from `/decisions` and
//! buckets rows by proposal_id + kind so callers can see at a glance which
//! proposals still need critique vs. those that reached consensus or escalated.

use crate::cli::{ConsensusCmd, ConsensusStatusArgs};
use crate::http::Client;
use anyhow::Result;
use serde_json::{json, Map, Value};

pub async fn run(cli: &Client, cmd: ConsensusCmd) -> Result<()> {
    match cmd {
        ConsensusCmd::Status(args) => status(cli, args).await,
    }
}

async fn status(cli: &Client, args: ConsensusStatusArgs) -> Result<()> {
    let v: Value = cli
        .get_json(&format!("/decisions?plan_id={}", urlencode(&args.plan_id)))
        .await?;
    let rows = v
        .get("decisions")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();

    let mut groups: Map<String, Value> = Map::new();
    for row in rows.iter() {
        let kind = row
            .get("kind")
            .and_then(|k| k.as_str())
            .unwrap_or("proposal");
        let id = row.get("id").and_then(|s| s.as_str()).unwrap_or("");
        let proposal_id = if kind == "proposal" {
            id
        } else {
            row.get("proposal_id")
                .and_then(|s| s.as_str())
                .unwrap_or("")
        };
        if proposal_id.is_empty() {
            continue;
        }
        if let Some(filter) = args.proposal_id.as_deref() {
            if proposal_id != filter {
                continue;
            }
        }
        let entry = groups.entry(proposal_id.to_string()).or_insert_with(|| {
            json!({
                "proposal_id": proposal_id,
                "proposal": Value::Null,
                "critiques": Value::Array(vec![]),
                "consensus": Value::Null,
                "dissensus": Value::Null,
            })
        });
        match kind {
            "proposal" => {
                entry["proposal"] = row.clone();
            }
            "critique" => {
                if let Some(arr) = entry.get_mut("critiques").and_then(|v| v.as_array_mut()) {
                    arr.push(row.clone());
                }
            }
            "consensus" => {
                entry["consensus"] = row.clone();
            }
            "dissensus" => {
                entry["dissensus"] = row.clone();
            }
            _ => {}
        }
    }

    let groups_out: Vec<Value> = groups
        .into_iter()
        .map(|(_, mut entry)| {
            let critique_count = entry
                .get("critiques")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let stage = if entry
                .get("dissensus")
                .map(|v| !v.is_null())
                .unwrap_or(false)
            {
                "dissensus"
            } else if entry
                .get("consensus")
                .map(|v| !v.is_null())
                .unwrap_or(false)
            {
                "consensus"
            } else if critique_count > 0 {
                "critique"
            } else {
                "proposal"
            };
            entry["stage"] = Value::String(stage.to_string());
            entry["critique_count"] = json!(critique_count);
            entry
        })
        .collect();

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({ "proposals": groups_out }))?
    );
    Ok(())
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
