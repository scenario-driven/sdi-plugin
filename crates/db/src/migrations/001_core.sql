-- SDI v0.1 core schema. All entities + per-round scenario verdicts + event log + FTS5 mirrors.
-- IDs are ULID-prefixed strings (PROJ-, PLAN-, REQ-, DEC-, SCN-, ROUND-, TASK-, KN-).
-- short_code is the human ticket label ("SDI-12"). Timestamps are RFC3339 UTC strings.

-- =========================================================================
-- projects + cwd anchors
-- =========================================================================
CREATE TABLE IF NOT EXISTS projects (
    id          TEXT PRIMARY KEY,
    key         TEXT NOT NULL UNIQUE,            -- short prefix (e.g. "SDI")
    name        TEXT NOT NULL,
    slug        TEXT NOT NULL UNIQUE,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_cwds (
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    cwd         TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    PRIMARY KEY (project_id, cwd)
);
CREATE INDEX IF NOT EXISTS idx_project_cwds_cwd ON project_cwds(cwd);

-- =========================================================================
-- plans (D8: approve gate enforced in domain)
-- =========================================================================
CREATE TABLE IF NOT EXISTS plans (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    short_code    TEXT NOT NULL UNIQUE,
    title         TEXT NOT NULL,
    body          TEXT NOT NULL DEFAULT '',      -- markdown (SNAPSHOT-ONLY per D12)
    status        TEXT NOT NULL CHECK (status IN ('draft','active','completed')),
    version       INTEGER NOT NULL DEFAULT 0,
    approved_at   TEXT,
    completed_at  TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_plans_project_status ON plans(project_id, status);
-- D8: only one active plan per project
CREATE UNIQUE INDEX IF NOT EXISTS uniq_one_active_plan_per_project
    ON plans(project_id) WHERE status = 'active';

-- =========================================================================
-- requirements (SNAPSHOT-ONLY per D12; rewritten in place)
-- =========================================================================
CREATE TABLE IF NOT EXISTS requirements (
    id           TEXT PRIMARY KEY,
    plan_id      TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    short_code   TEXT NOT NULL UNIQUE,
    title        TEXT NOT NULL,
    body         TEXT NOT NULL,
    source       TEXT NOT NULL DEFAULT 'snapshot',
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_requirements_plan ON requirements(plan_id);

-- =========================================================================
-- decisions (append-only ADR; supersession links instead of edits)
-- =========================================================================
CREATE TABLE IF NOT EXISTS decisions (
    id              TEXT PRIMARY KEY,
    plan_id         TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    short_code      TEXT NOT NULL UNIQUE,
    title           TEXT NOT NULL,
    body            TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'accepted'
                      CHECK (status IN ('proposed','accepted','superseded')),
    supersedes_id   TEXT REFERENCES decisions(id) ON DELETE SET NULL,
    created_at      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_decisions_plan ON decisions(plan_id);

-- =========================================================================
-- scenarios (D5: GWT strict; non-empty enforced in domain layer)
-- =========================================================================
CREATE TABLE IF NOT EXISTS scenarios (
    id               TEXT PRIMARY KEY,
    plan_id          TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    short_code       TEXT NOT NULL UNIQUE,
    given            TEXT NOT NULL,
    when_clause      TEXT NOT NULL,
    then_clause      TEXT NOT NULL,
    tags             TEXT NOT NULL DEFAULT '[]', -- JSON array
    origin_round_id  TEXT REFERENCES rounds(id) ON DELETE SET NULL,
    status           TEXT NOT NULL DEFAULT 'draft'
                       CHECK (status IN ('draft','confirmed')),
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    CHECK (length(trim(given)) > 0),
    CHECK (length(trim(when_clause)) > 0),
    CHECK (length(trim(then_clause)) > 0)
);
CREATE INDEX IF NOT EXISTS idx_scenarios_plan ON scenarios(plan_id);

-- =========================================================================
-- rounds (D6 default strict-regression, D9 disruption policy, D10 in-flight policy)
-- =========================================================================
CREATE TABLE IF NOT EXISTS rounds (
    id                  TEXT PRIMARY KEY,
    plan_id             TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    short_code          TEXT NOT NULL UNIQUE,
    round_number        INTEGER NOT NULL,
    mode                TEXT NOT NULL DEFAULT 'strict-regression'
                          CHECK (mode IN ('strict-regression','forward-only')),
    in_flight_policy    TEXT NOT NULL DEFAULT 'pause'
                          CHECK (in_flight_policy IN ('pause','abort','continue-on-noimpact')),
    disruption_policy   TEXT NOT NULL DEFAULT 'needs-review'
                          CHECK (disruption_policy IN ('needs-review','auto')),
    status              TEXT NOT NULL CHECK (status IN ('planning','active','completed')),
    activated_at        TEXT,
    completed_at        TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    UNIQUE (plan_id, round_number)
);
CREATE INDEX IF NOT EXISTS idx_rounds_plan_status ON rounds(plan_id, status);
-- D8: only one active round per plan
CREATE UNIQUE INDEX IF NOT EXISTS uniq_one_active_round_per_plan
    ON rounds(plan_id) WHERE status = 'active';

-- =========================================================================
-- per-round scenario verdicts (R2+ auto-regression population happens here)
-- =========================================================================
CREATE TABLE IF NOT EXISTS scenario_results (
    round_id      TEXT NOT NULL REFERENCES rounds(id) ON DELETE CASCADE,
    scenario_id   TEXT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    result        TEXT NOT NULL
                    CHECK (result IN ('passing','failing','impacted','retired')),
    note          TEXT,
    evidence_ref  TEXT,
    updated_at    TEXT NOT NULL,
    PRIMARY KEY (round_id, scenario_id)
);
CREATE INDEX IF NOT EXISTS idx_scenario_results_scenario ON scenario_results(scenario_id);

-- =========================================================================
-- tasks (D3: runtime artifact; PRD §6.6 evidence required on done)
-- =========================================================================
CREATE TABLE IF NOT EXISTS tasks (
    id                       TEXT PRIMARY KEY,
    round_id                 TEXT NOT NULL REFERENCES rounds(id) ON DELETE CASCADE,
    short_code               TEXT NOT NULL UNIQUE,
    description              TEXT NOT NULL,
    status                   TEXT NOT NULL DEFAULT 'todo'
                               CHECK (status IN ('todo','in_progress','done','cancelled','blocked')),
    parent_scenario_ids      TEXT NOT NULL DEFAULT '[]',  -- JSON array of scenario IDs
    parent_requirement_ids   TEXT NOT NULL DEFAULT '[]',  -- JSON array of requirement IDs
    evidence                 TEXT,                         -- JSON: TaskEvidence on done
    started_at               TEXT,
    evidence_at              TEXT,
    completed_at             TEXT,
    created_at               TEXT NOT NULL,
    updated_at               TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tasks_round_status ON tasks(round_id, status);

-- =========================================================================
-- knowledge — RAG scope split per PRD §5.4. Only scope='rag' is MCP-exposed.
-- =========================================================================
CREATE TABLE IF NOT EXISTS knowledge (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    scope       TEXT NOT NULL DEFAULT 'rag'
                  CHECK (scope IN ('rag','reference','archive')),
    kind        TEXT NOT NULL,                 -- decision | runbook | note | adr ...
    title       TEXT NOT NULL,
    body        TEXT NOT NULL,
    tags        TEXT NOT NULL DEFAULT '[]',    -- JSON array
    source_ref  TEXT,                          -- file:line / URL / commit
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_knowledge_project_scope ON knowledge(project_id, scope);

-- =========================================================================
-- events — SSE event log (PRD §5.4 daemon/SSE). Append-only.
-- =========================================================================
CREATE TABLE IF NOT EXISTS events (
    id          TEXT PRIMARY KEY,
    project_id  TEXT REFERENCES projects(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,                 -- e.g. "task.updated"
    entity_id   TEXT,
    payload     TEXT NOT NULL DEFAULT '{}',    -- JSON
    created_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_project_created ON events(project_id, created_at);

-- =========================================================================
-- FTS5 mirrors (PRD §5.2 keyword search; vector deferred)
-- =========================================================================
CREATE VIRTUAL TABLE IF NOT EXISTS scenarios_fts USING fts5(
    given, when_clause, then_clause, tags,
    content='scenarios', content_rowid='rowid', tokenize='unicode61'
);
CREATE TRIGGER IF NOT EXISTS scenarios_ai AFTER INSERT ON scenarios BEGIN
    INSERT INTO scenarios_fts(rowid, given, when_clause, then_clause, tags)
        VALUES (new.rowid, new.given, new.when_clause, new.then_clause, new.tags);
END;
CREATE TRIGGER IF NOT EXISTS scenarios_ad AFTER DELETE ON scenarios BEGIN
    INSERT INTO scenarios_fts(scenarios_fts, rowid, given, when_clause, then_clause, tags)
        VALUES ('delete', old.rowid, old.given, old.when_clause, old.then_clause, old.tags);
END;
CREATE TRIGGER IF NOT EXISTS scenarios_au AFTER UPDATE ON scenarios BEGIN
    INSERT INTO scenarios_fts(scenarios_fts, rowid, given, when_clause, then_clause, tags)
        VALUES ('delete', old.rowid, old.given, old.when_clause, old.then_clause, old.tags);
    INSERT INTO scenarios_fts(rowid, given, when_clause, then_clause, tags)
        VALUES (new.rowid, new.given, new.when_clause, new.then_clause, new.tags);
END;

CREATE VIRTUAL TABLE IF NOT EXISTS tasks_fts USING fts5(
    description,
    content='tasks', content_rowid='rowid', tokenize='unicode61'
);
CREATE TRIGGER IF NOT EXISTS tasks_ai AFTER INSERT ON tasks BEGIN
    INSERT INTO tasks_fts(rowid, description) VALUES (new.rowid, new.description);
END;
CREATE TRIGGER IF NOT EXISTS tasks_ad AFTER DELETE ON tasks BEGIN
    INSERT INTO tasks_fts(tasks_fts, rowid, description)
        VALUES ('delete', old.rowid, old.description);
END;
CREATE TRIGGER IF NOT EXISTS tasks_au AFTER UPDATE ON tasks BEGIN
    INSERT INTO tasks_fts(tasks_fts, rowid, description)
        VALUES ('delete', old.rowid, old.description);
    INSERT INTO tasks_fts(rowid, description) VALUES (new.rowid, new.description);
END;

CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_fts USING fts5(
    title, body, tags,
    content='knowledge', content_rowid='rowid', tokenize='unicode61'
);
CREATE TRIGGER IF NOT EXISTS knowledge_ai AFTER INSERT ON knowledge BEGIN
    INSERT INTO knowledge_fts(rowid, title, body, tags)
        VALUES (new.rowid, new.title, new.body, new.tags);
END;
CREATE TRIGGER IF NOT EXISTS knowledge_ad AFTER DELETE ON knowledge BEGIN
    INSERT INTO knowledge_fts(knowledge_fts, rowid, title, body, tags)
        VALUES ('delete', old.rowid, old.title, old.body, old.tags);
END;
CREATE TRIGGER IF NOT EXISTS knowledge_au AFTER UPDATE ON knowledge BEGIN
    INSERT INTO knowledge_fts(knowledge_fts, rowid, title, body, tags)
        VALUES ('delete', old.rowid, old.title, old.body, old.tags);
    INSERT INTO knowledge_fts(rowid, title, body, tags)
        VALUES (new.rowid, new.title, new.body, new.tags);
END;

CREATE VIRTUAL TABLE IF NOT EXISTS decisions_fts USING fts5(
    title, body,
    content='decisions', content_rowid='rowid', tokenize='unicode61'
);
CREATE TRIGGER IF NOT EXISTS decisions_ai AFTER INSERT ON decisions BEGIN
    INSERT INTO decisions_fts(rowid, title, body) VALUES (new.rowid, new.title, new.body);
END;
CREATE TRIGGER IF NOT EXISTS decisions_ad AFTER DELETE ON decisions BEGIN
    INSERT INTO decisions_fts(decisions_fts, rowid, title, body)
        VALUES ('delete', old.rowid, old.title, old.body);
END;
CREATE TRIGGER IF NOT EXISTS decisions_au AFTER UPDATE ON decisions BEGIN
    INSERT INTO decisions_fts(decisions_fts, rowid, title, body)
        VALUES ('delete', old.rowid, old.title, old.body);
    INSERT INTO decisions_fts(rowid, title, body) VALUES (new.rowid, new.title, new.body);
END;
