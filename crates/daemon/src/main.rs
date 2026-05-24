use anyhow::{bail, Result};
use clap::Parser;
use sdi_daemon::lifecycle;
use sdi_db::Paths;
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

    /// Port to bind. Default 19500; auto-increments (+1, up to 20 attempts)
    /// if the port is already in use. Pass 0 for an OS-assigned random port.
    #[arg(long, default_value_t = 19500)]
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
    lifecycle::write_pid(&paths.pid_file)?;

    let pool = sdi_db::open(&paths)?;
    let state = sdi_daemon::AppState::new(pool, paths.clone());
    let app = sdi_daemon::router::build(state);

    let listener = bind_with_fallback(&cli.host, cli.port).await?;
    let addr = listener.local_addr()?;
    std::fs::write(&paths.port_file, addr.port().to_string())?;
    tracing::info!(
        host = %cli.host,
        port = addr.port(),
        log_file = %paths.log_file.display(),
        db = %paths.db_file.display(),
        "sdid listening"
    );

    let paths_for_cleanup = paths.clone();
    let serve = axum::serve(listener, app).with_graceful_shutdown(async move {
        lifecycle::shutdown_signal().await;
    });

    let res = serve.await;
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

/// Bind a TCP listener on `host:port`. If `port == 0`, OS picks a random port.
/// Otherwise try `port`, `port+1`, ... for up to 20 attempts, skipping ports
/// already in use. Fails if no port in the range is free.
async fn bind_with_fallback(host: &str, port: u16) -> Result<tokio::net::TcpListener> {
    if port == 0 {
        let addr: SocketAddr = format!("{host}:0").parse()?;
        return Ok(tokio::net::TcpListener::bind(addr).await?);
    }

    const MAX_ATTEMPTS: u16 = 20;
    let mut last_err: Option<std::io::Error> = None;
    for offset in 0..MAX_ATTEMPTS {
        let candidate = port.saturating_add(offset);
        let addr: SocketAddr = format!("{host}:{candidate}").parse()?;
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                if offset > 0 {
                    tracing::warn!(
                        requested = port,
                        bound = candidate,
                        "port in use; bound to next free port"
                    );
                }
                return Ok(listener);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                last_err = Some(e);
                continue;
            }
            Err(e) => return Err(e.into()),
        }
    }
    let end = port.saturating_add(MAX_ATTEMPTS - 1);
    bail!(
        "no free port in range {port}..={end}: {}",
        last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "unknown".into())
    )
}
