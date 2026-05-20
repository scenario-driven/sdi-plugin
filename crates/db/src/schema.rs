//! DDL + migration runner.
//!
//! Schema is versioned in `schema_migrations`. Each migration is idempotent
//! via `CREATE TABLE IF NOT EXISTS`. FTS5 mirrors are populated through
//! AFTER INSERT/UPDATE/DELETE triggers — no manual rebuild required.

use crate::{map_sqlite_err, pool::PooledConn};
use rusqlite::Connection;
use sdi_core::error::DomainResult;

/// All migrations applied in order. `(version, label, sql)`.
const MIGRATIONS: &[(i64, &str, &str)] = &[
    (1, "core entities + FTS5", MIGRATION_001_CORE),
    (2, "disruption review queue", MIGRATION_002_DISRUPTION),
    (3, "collaboration: comments + questions + activity", MIGRATION_003_COLLAB),
    (4, "runs + task hierarchy + lease", MIGRATION_004_RUNS),
    (5, "usage accounting", MIGRATION_005_USAGE),
];

const MIGRATION_001_CORE: &str = include_str!("./migrations/001_core.sql");
const MIGRATION_002_DISRUPTION: &str = include_str!("./migrations/002_disruption_reviews.sql");
const MIGRATION_003_COLLAB: &str = include_str!("./migrations/003_collab.sql");
const MIGRATION_004_RUNS: &str = include_str!("./migrations/004_runs_hierarchy.sql");
const MIGRATION_005_USAGE: &str = include_str!("./migrations/005_usage.sql");

/// Apply any pending migrations against `conn`. Idempotent.
pub fn ensure_schema(conn: &Connection) -> DomainResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (\
            version    INTEGER PRIMARY KEY,\
            label      TEXT NOT NULL,\
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))\
        );",
    )
    .map_err(map_sqlite_err)?;

    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )
        .map_err(map_sqlite_err)?;

    for (version, label, sql) in MIGRATIONS {
        if *version <= current {
            continue;
        }
        tracing::info!(version, label, "applying SDI schema migration");
        conn.execute_batch(sql).map_err(map_sqlite_err)?;
        conn.execute(
            "INSERT INTO schema_migrations(version, label) VALUES (?1, ?2)",
            rusqlite::params![version, label],
        )
        .map_err(map_sqlite_err)?;
    }
    Ok(())
}

/// Helper used by daemon startup to grab a pooled connection and apply migrations.
pub fn ensure_schema_on(conn: &PooledConn) -> DomainResult<()> {
    ensure_schema(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::open_pool;

    #[test]
    fn migrations_apply_idempotently() {
        let tmp = std::env::temp_dir().join(format!("sdi-schema-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let pool = open_pool(&tmp).unwrap();
        let conn = pool.get().unwrap();
        ensure_schema(&conn).unwrap();
        ensure_schema(&conn).unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for must in [
            "projects",
            "plans",
            "requirements",
            "decisions",
            "scenarios",
            "scenario_results",
            "rounds",
            "tasks",
            "knowledge",
            "events",
            "disruption_reviews",
            "schema_migrations",
        ] {
            assert!(tables.iter().any(|t| t == must), "missing table {must}");
        }
        let _ = std::fs::remove_file(&tmp);
    }
}
