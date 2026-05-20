//! Shared application state cloned into every axum handler.
//!
//! Holds:
//! - a r2d2 SQLite pool (cheap to clone, all handlers share connections)
//! - a tokio broadcast sender so handlers can publish events to SSE subscribers
//! - the resolved [`Paths`] so handlers can find sockets / port files / log files

use sdi_db::{Pool, Paths};
use std::sync::Arc;
use tokio::sync::broadcast;

/// SSE event envelope. Kept tiny so the broadcast channel doesn't bloat.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EventEnvelope {
    pub kind: String,
    pub entity_id: Option<String>,
    pub payload: serde_json::Value,
}

#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
    pub paths: Arc<Paths>,
    pub events: broadcast::Sender<EventEnvelope>,
}

impl AppState {
    pub fn new(pool: Pool, paths: Arc<Paths>) -> Self {
        // Capacity big enough to absorb a burst from a CLI script.
        let (tx, _rx) = broadcast::channel(256);
        Self { pool, paths, events: tx }
    }

    /// Best-effort publish: subscribers may be 0 (no-op) or lagging (dropped).
    pub fn publish(&self, ev: EventEnvelope) {
        let _ = self.events.send(ev);
    }

    pub fn conn(&self) -> Result<sdi_db::PooledConn, sdi_core::error::DomainError> {
        self.pool.get().map_err(sdi_db::map_pool_err)
    }
}
