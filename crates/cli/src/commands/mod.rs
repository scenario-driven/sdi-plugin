//! Per-entity subcommand handlers. Each module thinly wraps the corresponding
//! daemon HTTP router; argument shapes live in `crate::cli`, output formatting
//! goes through `crate::output::emit`.

pub mod agent_note;
pub mod aggregate;
pub mod autonomy;
pub mod comment;
pub mod consensus;
pub mod decision;
pub mod impexp;
pub mod knowledge;
pub mod ops;
pub mod pattern;
pub mod plan;
pub mod project;
pub mod question;
pub mod requirement;
pub mod round;
pub mod run;
pub mod scenario;
pub mod task;
pub mod usage;
