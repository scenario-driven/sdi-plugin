//! `sdi daemon {start,stop,status}` — daemon lifecycle.
//!
//! `start` spawns the `sdid` binary detached (double-fork on unix; the parent
//! returns once the daemon has written its port file). `stop` reads the pid
//! file and sends SIGTERM. `status` reports running/port/pid.

use anyhow::{anyhow, Context, Result};
use sdi_db::Paths;
use std::ffi::OsString;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long to wait for the daemon's TCP listener to accept a connection
/// before declaring it dead. Loopback connects either succeed or are refused
/// almost instantly, so this only bounds the pathological "host present but
/// stack wedged" case.
const PROBE_TIMEOUT: Duration = Duration::from_millis(300);

/// Resolve the path to the `sdid` binary. Resolution order:
///   1. `SDI_DAEMON_BIN` env — the install gate resolves the daemon path and
///      hands it over (plugin/adapters/shared/sdi-hooks.cjs), so the Rust CLI
///      and the Node hook share one layout contract.
///   2. Sibling of the running `sdi` (`<dir>/sdid`) — the dev/workspace layout
///      where `cargo build` drops both binaries into `target/<profile>/`.
///   3. Distribution layout (`<dir>/../daemon/bin/sdid`) — the marketplace tree
///      ships `sdi` under `bin/` and `sdid` under `daemon/bin/`.
///   4. Bare `"sdid"`, letting the OS resolve it from PATH.
pub fn sdid_bin() -> PathBuf {
    resolve_sdid(
        std::env::var_os("SDI_DAEMON_BIN"),
        std::env::current_exe().ok(),
    )
}

/// Pure resolution policy split out from [`sdid_bin`] so the lookup order is
/// testable without touching the process environment or the real executable.
fn resolve_sdid(env_override: Option<OsString>, current_exe: Option<PathBuf>) -> PathBuf {
    if let Some(env) = env_override {
        let p = PathBuf::from(env);
        if p.exists() {
            return p;
        }
    }
    if let Some(dir) = current_exe.as_deref().and_then(Path::parent) {
        let sibling = dir.join("sdid");
        if sibling.exists() {
            return sibling;
        }
        if let Some(root) = dir.parent() {
            let dist = root.join("daemon").join("bin").join("sdid");
            if dist.exists() {
                return dist;
            }
        }
    }
    PathBuf::from("sdid")
}

/// Liveness = "does the daemon actually answer on its TCP port", NOT "does a
/// pid exist". The previous implementation trusted `kill(pid, 0)`, which is
/// also true for a zombie (`<defunct>`) process whose long-lived parent never
/// reaped it — so a dead-but-unreaped daemon read as alive and blocked restart.
/// A connect probe against the port the daemon binds (127.0.0.1:<port>, written
/// to the port file) is the single source of truth: a zombie listens on
/// nothing, so the connect is refused and we correctly treat it as dead.
pub fn is_running(paths: &Paths) -> bool {
    daemon_responds(paths)
}

/// True iff something accepts a connection on the daemon's port. `Ok(_)` means
/// a live listener (busy); any error (no port file, ECONNREFUSED, timeout)
/// means dead/stale.
fn daemon_responds(paths: &Paths) -> bool {
    let Some(port) = read_port(&paths.port_file) else {
        return false;
    };
    port_responds(port)
}

fn port_responds(port: u16) -> bool {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).is_ok()
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
extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

/// Start the daemon and wait until it writes its port file. Returns the port.
pub async fn start(paths: &Paths) -> Result<u16> {
    // `is_running` probes the daemon's TCP port, which it can only do after
    // reading the port file — so a `true` here guarantees a readable port.
    if is_running(paths) {
        if let Some(port) = read_port(&paths.port_file) {
            return Ok(port);
        }
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
        // Double-fork so the daemon reparents to init(1) and can never become a
        // zombie under a long-lived parent. `sdi mcp` (and Node MCP shells) spawn
        // the daemon in-process and never call wait(); setsid() alone does NOT
        // change the parent pid, so without the extra fork a crashed daemon would
        // linger as `<defunct>` and poison pid-based liveness checks forever.
        //
        // pre_exec runs in the post-fork child (a single-threaded copy of us):
        //   - fork() == 0  → grandchild: setsid() to lead a new session, then exec
        //                      the daemon. Its parent (the intermediate) exits
        //                      immediately, so it is reparented to init(1).
        //   - fork()  > 0  → intermediate: _exit(0) right away; the real parent
        //                      reaps it via child.wait() below (near-instant).
        unsafe {
            cmd.pre_exec(|| {
                match libc_fork() {
                    -1 => Err(std::io::Error::last_os_error()),
                    0 => {
                        libc_setsid();
                        Ok(())
                    }
                    _ => libc_exit(0),
                }
            });
        }
    }
    let child = cmd
        .spawn()
        .with_context(|| format!("spawn {}", bin.display()))?;
    // On unix `child` is the short-lived intermediate from the double-fork, not
    // the daemon — reap it (it `_exit(0)`s immediately, so this is near-instant)
    // so it does not become a zombie under us. The daemon (grandchild) reparents
    // to init(1) and writes its own pid/port file, which the readiness loop below
    // polls — that handshake is unaffected by the reap. On non-unix there is no
    // double-fork, so `child` is the daemon itself and we leave it running.
    #[cfg(unix)]
    {
        let mut child = child;
        let _ = child.wait();
    }
    #[cfg(not(unix))]
    let _ = child;

    // Wait up to ~5s for the daemon to write its port file.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(port) = read_port(&paths.port_file) {
            return Ok(port);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(anyhow!(
        "daemon did not write port file at {} within 5s — check log at {}",
        paths.port_file.display(),
        paths.log_file.display()
    ))
}

#[cfg(unix)]
extern "C" {
    #[link_name = "setsid"]
    fn libc_setsid() -> i32;
    #[link_name = "fork"]
    fn libc_fork() -> i32;
    #[link_name = "_exit"]
    fn libc_exit(status: i32) -> !;
}

/// Stop the daemon by signalling its pid with SIGTERM, then waiting until it
/// stops answering on its port (its graceful-shutdown path removes the
/// lifecycle files; we sweep any leftovers). Liveness is decided by a connect
/// probe, not pid existence — so a zombie (dead pid, listening on nothing) is
/// recognised as already-down and its stale files are swept without a spurious
/// "did not exit" error.
pub async fn stop(paths: &Paths) -> Result<()> {
    let Some(pid) = read_pid(&paths.pid_file) else {
        return Err(anyhow!("daemon not running (no pid file)"));
    };
    if !daemon_responds(paths) {
        // Not answering: crashed or zombie. Sweep stale lifecycle files.
        sweep_lifecycle_files(paths);
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
        if !daemon_responds(paths) {
            sweep_lifecycle_files(paths);
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(anyhow!("daemon (pid {}) did not exit within 5s", pid))
}

fn sweep_lifecycle_files(paths: &Paths) {
    let _ = std::fs::remove_file(&paths.pid_file);
    let _ = std::fs::remove_file(&paths.port_file);
}

pub struct Status {
    pub running: bool,
    pub pid: Option<u32>,
    pub port: Option<u16>,
}

pub fn status(paths: &Paths) -> Status {
    // `running` is whether the daemon answers on its port, not whether the pid
    // exists — a zombie pid would otherwise read as running. `pid` is reported
    // verbatim from the file for diagnostics.
    let pid = read_pid(&paths.pid_file);
    let running = daemon_responds(paths);
    let port = read_port(&paths.port_file);
    Status { running, pid, port }
}

#[cfg(test)]
mod tests {
    use super::{port_responds, resolve_sdid};
    use std::fs;
    use std::net::TcpListener;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sdi_sdid_{}_{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(p: &PathBuf) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, b"#!/bin/sh\n").unwrap();
    }

    #[test]
    fn env_override_wins_when_file_exists() {
        let dir = scratch("env");
        let target = dir.join("custom-sdid");
        touch(&target);
        let got = resolve_sdid(Some(target.clone().into_os_string()), None);
        assert_eq!(got, target);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn env_override_ignored_when_file_missing() {
        let dir = scratch("envmiss");
        let exe = dir.join("bin").join("sdi");
        let sdid = dir.join("daemon").join("bin").join("sdid");
        touch(&exe);
        touch(&sdid);
        // Env points at a nonexistent path → fall through to layout search.
        let got = resolve_sdid(Some("/no/such/sdid".into()), Some(exe));
        assert_eq!(got, sdid);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sibling_layout_resolves() {
        // Dev/workspace: sdi and sdid side by side in target/<profile>/.
        let dir = scratch("sib");
        let exe = dir.join("sdi");
        let sdid = dir.join("sdid");
        touch(&exe);
        touch(&sdid);
        let got = resolve_sdid(None, Some(exe));
        assert_eq!(got, sdid);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dist_layout_resolves_daemon_bin() {
        // Marketplace tree: <root>/bin/sdi + <root>/daemon/bin/sdid.
        let dir = scratch("dist");
        let exe = dir.join("bin").join("sdi");
        let sdid = dir.join("daemon").join("bin").join("sdid");
        touch(&exe);
        touch(&sdid);
        let got = resolve_sdid(None, Some(exe));
        assert_eq!(got, sdid);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn falls_back_to_bare_name() {
        let dir = scratch("bare");
        let exe = dir.join("bin").join("sdi");
        touch(&exe); // no sdid anywhere
        let got = resolve_sdid(None, Some(exe));
        assert_eq!(got, PathBuf::from("sdid"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn port_probe_true_when_listener_bound() {
        // A real listener on loopback answers the connect probe → "running".
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port_responds(port));
    }

    #[test]
    fn port_probe_false_when_no_listener() {
        // Bind then drop to obtain a port nobody listens on. The connect is
        // refused → "dead". This is the zombie case: a dead pid binds nothing,
        // so liveness must be false even though the pid (file) may still exist.
        let port = {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            listener.local_addr().unwrap().port()
        };
        assert!(!port_responds(port));
    }
}
