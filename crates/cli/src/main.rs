//! `sdi` — top-level CLI binary. Dispatches into per-family modules.

use anyhow::Result;
use clap::Parser;
use sdi_cli::cli::{App, Cmd, DaemonCmd};
use sdi_cli::commands;
use sdi_cli::http::Client;
use sdi_cli::{daemon_cmd, doctor};
use sdi_db::Paths;

#[tokio::main]
async fn main() -> Result<()> {
    let app = App::parse();
    match app.cmd {
        Cmd::Daemon(sub) => handle_daemon(sub).await,
        Cmd::Doctor => handle_doctor().await,
        Cmd::Mcp => handle_mcp().await,
        Cmd::Project(sub) => commands::project::run(&entity_client().await?, sub, app.quiet).await,
        Cmd::Plan(sub) => commands::plan::run(&entity_client().await?, sub, app.quiet).await,
        Cmd::Req(sub) => commands::requirement::run(&entity_client().await?, sub, app.quiet).await,
        Cmd::Scenario(sub) => {
            commands::scenario::run(&entity_client().await?, sub, app.quiet).await
        }
        Cmd::Round(sub) => commands::round::run(&entity_client().await?, sub, app.quiet).await,
        Cmd::Task(sub) => commands::task::run(&entity_client().await?, sub, app.quiet).await,
        Cmd::Decision(sub) => {
            commands::decision::run(&entity_client().await?, sub, app.quiet).await
        }
        Cmd::Knowledge(sub) => {
            commands::knowledge::run(&entity_client().await?, sub, app.quiet).await
        }
        Cmd::Autonomy(sub) => {
            commands::autonomy::run(&entity_client().await?, sub, app.quiet).await
        }
        Cmd::AgentNote(sub) => {
            commands::agent_note::run(&entity_client().await?, sub, app.quiet).await
        }
        Cmd::Consensus(sub) => commands::consensus::run(&entity_client().await?, sub).await,
        Cmd::Pattern(sub) => commands::pattern::run(&entity_client().await?, sub, app.quiet).await,
        Cmd::Comment(sub) => commands::comment::run(&entity_client().await?, sub, app.quiet).await,
        Cmd::Question(sub) => {
            commands::question::run(&entity_client().await?, sub, app.quiet).await
        }
        Cmd::Run(sub) => commands::run::run(&entity_client().await?, sub, app.quiet).await,
        Cmd::Usage(sub) => commands::usage::run(&entity_client().await?, sub, app.quiet).await,
        Cmd::Dashboard(args) => commands::aggregate::dashboard(&entity_client().await?, args).await,
        Cmd::Handoff(args) => commands::aggregate::handoff(&entity_client().await?, args).await,
        Cmd::Next(args) => commands::aggregate::next(&entity_client().await?, args).await,
        Cmd::Timeline(args) => commands::aggregate::timeline(&entity_client().await?, args).await,
        Cmd::Board(args) => commands::aggregate::board(&entity_client().await?, args).await,
        Cmd::Wiki(args) => commands::aggregate::wiki(&entity_client().await?, args).await,
        Cmd::Summary(args) => commands::aggregate::summary(&entity_client().await?, args).await,
        Cmd::Metrics => commands::aggregate::metrics(&entity_client().await?).await,
        Cmd::Replay(args) => commands::aggregate::replay(&entity_client().await?, args).await,
        Cmd::Export(sub) => commands::impexp::export(&entity_client().await?, sub).await,
        Cmd::Import(sub) => commands::impexp::import(&entity_client().await?, sub).await,
        Cmd::Init(args) => commands::ops::init(&entity_client().await?, args).await,
        Cmd::Backup(args) => commands::ops::backup(&Paths::resolve()?, &args.output),
        Cmd::Restore(args) => commands::ops::restore(&Paths::resolve()?, &args.input),
        Cmd::Config => commands::ops::config(&Paths::resolve()?),
        Cmd::Log(args) => commands::ops::log(&Paths::resolve()?, args),
        Cmd::Watch(args) => commands::ops::watch(&entity_client().await?, args).await,
        Cmd::Completions(args) => commands::ops::completions(&args.shell),
        Cmd::Bypass(sub) => commands::bypass::run(sub).await,
    }
}

/// Build an HTTP client pointed at the local daemon, auto-starting it if not
/// already running. The daemon binary (`sdid`) is expected to live as a
/// sibling of `sdi` on PATH.
async fn entity_client() -> Result<Client> {
    let paths = Paths::resolve()?;
    paths.ensure_dirs().ok();
    if !daemon_cmd::is_running(&paths) {
        daemon_cmd::start(&paths).await?;
    }
    Client::from_paths(&paths)
}

async fn handle_daemon(sub: DaemonCmd) -> Result<()> {
    let paths = Paths::resolve()?;
    paths.ensure_dirs().ok();
    match sub {
        DaemonCmd::Start => {
            let port = daemon_cmd::start(&paths).await?;
            println!(
                "{}",
                serde_json::json!({
                    "started": true,
                    "port": port,
                    "pid_file": paths.pid_file.display().to_string(),
                    "port_file": paths.port_file.display().to_string(),
                })
            );
            Ok(())
        }
        DaemonCmd::Stop => {
            daemon_cmd::stop(&paths).await?;
            println!("{}", serde_json::json!({ "stopped": true }));
            Ok(())
        }
        DaemonCmd::Status => {
            let st = daemon_cmd::status(&paths);
            println!(
                "{}",
                serde_json::json!({
                    "running": st.running,
                    "pid": st.pid,
                    "port": st.port,
                })
            );
            Ok(())
        }
    }
}

async fn handle_mcp() -> Result<()> {
    // The MCP server calls the daemon over HTTP, so the daemon must be up
    // before we surrender stdin/stdout to the JSON-RPC loop.
    let paths = Paths::resolve()?;
    paths.ensure_dirs().ok();
    if !daemon_cmd::is_running(&paths) {
        daemon_cmd::start(&paths).await?;
    }
    sdi_mcp::run_stdio().await
}

async fn handle_doctor() -> Result<()> {
    let report = doctor::run()?;
    let code = doctor::exit_code(&report);
    println!("{}", serde_json::to_string_pretty(&report)?);
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}
