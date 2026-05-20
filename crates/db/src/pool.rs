//! r2d2-managed rusqlite pool with WAL + FK pragmas.

use crate::{map_pool_err, map_sqlite_err};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::OpenFlags;
use sdi_core::error::DomainResult;
use std::path::Path;
use std::time::Duration;

pub type Pool = r2d2::Pool<SqliteConnectionManager>;
pub type PooledConn = r2d2::PooledConnection<SqliteConnectionManager>;

/// Open (or create) a pooled SQLite connection at `path`. WAL + foreign keys
/// are enabled per connection.
pub fn open_pool(path: &Path) -> DomainResult<Pool> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let manager = SqliteConnectionManager::file(path)
        .with_flags(flags)
        .with_init(|c| {
            c.busy_timeout(Duration::from_secs(5))?;
            c.pragma_update(None, "journal_mode", "WAL")?;
            c.pragma_update(None, "synchronous", "NORMAL")?;
            c.pragma_update(None, "foreign_keys", "ON")?;
            c.pragma_update(None, "temp_store", "MEMORY")?;
            Ok(())
        });
    r2d2::Pool::builder()
        .max_size(8)
        .connection_timeout(Duration::from_secs(5))
        .build(manager)
        .map_err(map_pool_err)
}

/// Run `f` inside an IMMEDIATE transaction.
pub fn tx<F, T>(conn: &mut rusqlite::Connection, f: F) -> DomainResult<T>
where
    F: FnOnce(&rusqlite::Transaction<'_>) -> DomainResult<T>,
{
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(map_sqlite_err)?;
    let out = f(&tx)?;
    tx.commit().map_err(map_sqlite_err)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_opens_in_tmp() {
        let tmp = std::env::temp_dir().join(format!("sdi-pool-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let pool = open_pool(&tmp).unwrap();
        let conn = pool.get().unwrap();
        let n: i64 = conn
            .query_row("SELECT 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let _ = std::fs::remove_file(&tmp);
    }
}
