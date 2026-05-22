//! `sdi init | backup | restore | config | log | watch | completions`.
//!
//! These are local-to-the-machine operations; only `init` actually talks to
//! the daemon. `backup`/`restore` operate directly on the SQLite file at
//! `paths.db_file`.

use crate::cli::{App, InitArgs, LogArgs, WatchArgs};
use crate::http::Client;
use crate::output::emit;
use anyhow::{anyhow, bail, Context, Result};
use clap::CommandFactory;
use clap_complete::{generate, Shell};
use sdi_db::Paths;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::Path;

pub async fn init(cli: &Client, args: InitArgs) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let basename = cwd
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    let name = args.name.clone().unwrap_or_else(|| basename.to_string());
    let key = args
        .key
        .clone()
        .unwrap_or_else(|| slug_to_key(basename));
    // Idempotent: if a project already owns this cwd, return it.
    let by_cwd: Value = cli
        .get_json(&format!("/projects/by-cwd?cwd={}", urlencode(&cwd.display().to_string())))
        .await
        .unwrap_or(Value::Null);
    if let Some(proj) = by_cwd.get("project") {
        if !proj.is_null() {
            return emit(&json!({ "project": proj, "created": false }), false);
        }
    }
    let body = json!({
        "key": key,
        "name": name,
        "cwds": [cwd.display().to_string()],
    });
    let created: Value = cli.post_json("/projects", &body).await?;
    emit(&json!({ "project": created, "created": true }), false)
}

fn slug_to_key(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    let chunk = cleaned.chars().take(8).collect::<String>();
    if chunk.is_empty() {
        "PROJ".into()
    } else {
        chunk
    }
}

pub fn backup(paths: &Paths, output: &str) -> Result<()> {
    let src = &paths.db_file;
    if !src.exists() {
        bail!("db file does not exist at {}", src.display());
    }
    let dst = Path::new(output);
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, dst).with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
    println!("{}", json!({
        "backup": dst.display().to_string(),
        "source": src.display().to_string(),
    }));
    Ok(())
}

pub fn restore(paths: &Paths, input: &str) -> Result<()> {
    let dst = &paths.db_file;
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(input, dst)
        .with_context(|| format!("copy {} -> {}", input, dst.display()))?;
    println!("{}", json!({
        "restored": dst.display().to_string(),
        "source": input,
    }));
    Ok(())
}

pub fn config(paths: &Paths) -> Result<()> {
    let running = paths.pid_file.exists() && paths.port_file.exists();
    let port = std::fs::read_to_string(&paths.port_file)
        .ok()
        .map(|s| s.trim().to_string());
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "data": paths.data.display().to_string(),
            "cache": paths.cache.display().to_string(),
            "config": paths.config.display().to_string(),
            "state": paths.state.display().to_string(),
            "db_file": paths.db_file.display().to_string(),
            "socket_file": paths.socket_file.display().to_string(),
            "pid_file": paths.pid_file.display().to_string(),
            "port_file": paths.port_file.display().to_string(),
            "log_file": paths.log_file.display().to_string(),
            "daemon_running": running,
            "port": port,
        }))?
    );
    Ok(())
}

pub fn log(paths: &Paths, args: LogArgs) -> Result<()> {
    let f = std::fs::File::open(&paths.log_file)
        .with_context(|| format!("open log {}", paths.log_file.display()))?;
    let lines: Vec<String> = BufReader::new(f).lines().map_while(Result::ok).collect();
    let start = lines.len().saturating_sub(args.lines);
    for line in &lines[start..] {
        println!("{line}");
    }
    if args.follow {
        // Re-open and seek to end for tail-follow.
        let f = std::fs::File::open(&paths.log_file)?;
        let mut len = f.metadata()?.len();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let f = std::fs::File::open(&paths.log_file)?;
            let new_len = f.metadata()?.len();
            if new_len <= len {
                continue;
            }
            use std::io::{Read, Seek, SeekFrom};
            let mut f = f;
            f.seek(SeekFrom::Start(len))?;
            let mut buf = Vec::with_capacity((new_len - len) as usize);
            f.read_to_end(&mut buf)?;
            print!("{}", String::from_utf8_lossy(&buf));
            len = new_len;
        }
    }
    Ok(())
}

pub async fn watch(cli: &Client, args: WatchArgs) -> Result<()> {
    use futures::StreamExt;
    let url = format!("{}/events", cli.base());
    let resp = reqwest::Client::new()
        .get(&url)
        .header("Accept", "text/event-stream")
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("SSE handshake failed: HTTP {}", resp.status());
    }
    let mut stream = resp.bytes_stream();
    let mut buf = Vec::<u8>::new();
    let mut emitted = 0usize;
    while let Some(chunk) = stream.next().await {
        buf.extend_from_slice(&chunk?);
        while let Some(pos) = find_event_boundary(&buf) {
            let raw = buf[..pos].to_vec();
            buf.drain(..pos + 2);
            let txt = String::from_utf8_lossy(&raw);
            let mut data_line: Option<&str> = None;
            for line in txt.lines() {
                if let Some(rest) = line.strip_prefix("data: ") {
                    data_line = Some(rest);
                }
            }
            let Some(data) = data_line else { continue };
            let Ok(ev) = serde_json::from_str::<Value>(data) else { continue };
            if let Some(prefix) = &args.kind {
                let kind = ev.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                if !kind.starts_with(prefix) {
                    continue;
                }
            }
            println!("{}", ev);
            emitted += 1;
            if args.count > 0 && emitted >= args.count {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn find_event_boundary(buf: &[u8]) -> Option<usize> {
    // SSE framing: events are separated by `\n\n`.
    buf.windows(2).position(|w| w == b"\n\n")
}

pub fn completions(shell: &str) -> Result<()> {
    let s: Shell = match shell {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "fish" => Shell::Fish,
        "powershell" | "pwsh" => Shell::PowerShell,
        "elvish" => Shell::Elvish,
        other => return Err(anyhow!("unsupported shell: {other}")),
    };
    let mut cmd = App::command();
    let name = cmd.get_name().to_string();
    generate(s, &mut cmd, name, &mut std::io::stdout());
    Ok(())
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
