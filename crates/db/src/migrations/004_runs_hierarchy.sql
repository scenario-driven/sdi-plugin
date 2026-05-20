-- Task runtime: execution sessions, hierarchy (decompose), single-writer lease.
--
-- - `runs`           — one row per CLI/Agent execution attempt on a task.
-- - `task_relations` — parent→subtask tree (decompose). Distinct from
--                      `parent_scenario_ids` (which links a task to scenarios it
--                      satisfies); this is structural decomposition.
-- - `task_leases`    — single-writer lease per task with heartbeat. Distinct
--                      from `status='in_progress'`, which is a domain flag.

-- =========================================================================
-- runs — execution attempts. A task may have many runs (retries, parallel
-- exploration). `result` is NULL until `finished_at` is set.
-- =========================================================================
CREATE TABLE IF NOT EXISTS runs (
    id            TEXT PRIMARY KEY,
    task_id       TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    actor         TEXT NOT NULL DEFAULT 'agent',
    session_id    TEXT,                          -- Claude Code session uuid (optional)
    started_at    TEXT NOT NULL,
    finished_at   TEXT,
    result        TEXT,                          -- 'success'|'failure'|'aborted'|NULL while running
    notes         TEXT,
    payload       TEXT NOT NULL DEFAULT '{}',    -- JSON sidecar (tokens, tool counts, …)
    CHECK (finished_at IS NULL OR result IN ('success','failure','aborted'))
);
CREATE INDEX IF NOT EXISTS idx_runs_task            ON runs(task_id, started_at);
CREATE INDEX IF NOT EXISTS idx_runs_session         ON runs(session_id) WHERE session_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_runs_running         ON runs(task_id) WHERE finished_at IS NULL;

-- =========================================================================
-- task_relations — directed graph of structural relations between tasks.
-- The dominant relation is `parent` (decomposition); other kinds are advisory.
-- (parent_id, child_id, kind) is the natural key.
-- =========================================================================
CREATE TABLE IF NOT EXISTS task_relations (
    id           TEXT PRIMARY KEY,
    parent_id    TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    child_id     TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL DEFAULT 'parent'
                   CHECK (kind IN ('parent','blocks','depends-on','duplicates','related')),
    created_at   TEXT NOT NULL,
    UNIQUE (parent_id, child_id, kind),
    CHECK (parent_id != child_id)
);
CREATE INDEX IF NOT EXISTS idx_task_relations_parent ON task_relations(parent_id, kind);
CREATE INDEX IF NOT EXISTS idx_task_relations_child  ON task_relations(child_id, kind);

-- =========================================================================
-- task_leases — single-writer mutex per task, with heartbeat-based expiry.
-- Acquiring a lease is independent from setting status='in_progress' so that
-- read-only inspection runs (e.g. dry-run) don't trip the writer guard.
-- =========================================================================
CREATE TABLE IF NOT EXISTS task_leases (
    task_id       TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    holder        TEXT NOT NULL,                 -- session id / agent id
    acquired_at   TEXT NOT NULL,
    heartbeat_at  TEXT NOT NULL,
    expires_at    TEXT NOT NULL                  -- callers refresh before this
);
CREATE INDEX IF NOT EXISTS idx_task_leases_expiry ON task_leases(expires_at);
