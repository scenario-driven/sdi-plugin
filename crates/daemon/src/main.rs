use anyhow::{bail, Result};
use clap::Parser;
use sdi_daemon::lifecycle;
use sdi_db::Paths;
use std::future::IntoFuture;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "sdid", about = "SDI background daemon (HTTP + SSE).")]
struct Cli {
    /// Log filter (e.g. "info", "sdi_daemon=debug").
    #[arg(long, default_value = "info", env = "SDI_LOG")]
    log: String,

    /// Host to bind. Must be a loopback address — non-loopback binds are
    /// refused (PUBLIC_BIND_NOT_ALLOWED). Default `127.0.0.1`.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port to bind. Default is the canonical sdi port; the daemon is a
    /// singleton per data dir, so it binds exactly this port and a second
    /// instance reuses the holder. Pass 0 for an OS-assigned random port
    /// (SDI_HOME-isolated test instances).
    #[arg(long, default_value_t = sdi_db::paths::DEFAULT_PORT)]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_new(&cli.log).unwrap_or_else(|_| EnvFilter::new("info")))
        .with_target(false)
        .init();

    if !is_loopback_host(&cli.host) {
        bail!(
            "PUBLIC_BIND_NOT_ALLOWED: host {:?} is not a loopback address; sdid only serves \
             local clients. Use 127.0.0.1, ::1, or localhost.",
            cli.host
        );
    }

    let paths = Arc::new(Paths::resolve()?);
    paths.ensure_dirs()?;

    // Singleton guard (#19): the sdi daemon is ONE instance per data dir — every
    // Claude profile/session sharing this $HOME talks to the same daemon and the
    // same SQLite file. The guarantee is an exclusive flock on `<cache>/sdid.lock`
    // held for this process's lifetime: if another daemon already owns this data
    // dir we cannot acquire it, so we stand down instead of racing the one DB.
    // The lock is port-independent and lives in the cache dir, so SDI_HOME-isolated
    // instances (a different cache dir) get their own lock and run concurrently.
    let _singleton = match lifecycle::acquire_singleton_lock(&paths.lock_file) {
        Some(guard) => guard,
        None => {
            tracing::info!(
                port = ?lifecycle::read_port(&paths.port_file),
                "sdid already running for this data dir; not starting a second instance (#19)"
            );
            return Ok(());
        }
    };

    // We hold the singleton lock, so we are the one daemon for this data dir.
    // SDI_HOME-isolated instances serve a *different* data dir and must not grab
    // the shared canonical port, so give them an OS-assigned one; the default
    // ($HOME) instance keeps the canonical port for a stable dashboard URL.
    // Either way the actual bound port is written to the (now single-writer,
    // authoritative) port file for clients to read.
    let bind_port = if cli.port == sdi_db::paths::DEFAULT_PORT
        && std::env::var(sdi_db::ENV_HOME_OVERRIDE).is_ok()
    {
        0
    } else {
        cli.port
    };
    let listener = try_bind(&cli.host, bind_port).await.map_err(|e| {
        anyhow::anyhow!(
            "failed to bind {}:{} for sdid: {e} (is another process using the port? \
             start sdid with --port 0 for an OS-assigned port)",
            cli.host,
            bind_port
        )
    })?;

    lifecycle::write_pid(&paths.pid_file)?;
    let addr = listener.local_addr()?;
    std::fs::write(&paths.port_file, addr.port().to_string())?;

    let pool = sdi_db::open(&paths)?;
    let state = sdi_daemon::AppState::new(pool, paths.clone());
    let app = sdi_daemon::router::build(state);
    tracing::info!(
        host = %cli.host,
        port = addr.port(),
        log_file = %paths.log_file.display(),
        db = %paths.db_file.display(),
        "sdid listening"
    );

    let paths_for_cleanup = paths.clone();
    // Bound the graceful shutdown. axum's graceful shutdown waits for EVERY
    // in-flight connection to finish — but the `/events` SSE stream is a
    // long-lived connection (an open dashboard tab) that never completes on
    // its own, so an unbounded wait hangs the process forever: it closes the
    // listener (the port stops responding, so `sdi daemon stop` reports
    // success) yet never exits, leaving an orphan still holding the DB. After
    // the signal fires we give in-flight work a short grace window, then force
    // exit so SSE clients cannot wedge shutdown.
    let (signalled_tx, signalled_rx) = tokio::sync::oneshot::channel::<()>();
    let serve = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            lifecycle::shutdown_signal().await;
            let _ = signalled_tx.send(());
        })
        .into_future();
    tokio::pin!(serve);

    let res = tokio::select! {
        r = &mut serve => r,
        _ = async {
            // Only starts counting once the shutdown signal has fired.
            let _ = signalled_rx.await;
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        } => {
            tracing::warn!(
                "graceful shutdown exceeded 3s (likely an open SSE stream); forcing exit"
            );
            lifecycle::cleanup(&paths_for_cleanup);
            std::process::exit(0);
        }
    };
    lifecycle::cleanup(&paths_for_cleanup);
    res?;
    Ok(())
}

fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return ip.is_loopback();
    }
    false
}

/// Bind a TCP listener on exactly `host:port`. `port == 0` lets the OS pick a
/// random port (SDI_HOME-isolated test instances). There is deliberately no
/// port-increment fallback: the canonical port is the per-data-dir singleton
/// token, so when it is taken the caller must reuse the holder (if it is a
/// sibling sdid) rather than move aside onto a second port. Returns the raw
/// `std::io::Error` so the caller can branch on `AddrInUse`.
async fn try_bind(host: &str, port: u16) -> std::io::Result<tokio::net::TcpListener> {
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    tokio::net::TcpListener::bind(addr).await
}
