-- Collaboration & observability surface (Clawket parity).
-- All three tables are append-friendly (no destructive PUT path); comments
-- support edit + delete, questions follow a single-answer model, and
-- activity is strictly append-only.

-- =========================================================================
-- comments — polymorphic anchor on plan_id | task_id | scenario_id | round_id.
-- Exactly one of the FKs is non-NULL (enforced by CHECK).
-- =========================================================================
CREATE TABLE IF NOT EXISTS comments (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    plan_id       TEXT REFERENCES plans(id)     ON DELETE CASCADE,
    task_id       TEXT REFERENCES tasks(id)     ON DELETE CASCADE,
    scenario_id   TEXT REFERENCES scenarios(id) ON DELETE CASCADE,
    round_id      TEXT REFERENCES rounds(id)    ON DELETE CASCADE,
    author        TEXT NOT NULL DEFAULT 'agent',  -- 'agent' | 'human' | actor name
    body          TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    CHECK (
        (CASE WHEN plan_id     IS NULL THEN 0 ELSE 1 END
       + CASE WHEN task_id     IS NULL THEN 0 ELSE 1 END
       + CASE WHEN scenario_id IS NULL THEN 0 ELSE 1 END
       + CASE WHEN round_id    IS NULL THEN 0 ELSE 1 END) = 1
    )
);
CREATE INDEX IF NOT EXISTS idx_comments_project    ON comments(project_id, created_at);
CREATE INDEX IF NOT EXISTS idx_comments_plan       ON comments(plan_id,    created_at) WHERE plan_id     IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_comments_task       ON comments(task_id,    created_at) WHERE task_id     IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_comments_scenario   ON comments(scenario_id,created_at) WHERE scenario_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_comments_round      ON comments(round_id,   created_at) WHERE round_id    IS NOT NULL;

-- =========================================================================
-- questions — single-answer Q&A anchored on a plan. `answered_at` flips the
-- lifecycle from `open` to `answered`. Questions are never deleted; clearing
-- an answer reopens the question.
-- =========================================================================
CREATE TABLE IF NOT EXISTS questions (
    id            TEXT PRIMARY KEY,
    plan_id       TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    asker         TEXT NOT NULL DEFAULT 'agent',
    body          TEXT NOT NULL,
    answer        TEXT,
    answered_by   TEXT,
    answered_at   TEXT,
    status        TEXT NOT NULL DEFAULT 'open'
                    CHECK (status IN ('open','answered')),
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    CHECK ((status = 'open'     AND answer IS NULL AND answered_at IS NULL)
       OR  (status = 'answered' AND answer IS NOT NULL AND answered_at IS NOT NULL))
);
CREATE INDEX IF NOT EXISTS idx_questions_plan_status ON questions(plan_id, status);

-- =========================================================================
-- activity — append-only log of every domain mutation. Acts as the source
-- of truth for `/projects/:id/timeline` and the dashboard's recent feed.
-- Distinct from `events` (which is the SSE wire log); activity is the
-- audit-grade record with human-readable summaries.
-- =========================================================================
CREATE TABLE IF NOT EXISTS activity (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    actor         TEXT NOT NULL DEFAULT 'agent',
    kind          TEXT NOT NULL,              -- e.g. "plan.approved", "task.completed"
    entity_id     TEXT,                       -- target entity (plan id / task id / …)
    summary       TEXT NOT NULL,              -- human line
    payload       TEXT NOT NULL DEFAULT '{}', -- JSON sidecar
    created_at    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_activity_project_created ON activity(project_id, created_at);
CREATE INDEX IF NOT EXISTS idx_activity_kind            ON activity(kind, created_at);
CREATE INDEX IF NOT EXISTS idx_activity_entity          ON activity(entity_id, created_at) WHERE entity_id IS NOT NULL;
