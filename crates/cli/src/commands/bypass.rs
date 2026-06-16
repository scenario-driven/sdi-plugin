//! `sdi bypass arm | disarm | status` — emergency hook bypass marker.
//!
//! Why this lives in the CLI (and not in the hook layer):
//!
//! D21 forbids the orchestrator (main session) from running mutating Bash. A
//! plain `touch ~/.cache/sdi/bypass-once` is mutating, so the only on-disk
//! surface that opens the gate is itself behind the gate — a self-deadlock.
//! `sdi` is on the D21 read-only Bash whitelist (sdi-hooks.cjs: `isReadOnlyBash`),
//! so a dedicated subcommand is the one substrate the main session can reach
//! without delegation.
//!
//! Why a single marker file unlocks every mutating gate (D21 delegation,
//! active-task, D29 claim overlap):
//!
//! The marker's semantics are "the user has consciously armed an emergency
//! bypass". Splitting that intent across separate env vars per gate
//! re-introduces the bootstrap deadlock (the user disarms one gate and the
//! next one blocks them anyway). The hook reads the marker once per
//! invocation and applies it uniformly. The env surfaces
//! (`SDI_DELEGATION_BYPASS=1`, `SDI_BYPASS_HOOKS=1`, `SDI_HOOK_V05_DISABLE=1`)
//! remain as startup-time switches for users who export them from shell rc.
//!
//! #14 — KNOWN LIMITATION: the marker is a single machine-global file, so a
//! marker armed here can be consumed by a concurrently-waking PreToolUse of a
//! *different* agent on the same machine. Per-(session, agent) scoping is not
//! implementable: this CLI runs in the agent's Bash shell, which cannot read
//! its own `session_id` / `agent_id` — Claude Code exposes those ONLY in the
//! hook payload JSON, never as env vars (official docs). So the arming side
//! has no key to scope by. The platform constraint is documented rather than
//! worked around; the routine driver of bypass (the old env-based active-task
//! gate, #9) is gone, so concurrent races are rare. For a windowed emergency
//! during concurrent runs, prefer the startup-time `SDI_HOOK_V05_DISABLE`.

use crate::cli::{BypassArmArgs, BypassCmd};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration, Utc};
use sdi_db::Paths;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

pub async fn run(sub: BypassCmd) -> Result<()> {
    let marker = marker_path()?;
    match sub {
        BypassCmd::Arm(args) => arm(&marker, args),
        BypassCmd::Disarm => disarm(&marker),
        BypassCmd::Status => status(&marker),
    }
}

/// Where the hook reads the marker. Must stay byte-identical with
/// `bypassOnceFile()` in `plugin/adapters/shared/sdi-hooks.cjs`.
fn marker_path() -> Result<PathBuf> {
    let paths = Paths::resolve()?;
    Ok(paths.cache.join("bypass-once"))
}

fn arm(marker: &PathBuf, args: BypassArmArgs) -> Result<()> {
    if args.ttl <= 0 {
        return Err(anyhow!("--ttl must be a positive number of seconds"));
    }
    if let Some(parent) = marker.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create cache dir {}", parent.display()))?;
    }
    let armed_at: DateTime<Utc> = Utc::now();
    let expires_at = armed_at + Duration::seconds(args.ttl);
    let body = json!({
        "reason": args.reason,
        "armed_at": armed_at.to_rfc3339(),
        "expires_at": expires_at.to_rfc3339(),
        "ttl_seconds": args.ttl,
    });
    fs::write(marker, serde_json::to_string_pretty(&body)? + "\n")
        .with_context(|| format!("write marker {}", marker.display()))?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "armed": true,
            "marker": marker.display().to_string(),
            "reason": args.reason,
            "armed_at": armed_at.to_rfc3339(),
            "expires_at": expires_at.to_rfc3339(),
            "ttl_seconds": args.ttl,
        }))?
    );
    Ok(())
}

fn disarm(marker: &PathBuf) -> Result<()> {
    let existed = marker.exists();
    if existed {
        fs::remove_file(marker).with_context(|| format!("remove marker {}", marker.display()))?;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "disarmed": existed,
            "marker": marker.display().to_string(),
        }))?
    );
    Ok(())
}

fn status(marker: &PathBuf) -> Result<()> {
    if !marker.exists() {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "state": "absent",
                "marker": marker.display().to_string(),
            }))?
        );
        return Ok(());
    }
    let raw =
        fs::read_to_string(marker).with_context(|| format!("read marker {}", marker.display()))?;
    let parsed: Option<Value> = serde_json::from_str(&raw).ok();
    let now = Utc::now();
    if let Some(v) = parsed {
        let expires_at = v.get("expires_at").and_then(|x| x.as_str());
        let reason = v.get("reason").cloned().unwrap_or(Value::Null);
        let armed_at = v.get("armed_at").cloned().unwrap_or(Value::Null);
        let (state, ttl_remaining) =
            match expires_at.and_then(|s| DateTime::parse_from_rfc3339(s).ok()) {
                Some(exp) => {
                    let exp_utc = exp.with_timezone(&Utc);
                    if exp_utc <= now {
                        ("expired", 0i64)
                    } else {
                        ("armed", (exp_utc - now).num_seconds())
                    }
                }
                // Marker JSON without a parseable expires_at — treat as armed-forever.
                None => ("armed", -1),
            };
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "state": state,
                "marker": marker.display().to_string(),
                "reason": reason,
                "armed_at": armed_at,
                "expires_at": expires_at,
                "ttl_remaining_seconds": ttl_remaining,
            }))?
        );
    } else {
        // Plain-text body (the v0.1.4 `touch` shape). Treat as armed forever
        // for backward compatibility, surface the body as reason.
        let reason = raw.trim();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "state": "armed",
                "marker": marker.display().to_string(),
                "reason": if reason.is_empty() { Value::Null } else { Value::String(reason.into()) },
                "armed_at": Value::Null,
                "expires_at": Value::Null,
                "ttl_remaining_seconds": -1,
                "format": "plain-text",
            }))?
        );
    }
    Ok(())
}
