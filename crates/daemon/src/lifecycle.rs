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
