//! `sdi run …` — execution attempts (start/finish/list) and scenario
//! verification runs (`verify` / `status` / `budget` / `release-budget`).
//!
//! `verify` is deliberately thin: the daemon owns every judgement (input
//! fingerprint, report digest, failure classification, signature, retry
//! budget). The CLI only executes the command, tees its output into the
//! daemon-assigned report file, and reports the raw exit status.

use crate::cli::{
    RunBudgetArgs, RunCmd, RunFinishArgs, RunListArgs, RunReleaseBudgetArgs, RunStartArgs,
    RunStatusArgs, RunVerifyArgs,
};
use crate::http::Client;
use crate::output::emit;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

pub async fn run(cli: &Client, cmd: RunCmd, quiet: bool) -> Result<()> {
    match cmd {
        RunCmd::Verify(args) => verify(cli, args, quiet).await,
        RunCmd::Status(args) => status(cli, args).await,
        RunCmd::Budget(args) => budget(cli, args).await,
        RunCmd::ReleaseBudget(args) => release_budget(cli, args, quiet).await,
        RunCmd::Start(args) => start(cli, args, quiet).await,
        RunCmd::Finish(args) => finish(cli, args, quiet).await,
        RunCmd::View { id } => {
            let v: Value = cli.get_json(&format!("/runs/{id}")).await?;
            emit(&v, quiet)
        }
        RunCmd::List(args) => list(cli, args).await,
    }
}

fn resolve_cwd(cwd: Option<&str>) -> Result<PathBuf> {
    let raw = match cwd {
        Some(c) => PathBuf::from(c),
        None => std::env::current_dir().context("read current directory")?,
    };
    raw.canonicalize()
        .with_context(|| format!("resolve cwd {}", raw.display()))
}

fn tool_versions(specs: &[String]) -> Result<Map<String, Value>> {
    let parsed = sdi_core::verification::parse_tool_versions(specs).map_err(|e| anyhow!("{e}"))?;
    Ok(parsed
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect())
}

async fn pump<R: AsyncRead + Unpin>(
    mut src: R,
    report: Arc<Mutex<tokio::fs::File>>,
) -> std::io::Result<()> {
    let mut buf = vec![0u8; 16 * 1024];
    let mut echo = tokio::io::stderr();
    loop {
        let n = src.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        report.lock().await.write_all(&buf[..n]).await?;
        echo.write_all(&buf[..n]).await?;
    }
}

#[derive(Default)]
struct Outcome {
    exit_code: Option<i32>,
    signal: Option<i32>,
    timed_out: bool,
    spawn_error: Option<&'static str>,
    aborted: bool,
}

#[cfg(unix)]
fn signal_of(status: &std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn signal_of(_status: &std::process::ExitStatus) -> Option<i32> {
    None
}

async fn execute(
    command: &[String],
    cwd: &std::path::Path,
    report_path: Option<&str>,
    timeout: Option<u64>,
) -> Result<Outcome> {
    let report_file = match report_path {
        Some(p) => tokio::fs::File::create(p)
            .await
            .with_context(|| format!("create report {p}"))?,
        None => return Err(anyhow!("daemon did not assign a report path")),
    };
    let report = Arc::new(Mutex::new(report_file));

    let mut child = match tokio::process::Command::new(&command[0])
        .args(&command[1..])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let kind = match e.kind() {
                std::io::ErrorKind::NotFound => "not-found",
                std::io::ErrorKind::PermissionDenied => "permission-denied",
                _ => "other",
            };
            let msg = format!("sdi run verify: cannot spawn `{}`: {e}\n", command[0]);
            eprint!("{msg}");
            report.lock().await.write_all(msg.as_bytes()).await.ok();
            return Ok(Outcome {
                spawn_error: Some(kind),
                ..Default::default()
            });
        }
    };
    let out = child
        .stdout
        .take()
        .map(|s| tokio::spawn(pump(s, report.clone())));
    let err = child
        .stderr
        .take()
        .map(|s| tokio::spawn(pump(s, report.clone())));

    let deadline = async {
        match timeout {
            Some(secs) => tokio::time::sleep(Duration::from_secs(secs)).await,
            None => std::future::pending::<()>().await,
        }
    };
    let mut outcome = Outcome::default();
    tokio::select! {
        status = child.wait() => {
            let status = status.context("wait for command")?;
            outcome.exit_code = status.code();
            outcome.signal = signal_of(&status);
        }
        _ = deadline => {
            outcome.timed_out = true;
            child.kill().await.ok();
            let status = child.wait().await.ok();
            outcome.signal = status.as_ref().and_then(signal_of);
            outcome.exit_code = status.and_then(|s| s.code());
        }
        _ = tokio::signal::ctrl_c() => {
            outcome.aborted = true;
            child.kill().await.ok();
            let _ = child.wait().await;
        }
    }
    // A grandchild may keep the pipes open after the child exits; do not let
    // it hold the report hostage.
    for h in [out, err].into_iter().flatten() {
        let _ = tokio::time::timeout(Duration::from_secs(5), h).await;
    }
    report.lock().await.flush().await.ok();
    Ok(outcome)
}

async fn verify(cli: &Client, args: RunVerifyArgs, quiet: bool) -> Result<()> {
    if args.command.is_empty() {
        return Err(anyhow!("a command is required after `--`"));
    }
    let cwd = resolve_cwd(args.cwd.as_deref())?;
    let mut start = json!({
        "scenario": args.scenario,
        "command": args.command,
        "cwd": cwd.to_string_lossy(),
        "input_globs": args.inputs,
        "tool_versions": tool_versions(&args.tool_versions)?,
        "actor": args.actor,
    });
    if let Some(p) = args.plan.as_deref() {
        start["plan_id"] = json!(p);
    }
    if let Some(t) = args.task.as_deref() {
        start["task_id"] = json!(t);
    }
    if let Some(s) = args.session_id.as_deref() {
        start["session_id"] = json!(s);
    }
    let run: Value = cli.post_json("/runs", &start).await?;
    let run_id = run["id"]
        .as_str()
        .ok_or_else(|| anyhow!("daemon returned no run id: {run}"))?
        .to_string();
    if let Some(ex) = run["exhausted_retry_budgets"]
        .as_array()
        .filter(|a| !a.is_empty())
    {
        eprintln!(
            "sdi run verify: warning — {} retry budget(s) for this scenario are exhausted; \
             a failure here cannot be retried under a new task. Escalate with a decision \
             (`sdi run budget --scenario {}`).",
            ex.len(),
            args.scenario
        );
    }
    eprintln!(
        "sdi run verify: run {run_id} fingerprint {}",
        run["input_fingerprint"].as_str().unwrap_or("?")
    );

    let began = Instant::now();
    let outcome = execute(
        &args.command,
        &cwd,
        run["report_path"].as_str(),
        args.timeout,
    )
    .await?;
    let mut finish = json!({
        "timed_out": outcome.timed_out,
        "duration_ms": began.elapsed().as_millis() as i64,
    });
    if let Some(c) = outcome.exit_code {
        finish["exit_code"] = json!(c);
    }
    if let Some(s) = outcome.signal {
        finish["signal"] = json!(s);
    }
    if let Some(e) = outcome.spawn_error {
        finish["spawn_error"] = json!(e);
    }
    if outcome.aborted {
        finish["result"] = json!("aborted");
        finish["notes"] = json!("interrupted by the operator");
    }
    if let Some(c) = args.failure_class.as_deref() {
        finish["failure_class"] = json!(c);
    }
    if let Some(s) = args.signature.as_deref() {
        finish["failure_signature"] = json!(s);
    }
    let done: Value = cli
        .post_json(&format!("/runs/{run_id}/finish"), &finish)
        .await?;
    emit(&done, quiet)?;

    let passed = done["result"].as_str() == Some("success");
    if !passed {
        use std::io::Write;
        std::io::stdout().flush().ok();
        let code = outcome.exit_code.filter(|c| *c != 0).unwrap_or(1);
        std::process::exit(code);
    }
    Ok(())
}

async fn status(cli: &Client, args: RunStatusArgs) -> Result<()> {
    let cwd = resolve_cwd(args.cwd.as_deref())?;
    let mut body = json!({
        "scenario": args.scenario,
        "cwd": cwd.to_string_lossy(),
        "input_globs": args.inputs,
        "tool_versions": tool_versions(&args.tool_versions)?,
    });
    if !args.command.is_empty() {
        body["command"] = json!(args.command);
    }
    if let Some(p) = args.plan {
        body["plan_id"] = json!(p);
    }
    let v: Value = cli.post_json("/runs/reuse-check", &body).await?;
    emit(&v, false)
}

fn encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

async fn budget(cli: &Client, args: RunBudgetArgs) -> Result<()> {
    let cwd = resolve_cwd(None)?;
    let mut qs = vec![format!("cwd={}", encode(&cwd.to_string_lossy()))];
    if let Some(s) = args.scenario.as_deref() {
        qs.push(format!("scenario={}", encode(s)));
    }
    if let Some(p) = args.plan.as_deref() {
        qs.push(format!("plan_id={}", encode(p)));
    }
    let v: Value = cli
        .get_json(&format!("/retry-budgets?{}", qs.join("&")))
        .await?;
    emit(&v, false)
}

async fn release_budget(cli: &Client, args: RunReleaseBudgetArgs, quiet: bool) -> Result<()> {
    let cwd = resolve_cwd(None)?;
    let mut body = json!({
        "scenario": args.scenario,
        "decision_id": args.decision,
        "cwd": cwd.to_string_lossy(),
    });
    if let Some(s) = args.signature {
        body["signature"] = json!(s);
    }
    if let Some(p) = args.plan {
        body["plan_id"] = json!(p);
    }
    let v: Value = cli.post_json("/retry-budgets/release", &body).await?;
    emit(&v, quiet)
}

async fn start(cli: &Client, args: RunStartArgs, quiet: bool) -> Result<()> {
    if args.task_id.is_none() && args.scenario.is_none() {
        return Err(anyhow!("a task id or --scenario is required"));
    }
    let mut body = json!({ "actor": args.actor });
    if let Some(t) = args.task_id {
        body["task_id"] = Value::String(t);
    }
    if let Some(s) = args.scenario {
        body["scenario"] = Value::String(s);
        body["cwd"] = json!(resolve_cwd(None)?.to_string_lossy());
    }
    if let Some(s) = args.session_id {
        body["session_id"] = Value::String(s);
    }
    let v: Value = cli.post_json("/runs", &body).await?;
    emit(&v, quiet)
}

async fn finish(cli: &Client, args: RunFinishArgs, quiet: bool) -> Result<()> {
    if args.result.is_none() && args.exit_code.is_none() && args.signal.is_none() {
        return Err(anyhow!("--result or --exit-code/--signal is required"));
    }
    let mut body = json!({});
    if let Some(r) = args.result {
        body["result"] = Value::String(r);
    }
    if let Some(c) = args.exit_code {
        body["exit_code"] = json!(c);
    }
    if let Some(s) = args.signal {
        body["signal"] = json!(s);
    }
    if let Some(c) = args.failure_class {
        body["failure_class"] = Value::String(c);
    }
    if let Some(s) = args.signature {
        body["failure_signature"] = Value::String(s);
    }
    if let Some(n) = args.notes {
        body["notes"] = Value::String(n);
    }
    let v: Value = cli
        .post_json(&format!("/runs/{}/finish", args.id), &body)
        .await?;
    emit(&v, quiet)
}

async fn list(cli: &Client, args: RunListArgs) -> Result<()> {
    let path = if let Some(t) = args.task_id {
        format!("/runs?task_id={t}")
    } else if let Some(s) = args.session_id {
        format!("/runs?session_id={s}")
    } else if let Some(s) = args.scenario_id {
        format!("/runs?scenario_id={s}")
    } else {
        return Err(anyhow!("--task-id, --session-id or --scenario-id required"));
    };
    let v: Value = cli.get_json(&path).await?;
    emit(&v, false)
}
