//! Daemon lifecycle helpers — pid file write and port file write are handled
//! inline by [`crate::build`]. This module exposes graceful shutdown signals
//! and a tiny is-running probe used by the CLI's `daemon status`.

use sdi_db::Paths;
use std::path::Path;

/// Write the current process pid to `pid_path`. Caller is expected to wrap
/// this in a deletion-on-Drop guard if it cares about cleanup.
pub fn write_pid(pid_path: &Path) -> std::io::Result<()> {
    std::fs::write(pid_path, std::process::id().to_string())
}

/// Best-effort cleanup of the lifecycle files. Failures are swallowed because
/// we may have crashed mid-startup.
pub fn cleanup(paths: &Paths) {
    let _ = std::fs::remove_file(&paths.pid_file);
    let _ = std::fs::remove_file(&paths.port_file);
    let _ = std::fs::remove_file(&paths.socket_file);
}

/// Read the port this data dir last recorded, if any.
pub fn read_port(port_file: &Path) -> Option<u16> {
    std::fs::read_to_string(port_file)
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
}

/// True iff something is accepting TCP connections on `host:port`.
fn port_responds(host: &str, port: u16) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    addrs
        .into_iter()
        .any(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok())
}

/// Singleton guard (#19): is a daemon already serving THIS data dir? A second
/// `sdid` exec onto the single global DB is the only way N daemons end up
/// racing one sqlite file (the second overwrites the pid/port files and binds
/// an incremented port). The authoritative signal is "the port we last wrote to
/// `port_file` still accepts a connection". It reads this paths' own port_file,
/// so SDI_HOME-isolated instances never false-positive against each other.
pub fn daemon_already_running(paths: &Paths, host: &str) -> bool {
    match read_port(&paths.port_file) {
        Some(port) => port_responds(host, port),
        None => false,
    }
}

/// Block until SIGINT/SIGTERM fires.
#[cfg(unix)]
pub async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    tokio::select! {
        _ = sigterm.recv() => tracing::info!("SIGTERM received"),
        _ = sigint.recv()  => tracing::info!("SIGINT received"),
    }
}

#[cfg(not(unix))]
pub async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("ctrl_c received");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn paths_with_port_file(dir: &std::path::Path, body: &str) -> Paths {
        let port_file = dir.join("sdid.port");
        std::fs::write(&port_file, body).unwrap();
        mk_paths(dir, port_file)
    }

    fn mk_paths(dir: &std::path::Path, port_file: std::path::PathBuf) -> Paths {
        Paths {
            data: dir.to_path_buf(),
            cache: dir.to_path_buf(),
            config: dir.to_path_buf(),
            state: dir.to_path_buf(),
            db_file: dir.join("sdi.db"),
            pid_file: dir.join("sdid.pid"),
            port_file,
            socket_file: dir.join("sdid.sock"),
            log_file: dir.join("sdid.log"),
        }
    }

    #[test]
    fn already_running_true_when_recorded_port_accepts() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let dir = std::env::temp_dir().join(format!("sdid-life-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = paths_with_port_file(&dir, &port.to_string());
        assert!(daemon_already_running(&paths, "127.0.0.1"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn already_running_false_when_no_port_file_or_dead() {
        let dir = std::env::temp_dir().join(format!("sdid-life-dead-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // No port file → not running.
        let paths_none = mk_paths(&dir, dir.join("sdid.port"));
        assert!(!daemon_already_running(&paths_none, "127.0.0.1"));
        // Port file pointing at a free (unbound) port → stale → not running.
        let free = TcpListener::bind("127.0.0.1:0").unwrap();
        let free_port = free.local_addr().unwrap().port();
        drop(free); // release it so nothing is listening
        let paths_stale = paths_with_port_file(&dir, &free_port.to_string());
        assert!(!daemon_already_running(&paths_stale, "127.0.0.1"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
