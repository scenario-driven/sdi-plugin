//! `sdi daemon {start,stop,status}` — daemon lifecycle.
//!
//! `start` spawns the `sdid` binary detached (double-fork on unix; the parent
//! returns once the daemon has written its port file). `stop` reads the pid
//! file and sends SIGTERM. `status` reports running/port/pid.

use anyhow::{anyhow, Context, Result};
use sdi_db::Paths;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Resolve the path to the `sdid` binary. We prefer a sibling of the running
/// `sdi` binary (release/dev install layout), fall back to PATH, finally fall
/// back to "sdid" and let the OS resolve it.
pub fn sdid_bin() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("sdid");
            if sibling.exists() {
                return sibling;
            }
        }
    }
    PathBuf::from("sdid")
}

pub fn is_running(paths: &Paths) -> bool {
    let Some(pid) = read_pid(&paths.pid_file) else {
        return false;
    };
    pid_alive(pid)
}

pub fn read_pid(pid_file: &Path) -> Option<u32> {
    std::fs::read_to_string(pid_file)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

pub fn read_port(port_file: &Path) -> Option<u16> {
    std::fs::read_to_string(port_file)
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    // kill(pid, 0) returns 0 if the process exists and we have permission.
    unsafe { libc_kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
fn pid_alive(_pid: u32) -> bool {
    // Best-effort on non-unix; rely on the port file as a liveness hint.
    true
}

#[cfg(unix)]
extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

/// Start the daemon and wait until it writes its port file. Returns the port.
pub async fn start(paths: &Paths) -> Result<u16> {
    if is_running(paths) {
        if let Some(port) = read_port(&paths.port_file) {
            return Ok(port);
        }
        return Err(anyhow!(
            "daemon pid file exists at {} but no port file — stale state? try `sdi daemon stop`",
            paths.pid_file.display()
        ));
    }
    paths.ensure_dirs().ok();
    // Stale lifecycle files from a previous crashed run would confuse our
    // "wait for port file" loop; clear them before spawning.
    let _ = std::fs::remove_file(&paths.port_file);
    let _ = std::fs::remove_file(&paths.pid_file);

    let bin = sdid_bin();
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log_file)
        .with_context(|| format!("open log file {}", paths.log_file.display()))?;
    let err_log = log.try_clone()?;

    let mut cmd = Command::new(&bin);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err_log));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Detach into its own session so closing the parent terminal doesn't SIGHUP it.
        unsafe {
            cmd.pre_exec(|| {
                libc_setsid();
                Ok(())
            });
        }
    }
    let child = cmd
        .spawn()
        .with_context(|| format!("spawn {}", bin.display()))?;
    let pid = child.id();

    // Wait up to ~5s for the port file to appear.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(port) = read_port(&paths.port_file) {
            return Ok(port);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(anyhow!(
        "daemon (pid {}) did not write port file at {} within 5s — check log at {}",
        pid,
        paths.port_file.display(),
        paths.log_file.display()
    ))
}

#[cfg(unix)]
extern "C" {
    #[link_name = "setsid"]
    fn libc_setsid() -> i32;
}

/// Stop the daemon by signalling its pid with SIGTERM, then waiting for the
/// pid file to disappear (or fall back to deleting it after a timeout).
pub async fn stop(paths: &Paths) -> Result<()> {
    let Some(pid) = read_pid(&paths.pid_file) else {
        return Err(anyhow!("daemon not running (no pid file)"));
    };
    if !pid_alive(pid) {
        // Clean stale lifecycle files.
        let _ = std::fs::remove_file(&paths.pid_file);
        let _ = std::fs::remove_file(&paths.port_file);
        return Ok(());
    }
    #[cfg(unix)]
    unsafe {
        if libc_kill(pid as i32, 15) != 0 {
            return Err(anyhow!("kill({}, SIGTERM) failed", pid));
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if !pid_alive(pid) {
            let _ = std::fs::remove_file(&paths.pid_file);
            let _ = std::fs::remove_file(&paths.port_file);
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(anyhow!("daemon (pid {}) did not exit within 5s", pid))
}

pub struct Status {
    pub running: bool,
    pub pid: Option<u32>,
    pub port: Option<u16>,
}

pub fn status(paths: &Paths) -> Status {
    let pid = read_pid(&paths.pid_file);
    let running = pid.map(pid_alive).unwrap_or(false);
    let port = read_port(&paths.port_file);
    Status { running, pid, port }
}
