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
    (
        3,
        "collaboration: comments + questions + activity",
        MIGRATION_003_COLLAB,
    ),
    (4, "runs + task hierarchy + lease", MIGRATION_004_RUNS),
    (5, "usage accounting", MIGRATION_005_USAGE),
    (
        6,
        "v0.4 multi-agent governance",
        MIGRATION_006_V04_MULTI_AGENT,
    ),
    (
        7,
        "v0.5 pattern enforcement",
        MIGRATION_007_V05_PATTERN_ENFORCEMENT,
    ),
];

const MIGRATION_001_CORE: &str = include_str!("./migrations/001_core.sql");
const MIGRATION_002_DISRUPTION: &str = include_str!("./migrations/002_disruption_reviews.sql");
const MIGRATION_003_COLLAB: &str = include_str!("./migrations/003_collab.sql");
const MIGRATION_004_RUNS: &str = include_str!("./migrations/004_runs_hierarchy.sql");
const MIGRATION_005_USAGE: &str = include_str!("./migrations/005_usage.sql");
const MIGRATION_006_V04_MULTI_AGENT: &str = include_str!("./migrations/006_v04_multi_agent.sql");
const MIGRATION_007_V05_PATTERN_ENFORCEMENT: &str =
    include_str!("./migrations/007_v05_pattern_enforcement.sql");

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
        // Wrap each migration in a transaction so partial failures (e.g. an
        // ALTER TABLE that runs after some CREATEs) leave the schema intact
        // and a retry sees the same starting state.
        conn.execute_batch("BEGIN").map_err(map_sqlite_err)?;
        let apply = conn.execute_batch(sql).and_then(|_| {
            conn.execute(
                "INSERT INTO schema_migrations(version, label) VALUES (?1, ?2)",
                rusqlite::params![version, label],
            )
            .map(|_| ())
        });
        match apply {
            Ok(()) => {
                conn.execute_batch("COMMIT").map_err(map_sqlite_err)?;
            }
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(map_sqlite_err(err));
            }
        }
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
            "autonomy_policies",
            "agent_notes",
            "agent_specs",
            "collaboration_patterns",
        ] {
            assert!(tables.iter().any(|t| t == must), "missing table {must}");
        }
        let scenario_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(scenarios)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for col in [
            "depends_on",
            "produced_by",
            "verified_by",
            "claimed_resources_json",
            "claim_status",
            "produced_via_pattern_id",
        ] {
            assert!(
                scenario_cols.iter().any(|c| c == col),
                "scenarios missing column {col}"
            );
        }
        let decision_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(decisions)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for col in [
            "kind",
            "proposal_id",
            "agent_name",
            "escalated_at",
            "reversal_plan",
            "blast_radius_score",
            "reversal_of",
            "produced_via_pattern_id",
        ] {
            assert!(
                decision_cols.iter().any(|c| c == col),
                "decisions missing column {col}"
            );
        }
        let autonomy_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(autonomy_policies)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for col in [
            "pattern_kind",
            "l5_threshold",
            "pattern_depth_cap",
            "plan_single_session_lock",
            "external_surface",
            "timeout_ms",
            "forced",
        ] {
            assert!(
                autonomy_cols.iter().any(|c| c == col),
                "autonomy_policies missing column {col}"
            );
        }
        let agent_spec_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(agent_specs)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for col in [
            "stance",
            "blast_radius_rules_json",
            "status",
            "expires_at",
            "tool_allowlist_json",
            "decision_kinds_json",
        ] {
            assert!(
                agent_spec_cols.iter().any(|c| c == col),
                "agent_specs missing column {col}"
            );
        }
        for col in ["produced_via_pattern_id"] {
            for table in ["plans", "requirements", "tasks", "rounds"] {
                let cols: Vec<String> = conn
                    .prepare(&format!("PRAGMA table_info({table})"))
                    .unwrap()
                    .query_map([], |r| r.get::<_, String>(1))
                    .unwrap()
                    .map(|r| r.unwrap())
                    .collect();
                assert!(
                    cols.iter().any(|c| c == col),
                    "{table} missing column {col}"
                );
            }
        }
        let _ = std::fs::remove_file(&tmp);
    }
}
