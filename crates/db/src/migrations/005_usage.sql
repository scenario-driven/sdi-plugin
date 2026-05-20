-- Token / tool usage accounting. One row per run-level usage emission; the
-- daemon aggregates on read. Distinct from `runs.payload` (which is opaque
-- JSON) — this table is structured so `/plans/:id/usage` can answer without
-- decoding JSON for every run.

CREATE TABLE IF NOT EXISTS usage_records (
    id              TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    plan_id         TEXT REFERENCES plans(id)    ON DELETE CASCADE,
    task_id         TEXT REFERENCES tasks(id)    ON DELETE CASCADE,
    run_id          TEXT REFERENCES runs(id)     ON DELETE CASCADE,
    model           TEXT NOT NULL DEFAULT '',
    tier            TEXT NOT NULL DEFAULT 'med'
                      CHECK (tier IN ('low','med','high')),
    input_tokens    INTEGER NOT NULL DEFAULT 0,
    output_tokens   INTEGER NOT NULL DEFAULT 0,
    cache_read      INTEGER NOT NULL DEFAULT 0,
    cache_write     INTEGER NOT NULL DEFAULT 0,
    tool_calls      INTEGER NOT NULL DEFAULT 0,
    cost_usd        REAL    NOT NULL DEFAULT 0,
    created_at      TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_usage_project  ON usage_records(project_id, created_at);
CREATE INDEX IF NOT EXISTS idx_usage_plan     ON usage_records(plan_id,    created_at) WHERE plan_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_usage_task     ON usage_records(task_id,    created_at) WHERE task_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_usage_run      ON usage_records(run_id,     created_at) WHERE run_id  IS NOT NULL;
