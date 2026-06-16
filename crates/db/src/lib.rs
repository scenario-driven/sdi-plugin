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
            DomainError::Conflict(
                msg.as_deref()
                    .map(humanize_unique_violation)
                    .unwrap_or_else(|| "unique constraint".into()),
            )
        }
        other => DomainError::Validation(format!("sqlite: {other}")),
    }
}

/// Translate SQLite's raw UNIQUE failure text ("UNIQUE constraint failed:
/// scenarios.plan_id, scenarios.short_code") into a message that names the
/// uniqueness scope, so a 409 tells the caller *where* the value collides
/// instead of leaking constraint internals.
fn humanize_unique_violation(msg: &str) -> String {
    let Some(rest) = msg.strip_prefix("UNIQUE constraint failed: ") else {
        return msg.to_string();
    };
    let cols: Vec<(&str, &str)> = rest.split(", ").filter_map(|c| c.split_once('.')).collect();
    let Some((table, _)) = cols.first() else {
        return msg.to_string();
    };
    let names: Vec<&str> = cols.iter().map(|c| c.1).collect();
    match names.as_slice() {
        [scope, "short_code"] => {
            let noun = match *scope {
                "project_id" => "project",
                "plan_id" => "plan",
                other => other,
            };
            // A cancelled/terminal entity keeps its short_code (#6: codes are
            // single-purpose ticket labels and are not recycled), so a "used"
            // code may belong to a row that is no longer active — pick a new
            // suffix rather than expecting the code to free up.
            format!(
                "{table}: short_code already used within this {noun} \
                 (cancelled/terminal {table} keep their code — choose a new suffix)"
            )
        }
        [single] => format!("{table}: {single} already exists"),
        _ => format!("{table}: duplicate value for unique ({})", names.join(", ")),
    }
}

/// Map a r2d2 pool error.
pub fn map_pool_err(err: r2d2::Error) -> DomainError {
    DomainError::Validation(format!("pool: {err}"))
}

pub type Result<T> = DomainResult<T>;
