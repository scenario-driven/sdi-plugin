-- =========================================================================
-- 018 — verification-run integrity.
--
-- Four defects measured across real projects share one root cause: the
-- daemon could not tell *what* a verification proved, *why* it failed, or
-- *against which baseline* progress was measured.
--
--   1. Evidence was free text with no input fingerprint, so a pass recorded
--      against old code carried into later rounds as if still true.
--   2. Run records knew only success/failure/aborted, so a missing binary or
--      a timeout was indistinguishable from a product defect.
--   3. Same-cause failures were retried under new task names with no cap —
--      nothing keyed a failure to its cause independently of the task.
--   4. The round baseline was a free JSON blob any caller could overwrite.
--
-- This migration gives each of those a structural home:
--
--   * runs           — rebuilt: task_id becomes optional (a verification run
--                      is scenario-scoped), plus plan/round/scenario links,
--                      command/cwd/inputs, input fingerprint, exit status,
--                      report + digest, failure class + signature.
--   * scenario_results — rebuilt: the verdict CHECK admits the daemon-only
--                      `needs_verification` (a carried pass whose fingerprint
--                      no longer matches).
--   * retry_budgets  — failure counters keyed (plan, scenario, failure class,
--                      signature), independent of task id/title.
--   * rounds         — activation baseline (commit + verdict snapshot) and the
--                      decision that last superseded the baseline.
--   * autonomy_policies — per-plan `retry_budget_cap` (default 3).
--
-- Table rebuilds follow the procedure documented in 010 (foreign keys are
-- switched off by the runner and re-checked before COMMIT). Neither rebuilt
-- table carries an FTS mirror or triggers.
-- =========================================================================

-- ── runs ────────────────────────────────────────────────────────────────
CREATE TABLE runs_new (
    id                  TEXT PRIMARY KEY,
    task_id             TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    plan_id             TEXT REFERENCES plans(id) ON DELETE CASCADE,
    round_id            TEXT REFERENCES rounds(id) ON DELETE SET NULL,
    scenario_id         TEXT REFERENCES scenarios(id) ON DELETE CASCADE,
    actor               TEXT NOT NULL DEFAULT 'agent',
    session_id          TEXT,
    started_at          TEXT NOT NULL,
    finished_at         TEXT,
    result              TEXT,
    notes               TEXT,
    payload             TEXT NOT NULL DEFAULT '{}',
    command_json        TEXT,                              -- JSON argv
    cwd                 TEXT,
    input_globs_json    TEXT NOT NULL DEFAULT '[]',
    tool_versions_json  TEXT NOT NULL DEFAULT '{}',
    input_fingerprint   TEXT,                              -- sha256:<hex>
    exit_code           INTEGER,
    signal              INTEGER,
    timed_out           INTEGER NOT NULL DEFAULT 0,
    spawn_error         TEXT
                          CHECK (spawn_error IS NULL OR spawn_error IN ('not-found','permission-denied','other')),
    report_path         TEXT,
    report_sha256       TEXT,
    failure_class       TEXT
                          CHECK (failure_class IS NULL OR failure_class IN ('product','harness','environment','spec','none')),
    failure_signature   TEXT,
    error_class         TEXT,
    failing_test        TEXT,
    duration_ms         INTEGER,
    CHECK (finished_at IS NULL OR result IN ('success','failure','aborted')),
    CHECK (task_id IS NOT NULL OR scenario_id IS NOT NULL)
);
INSERT INTO runs_new (id, task_id, plan_id, round_id, actor, session_id,
                      started_at, finished_at, result, notes, payload)
SELECT r.id, r.task_id, t.plan_id, t.round_id, r.actor, r.session_id,
       r.started_at, r.finished_at, r.result, r.notes, r.payload
FROM runs r
LEFT JOIN tasks t ON t.id = r.task_id;
DROP TABLE runs;
ALTER TABLE runs_new RENAME TO runs;
CREATE INDEX idx_runs_task     ON runs(task_id, started_at) WHERE task_id IS NOT NULL;
CREATE INDEX idx_runs_session  ON runs(session_id) WHERE session_id IS NOT NULL;
CREATE INDEX idx_runs_running  ON runs(task_id) WHERE finished_at IS NULL;
CREATE INDEX idx_runs_scenario ON runs(scenario_id, finished_at) WHERE scenario_id IS NOT NULL;
CREATE INDEX idx_runs_round    ON runs(round_id, scenario_id) WHERE round_id IS NOT NULL;

-- ── scenario_results ────────────────────────────────────────────────────
CREATE TABLE scenario_results_new (
    round_id      TEXT NOT NULL REFERENCES rounds(id) ON DELETE CASCADE,
    scenario_id   TEXT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    result        TEXT NOT NULL
                    CHECK (result IN ('passing','failing','impacted','retired','needs_verification')),
    note          TEXT,
    evidence_ref  TEXT,
    updated_at    TEXT NOT NULL,
    PRIMARY KEY (round_id, scenario_id)
);
INSERT INTO scenario_results_new (round_id, scenario_id, result, note, evidence_ref, updated_at)
SELECT round_id, scenario_id, result, note, evidence_ref, updated_at
FROM scenario_results;
DROP TABLE scenario_results;
ALTER TABLE scenario_results_new RENAME TO scenario_results;
CREATE INDEX idx_scenario_results_scenario ON scenario_results(scenario_id);

-- ── retry_budgets ───────────────────────────────────────────────────────
-- One counter per failure cause. `count` restarts at 0 only when a decision
-- releases it (released_by_decision_id); `total_failures` never resets.
CREATE TABLE retry_budgets (
    plan_id                  TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    scenario_id              TEXT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    failure_class            TEXT NOT NULL
                               CHECK (failure_class IN ('product','harness','environment','spec')),
    failure_signature        TEXT NOT NULL,
    count                    INTEGER NOT NULL DEFAULT 0 CHECK (count >= 0),
    total_failures           INTEGER NOT NULL DEFAULT 0 CHECK (total_failures >= 0),
    first_run_id             TEXT REFERENCES runs(id) ON DELETE SET NULL,
    last_run_id              TEXT REFERENCES runs(id) ON DELETE SET NULL,
    last_failed_at           TEXT NOT NULL,
    released_by_decision_id  TEXT REFERENCES decisions(id) ON DELETE SET NULL,
    released_at              TEXT,
    PRIMARY KEY (plan_id, scenario_id, failure_class, failure_signature)
);
CREATE INDEX idx_retry_budgets_scenario ON retry_budgets(scenario_id);

-- ── rounds: activation baseline ─────────────────────────────────────────
ALTER TABLE rounds ADD COLUMN baseline_commit TEXT;
ALTER TABLE rounds ADD COLUMN baseline_snapshot_json TEXT;
ALTER TABLE rounds ADD COLUMN baseline_recorded_at TEXT;
ALTER TABLE rounds ADD COLUMN baseline_decision_id TEXT REFERENCES decisions(id) ON DELETE SET NULL;

-- ── autonomy_policies: per-plan retry budget cap ────────────────────────
ALTER TABLE autonomy_policies ADD COLUMN retry_budget_cap INTEGER NOT NULL DEFAULT 3
    CHECK (retry_budget_cap BETWEEN 1 AND 100);
