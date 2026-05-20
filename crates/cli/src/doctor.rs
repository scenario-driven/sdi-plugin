//! `sdi doctor` — environment diagnostics.
//!
//! Checks (PRD §5.3 + LM-8):
//! - XDG paths resolve cleanly
//! - LM-8 invariant: no user-data path resolves under `~/.claude/plugins/`
//! - Database file open + schema migrations apply
//! - Daemon running / port reachable (best-effort)

use crate::daemon_cmd;
use anyhow::Result;
use sdi_db::Paths;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Report {
    pub paths: PathReport,
    pub db: CheckResult,
    pub daemon: DaemonReport,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PathReport {
    pub data: String,
    pub cache: String,
    pub config: String,
    pub state: String,
    pub db_file: String,
    pub lm8: CheckResult,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DaemonReport {
    pub running: bool,
    pub pid: Option<u32>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CheckResult {
    pub ok: bool,
    pub detail: Option<String>,
}

impl CheckResult {
    fn ok() -> Self {
        Self { ok: true, detail: None }
    }
    fn fail(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            detail: Some(msg.into()),
        }
    }
}

pub fn run() -> Result<Report> {
    let paths = match Paths::resolve() {
        Ok(p) => p,
        Err(e) => {
            // Path resolution failed (LM-8 violation, no home dir, etc.).
            // We still want to emit a report so the user sees the structured failure.
            return Ok(Report {
                paths: PathReport {
                    data: String::new(),
                    cache: String::new(),
                    config: String::new(),
                    state: String::new(),
                    db_file: String::new(),
                    lm8: CheckResult::fail(e.to_string()),
                },
                db: CheckResult::fail("skipped — paths failed"),
                daemon: DaemonReport {
                    running: false,
                    pid: None,
                    port: None,
                },
            });
        }
    };
    let lm8 = paths
        .ensure_no_plugin_overlap()
        .map(|_| CheckResult::ok())
        .unwrap_or_else(|e| CheckResult::fail(e.to_string()));
    let db = (|| -> Result<()> {
        paths
            .ensure_dirs()
            .map_err(|e| anyhow::anyhow!("ensure_dirs: {e}"))?;
        let _pool = sdi_db::open(&paths)?;
        Ok(())
    })()
    .map(|_| CheckResult::ok())
    .unwrap_or_else(|e| CheckResult::fail(e.to_string()));

    let st = daemon_cmd::status(&paths);
    Ok(Report {
        paths: PathReport {
            data: paths.data.display().to_string(),
            cache: paths.cache.display().to_string(),
            config: paths.config.display().to_string(),
            state: paths.state.display().to_string(),
            db_file: paths.db_file.display().to_string(),
            lm8,
        },
        db,
        daemon: DaemonReport {
            running: st.running,
            pid: st.pid,
            port: st.port,
        },
    })
}

/// Returns 0 on all-clear, 1 if any check failed.
pub fn exit_code(report: &Report) -> i32 {
    let lm8_ok = report.paths.lm8.ok;
    if !(lm8_ok && report.db.ok) {
        return 1;
    }
    0
}
