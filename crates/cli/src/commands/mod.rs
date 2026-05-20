//! Per-entity subcommand handlers. Each module thinly wraps the corresponding
//! daemon HTTP router; argument shapes live in `crate::cli`, output formatting
//! goes through `crate::output::emit`.

pub mod aggregate;
pub mod comment;
pub mod decision;
pub mod impexp;
pub mod knowledge;
pub mod ops;
pub mod plan;
pub mod project;
pub mod question;
pub mod requirement;
pub mod round;
pub mod run;
pub mod scenario;
pub mod task;
pub mod usage;
