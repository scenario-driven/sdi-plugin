//! SDI CLI internals. The binary is `src/main.rs`; this lib hosts the
//! reusable pieces (clap App, daemon lifecycle helpers, doctor checks, HTTP
//! client) so integration tests can drive them directly without spawning a
//! subprocess.

pub mod cli;
pub mod commands;
pub mod daemon_cmd;
pub mod doctor;
pub mod http;
pub mod output;
