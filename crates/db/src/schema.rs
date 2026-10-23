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
    (8, "project metadata", MIGRATION_008_PROJECT_METADATA),
    (
        9,
        "D23 direct-provenance back-fill",
        MIGRATION_009_DIRECT_PROVENANCE_BACKFILL,
    ),
    (
        10,
        "short_code scope: per-plan composite UNIQUE",
        MIGRATION_010_SHORT_CODE_SCOPE,
    ),
    (
        11,
        "D23 direct-provenance back-fill rerun",
        MIGRATION_011_DIRECT_PROVENANCE_BACKFILL_RERUN,
    ),
    (12, "scenario retirement", MIGRATION_012_SCENARIO_RETIRE),
    (
        13,
        "provisional decision supersede-when",
        MIGRATION_013_DECISION_SUPERSEDE_WHEN,
    ),
    (
        14,
        "round verification baseline",
        MIGRATION_014_ROUND_BASELINE,
    ),
    (15, "chore task kind", MIGRATION_015_TASK_KIND),
    (
        16,
        "v2 oracle layer: ssot graph + user flows + decision-question engine",
        MIGRATION_016_ORACLE_LAYER,
    ),
    (17, "v2 plan↔flow targeting", MIGRATION_017_PLAN_FLOWS),
    (
        18,
        "verification integrity: run fingerprints, retry budgets, fixed baseline",
        MIGRATION_018_VERIFICATION_INTEGRITY,
    ),
    (
        19,
        "failure cause provenance",
        MIGRATION_019_FAILURE_IDENTITY,
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
const MIGRATION_008_PROJECT_METADATA: &str = include_str!("./migrations/008_project_metadata.sql");
const MIGRATION_009_DIRECT_PROVENANCE_BACKFILL: &str =
    include_str!("./migrations/009_direct_provenance_backfill.sql");
const MIGRATION_010_SHORT_CODE_SCOPE: &str = include_str!("./migrations/010_short_code_scope.sql");
const MIGRATION_011_DIRECT_PROVENANCE_BACKFILL_RERUN: &str =
    include_str!("./migrations/011_direct_provenance_backfill_rerun.sql");
const MIGRATION_012_SCENARIO_RETIRE: &str = include_str!("./migrations/012_scenario_retire.sql");
const MIGRATION_013_DECISION_SUPERSEDE_WHEN: &str =
    include_str!("./migrations/013_decision_supersede_when.sql");
const MIGRATION_014_ROUND_BASELINE: &str = include_str!("./migrations/014_round_baseline.sql");
const MIGRATION_015_TASK_KIND: &str = include_str!("./migrations/015_task_kind.sql");
const MIGRATION_016_ORACLE_LAYER: &str = include_str!("./migrations/016_oracle_layer.sql");
const MIGRATION_017_PLAN_FLOWS: &str = include_str!("./migrations/017_plan_flows.sql");
const MIGRATION_018_VERIFICATION_INTEGRITY: &str =
    include_str!("./migrations/018_verification_integrity.sql");
const MIGRATION_019_FAILURE_IDENTITY: &str = include_str!("./migrations/019_failure_identity.sql");

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
        // Table-rebuild migrations (007, 010) DROP parent tables. With
        // foreign-key enforcement on, DROP TABLE acts as DELETE FROM and
        // ON DELETE CASCADE would wipe child rows — so enforcement is
        // switched off for the migration. `PRAGMA foreign_keys` is a no-op
        // inside a transaction, so the switch must happen before BEGIN, and
        // referential integrity is re-asserted via `PRAGMA foreign_key_check`
        // before COMMIT (the documented SQLite table-rebuild procedure).
        conn.pragma_update(None, "foreign_keys", "OFF")
            .map_err(map_sqlite_err)?;
        // Wrap each migration in a transaction so partial failures (e.g. an
        // ALTER TABLE that runs after some CREATEs) leave the schema intact
        // and a retry sees the same starting state.
        let apply = conn
            .execute_batch("BEGIN")
            .and_then(|_| conn.execute_batch(sql))
            .and_then(|_| {
                conn.execute(
                    "INSERT INTO schema_migrations(version, label) VALUES (?1, ?2)",
                    rusqlite::params![version, label],
                )
                .map(|_| ())
            })
            .and_then(|_| {
                let violation: Option<String> = conn
                    .query_row("PRAGMA foreign_key_check", [], |r| r.get(0))
                    .map(Some)
                    .or_else(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => Ok(None),
                        other => Err(other),
                    })?;
                if let Some(table) = violation {
                    return Err(rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY),
                        Some(format!(
                            "migration {version} ({label}) broke referential integrity in table {table}"
                        )),
                    ));
                }
                Ok(())
            });
        let result = match apply {
            Ok(()) => conn.execute_batch("COMMIT").map_err(map_sqlite_err),
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(map_sqlite_err(err))
            }
        };
        let restore = conn
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(map_sqlite_err);
        result?;
        restore?;
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
            "retry_budgets",
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
        // 008_project_metadata.sql — Project lifecycle columns.
        let project_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(projects)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for col in ["description", "enabled", "wiki_paths_json"] {
            assert!(
                project_cols.iter().any(|c| c == col),
                "projects missing column {col}"
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

    // 018 upgrade path: a pre-018 database with a task run and a scenario
    // verdict must keep both rows; legacy runs gain plan/round links through
    // their task, and the widened verdict CHECK admits needs_verification.
    #[test]
    fn migration_018_preserves_runs_and_results() {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-schema-018-{}-{}.db",
            std::process::id(),
            ulid::Ulid::new()
        ));
        let _ = std::fs::remove_file(&tmp);
        let pool = open_pool(&tmp).unwrap();
        let conn = pool.get().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (\
                version    INTEGER PRIMARY KEY,\
                label      TEXT NOT NULL,\
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))\
            );",
        )
        .unwrap();
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        for (version, label, sql) in MIGRATIONS.iter().filter(|(v, _, _)| *v <= 17) {
            conn.execute_batch(sql).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations(version, label) VALUES (?1, ?2)",
                rusqlite::params![version, label],
            )
            .unwrap();
        }
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        let ts = "2026-01-01T00:00:00.000Z";
        conn.execute_batch(&format!(
            "INSERT INTO projects(id, key, name, slug, created_at, updated_at)
                 VALUES ('PROJ-A', 'TSA', 'a', 'sa', '{ts}', '{ts}');
             INSERT INTO plans(id, project_id, short_code, title, status, created_at, updated_at)
                 VALUES ('PLAN-A', 'PROJ-A', 'P-1', 'plan a', 'active', '{ts}', '{ts}');
             INSERT INTO scenarios(id, plan_id, short_code, given, when_clause, then_clause, created_at, updated_at)
                 VALUES ('SCN-A1', 'PLAN-A', 'SC-1', 'g', 'w', 't', '{ts}', '{ts}');
             INSERT INTO rounds(id, plan_id, short_code, round_number, status, created_at, updated_at)
                 VALUES ('ROUND-A1', 'PLAN-A', 'R-1', 1, 'active', '{ts}', '{ts}');
             INSERT INTO tasks(id, round_id, plan_id, short_code, description, created_at, updated_at)
                 VALUES ('TASK-A1', 'ROUND-A1', 'PLAN-A', 'T-1', 'x', '{ts}', '{ts}');
             INSERT INTO runs(id, task_id, actor, started_at, finished_at, result, payload)
                 VALUES ('RUN-A1', 'TASK-A1', 'agent', '{ts}', '{ts}', 'failure', '{{}}');
             INSERT INTO usage_records(id, project_id, run_id, created_at)
                 VALUES ('USE-A1', 'PROJ-A', 'RUN-A1', '{ts}');
             INSERT INTO scenario_results(round_id, scenario_id, result, note, evidence_ref, updated_at)
                 VALUES ('ROUND-A1', 'SCN-A1', 'passing', 'n', 'src/a.rs:1', '{ts}');"
        ))
        .unwrap();

        ensure_schema(&conn).unwrap();

        let (task, plan, round): (String, String, String) = conn
            .query_row(
                "SELECT task_id, plan_id, round_id FROM runs WHERE id = 'RUN-A1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (task.as_str(), plan.as_str(), round.as_str()),
            ("TASK-A1", "PLAN-A", "ROUND-A1")
        );
        let (verdict, evidence): (String, String) = conn
            .query_row(
                "SELECT result, evidence_ref FROM scenario_results WHERE scenario_id = 'SCN-A1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (verdict.as_str(), evidence.as_str()),
            ("passing", "src/a.rs:1")
        );
        conn.execute(
            "UPDATE scenario_results SET result = 'needs_verification' WHERE scenario_id = 'SCN-A1'",
            [],
        )
        .unwrap();
        // usage_records still references the rebuilt runs table.
        let usage_run: String = conn
            .query_row(
                "SELECT run_id FROM usage_records WHERE id = 'USE-A1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(usage_run, "RUN-A1");
        let fk_violation = conn
            .query_row("PRAGMA foreign_key_check", [], |r| r.get::<_, String>(0))
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
            .unwrap();
        assert_eq!(fk_violation, None);
        let cap: i64 = conn
            .query_row(
                "SELECT dflt_value FROM pragma_table_info('autonomy_policies') WHERE name = 'retry_budget_cap'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(cap, 3);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn migration_019_preserves_failure_history_and_totals() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, label TEXT NOT NULL);",
        )
        .unwrap();
        for (version, label, sql) in MIGRATIONS.iter().filter(|(v, _, _)| *v <= 18) {
            conn.execute_batch(sql).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations(version, label) VALUES (?1, ?2)",
                rusqlite::params![version, label],
            )
            .unwrap();
        }
        conn.execute_batch(
            "INSERT INTO projects(id,key,name,slug,created_at,updated_at)
                 VALUES ('project','history','history','history','2026-01-01','2026-01-01');
             INSERT INTO plans(id,project_id,short_code,title,status,created_at,updated_at)
                 VALUES ('plan','project','history','history','active','2026-01-01','2026-01-01');
             INSERT INTO scenarios(id,plan_id,short_code,given,when_clause,then_clause,created_at,updated_at)
                 VALUES ('scenario','plan','history','g','w','t','2026-01-01','2026-01-01');
             INSERT INTO runs(id,plan_id,scenario_id,actor,started_at,finished_at,result,payload,
                 exit_code,report_path,report_sha256,failure_class,failure_signature,error_class)
                 VALUES ('run','plan','scenario','agent','2026-01-01','2026-01-01','failure',
                 '{\"original\":true}',1,'reports/original.log','original-digest','product','sig:legacy','assertion');
             INSERT INTO retry_budgets(plan_id,scenario_id,failure_class,failure_signature,count,
                 total_failures,first_run_id,last_run_id,last_failed_at)
                 VALUES ('plan','scenario','product','sig:legacy',3,7,'run','run','2026-01-01');",
        )
        .unwrap();
        let snapshot = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, String>(0)).unwrap();
        let run_sql = "SELECT json_array(result,payload,exit_code,report_path,report_sha256,
            failure_class,failure_signature,error_class) FROM runs WHERE id='run'";
        let budget_sql = "SELECT json_array(plan_id,scenario_id,failure_class,failure_signature,
            count,total_failures,first_run_id,last_run_id,last_failed_at,released_by_decision_id,
            released_at) FROM retry_budgets";
        let original_run = snapshot(run_sql);
        let original_budget = snapshot(budget_sql);

        ensure_schema(&conn).unwrap();
        ensure_schema(&conn).unwrap();

        assert_eq!(snapshot(run_sql), original_run);
        assert_eq!(snapshot(budget_sql), original_budget);
        let provenance: Option<String> = conn
            .query_row("SELECT failure_identity_json FROM runs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(provenance, None);
    }

    // 010 upgrade path: a populated pre-010 database (global short_code
    // UNIQUE, tasks without plan_id) must come out of the rebuild with all
    // rows intact, tasks.plan_id backfilled through rounds, FTS mirrors
    // still answering, FK graph clean, and short_code scoped per plan.
    #[test]
    fn migration_010_rescopes_short_code_preserving_data() {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-schema-010-{}-{}.db",
            std::process::id(),
            ulid::Ulid::new()
        ));
        let _ = std::fs::remove_file(&tmp);
        let pool = open_pool(&tmp).unwrap();
        let conn = pool.get().unwrap();

        // Build the pre-010 schema by applying versions 1..=9 only.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (\
                version    INTEGER PRIMARY KEY,\
                label      TEXT NOT NULL,\
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))\
            );",
        )
        .unwrap();
        for (version, label, sql) in MIGRATIONS.iter().filter(|(v, _, _)| *v <= 9) {
            conn.execute_batch(sql).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations(version, label) VALUES (?1, ?2)",
                rusqlite::params![version, label],
            )
            .unwrap();
        }

        // Two plans; the global UNIQUE means SC-1 can exist in only one.
        // PLAN-A additionally carries a RUNTIME-minted direct sentinel
        // (`CP-<ulid>` id scheme from ensure_direct_pattern, NOT 009/011's
        // deterministic `CP-DIRECT-<plan_id>`) alongside a NULL-provenance
        // scenario — the dogfooding shape that made the first cut of 011
        // collide with 010's (plan_id, short_code) UNIQUE.
        let ts = "2026-01-01T00:00:00.000Z";
        conn.execute_batch(&format!(
            "INSERT INTO projects(id, key, name, slug, created_at, updated_at)
                 VALUES ('PROJ-A', 'TSA', 'a', 'sa', '{ts}', '{ts}');
             INSERT INTO plans(id, project_id, short_code, title, status, created_at, updated_at)
                 VALUES ('PLAN-A', 'PROJ-A', 'P-1', 'plan a', 'active', '{ts}', '{ts}'),
                        ('PLAN-B', 'PROJ-A', 'P-2', 'plan b', 'draft',  '{ts}', '{ts}');
             INSERT INTO scenarios(id, plan_id, short_code, given, when_clause, then_clause, created_at, updated_at)
                 VALUES ('SCN-A1', 'PLAN-A', 'SC-1', 'a given', 'a when', 'a then', '{ts}', '{ts}');
             INSERT INTO rounds(id, plan_id, short_code, round_number, status, created_at, updated_at)
                 VALUES ('ROUND-A1', 'PLAN-A', 'R-1', 1, 'active', '{ts}', '{ts}');
             INSERT INTO tasks(id, round_id, short_code, description, created_at, updated_at)
                 VALUES ('TASK-A1', 'ROUND-A1', 'T-1', 'wire cli', '{ts}', '{ts}');
             INSERT INTO collaboration_patterns(id, short_code, plan_id, kind, applies_to, scope_id, depth, lifecycle, created_at, updated_at)
                 VALUES ('CP-01RUNTIME', 'DIRECT-P-1', 'PLAN-A', 'direct', 'plan', 'PLAN-A', 0, 'active', '{ts}', '{ts}');"
        ))
        .unwrap();

        // Apply 010 + 011 through the real runner.
        ensure_schema(&conn).unwrap();

        // 011 reuses the runtime sentinel instead of minting a duplicate
        // (which would violate the fresh (plan_id, short_code) UNIQUE) and
        // links the NULL-provenance rows to it.
        let direct_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM collaboration_patterns WHERE plan_id = 'PLAN-A' AND kind = 'direct'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(direct_count, 1, "011 must not duplicate a runtime sentinel");
        let scn_prov: Option<String> = conn
            .query_row(
                "SELECT produced_via_pattern_id FROM scenarios WHERE id = 'SCN-A1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(scn_prov.as_deref(), Some("CP-01RUNTIME"));
        let task_prov: Option<String> = conn
            .query_row(
                "SELECT produced_via_pattern_id FROM tasks WHERE id = 'TASK-A1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(task_prov.as_deref(), Some("CP-01RUNTIME"));

        // Rows survived; tasks.plan_id backfilled through the round join.
        let task_plan: String = conn
            .query_row("SELECT plan_id FROM tasks WHERE id = 'TASK-A1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(task_plan, "PLAN-A");

        // FK graph clean after the rebuild.
        let fk_violation = conn
            .query_row("PRAGMA foreign_key_check", [], |r| r.get::<_, String>(0))
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
            .unwrap();
        assert_eq!(fk_violation, None);

        // FTS mirror still answers for pre-migration content.
        let fts_hits: i64 = conn
            .query_row(
                "SELECT count(*) FROM scenarios_fts WHERE scenarios_fts MATCH 'given'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_hits, 1);

        // The documented contract: SC-1 is now mintable in the other plan…
        conn.execute(
            "INSERT INTO scenarios(id, plan_id, short_code, given, when_clause, then_clause, created_at, updated_at)
                 VALUES ('SCN-B1', 'PLAN-B', 'SC-1', 'b given', 'b when', 'b then', ?1, ?1)",
            [ts],
        )
        .unwrap();
        // …while a duplicate within the same plan stays a conflict, reported
        // with the uniqueness scope rather than raw constraint text.
        let dup = conn.execute(
            "INSERT INTO scenarios(id, plan_id, short_code, given, when_clause, then_clause, created_at, updated_at)
                 VALUES ('SCN-A2', 'PLAN-A', 'SC-1', 'x', 'y', 'z', ?1, ?1)",
            [ts],
        );
        let err = crate::map_sqlite_err(dup.unwrap_err());
        match err {
            sdi_core::error::DomainError::Conflict(msg) => {
                assert!(
                    msg.contains("within this plan"),
                    "conflict message should name the plan scope, got: {msg}"
                );
            }
            other => panic!("expected Conflict, got {other:?}"),
        }
        let _ = std::fs::remove_file(&tmp);
    }
}
