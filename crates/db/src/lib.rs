//! SDI storage adapter.
//!
//! Local SQLite (rusqlite + r2d2). FTS5 keyword search; vector search deferred
//! (PRD §5.2). Path resolution follows XDG with the LM-8 path-separation
//! invariant inherited from Clawket — user data MUST NOT resolve under
//! `~/.claude/plugins/`.
//!
//! Concrete schema migrations and CRUD live behind this crate; downstream
//! crates (cli, daemon, mcp) talk to the daemon HTTP/socket surface, not
//! directly to SQLite.

pub mod paths;
pub mod pool;
pub mod repo;
pub mod schema;

pub use paths::{Paths, ENV_ALLOW_OVERLAP, ENV_HOME_OVERRIDE};
pub use pool::{open_pool, tx, Pool, PooledConn};
pub use schema::ensure_schema;

/// Bootstrap a fresh pool + schema at a resolved [`Paths`] base. Used by the
/// daemon on startup.
pub fn open(paths: &Paths) -> DomainResult<Pool> {
    paths
        .ensure_dirs()
        .map_err(|e| DomainError::Validation(format!("create xdg dirs: {e}")))?;
    let pool = pool::open_pool(&paths.db_file)?;
    {
        let conn = pool.get().map_err(map_pool_err)?;
        ensure_schema(&conn)?;
    }
    Ok(pool)
}

use sdi_core::error::{DomainError, DomainResult};

/// Map a rusqlite error into a domain error so callers do not depend on rusqlite types.
pub fn map_sqlite_err(err: rusqlite::Error) -> DomainError {
    match err {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound("row not found".into()),
        rusqlite::Error::SqliteFailure(ref code, ref msg)
            if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
        {
            DomainError::Conflict(msg.clone().unwrap_or_else(|| "unique constraint".into()))
        }
        other => DomainError::Validation(format!("sqlite: {other}")),
    }
}

/// Map a r2d2 pool error.
pub fn map_pool_err(err: r2d2::Error) -> DomainError {
    DomainError::Validation(format!("pool: {err}"))
}

pub type Result<T> = DomainResult<T>;
