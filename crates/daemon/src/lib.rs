//! SDI daemon (`sdid`) — HTTP API + SSE event bus on a single tokio runtime.
//!
//! Surface is split across submodules:
//! - [`state`]     shared `AppState` (db pool + event broadcaster + paths)
//! - [`error`]     `DomainError` → JSON HTTP response mapping
//! - [`router`]    axum router assembly (one submodule per entity)
//! - [`events`]    tokio broadcast + SSE handler
//! - [`lifecycle`] pid / port / socket file management

pub mod error;
pub mod events;
pub mod lifecycle;
pub mod router;
pub mod state;

pub use error::{ApiError, ApiResult};
pub use state::{AppState, EventEnvelope};
