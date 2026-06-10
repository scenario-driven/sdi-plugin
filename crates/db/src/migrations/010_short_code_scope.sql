-- =========================================================================
-- 010 — short_code scope correction (GH issue #2).
--
-- 001 (and 007 for collaboration_patterns) declared `short_code` with a
-- single-column global UNIQUE, contradicting the documented contract that
-- short codes are human ticket labels scoped per plan (per project for
-- plans). The composite pattern already existed in the schema —
-- `rounds(plan_id, round_number)` — but short_code never received it, so a
-- fresh plan could not mint `SC-1` once any other plan owned it.
--
-- SQLite cannot alter constraints, so each affected table is rebuilt
-- (create new → copy → drop → rename) under the documented table-rebuild
-- procedure. The migration runner (schema.rs) disables foreign_keys for the
-- duration of a migration transaction and runs `PRAGMA foreign_key_check`
-- before committing — DROP TABLE on a parent with enforcement on would act
-- as DELETE FROM and cascade-wipe children.
--
-- Existing rows were globally unique, therefore already unique within any
-- narrower scope — the data copy cannot collide.
--
-- tasks have no plan column; their plan is reachable only through rounds.
-- The per-plan ticket contract needs a same-table column for SQLite to
-- enforce it, so tasks gain a denormalized `plan_id` (NOT NULL, FK,
-- backfilled from rounds). `round_id` is immutable after insert (no write
-- path updates it), so the denormalization cannot drift.
--
-- scenarios / tasks / decisions carry external-content FTS5 mirrors keyed by
-- rowid. The copies preserve rowid explicitly, the content-table triggers
-- are recreated on the new tables, and each mirror is rebuilt afterwards.
-- =========================================================================

-- ── plans — UNIQUE (project_id, short_code) ─────────────────────────────
CREATE TABLE plans_new (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    short_code    TEXT NOT NULL,
    title         TEXT NOT NULL,
    body          TEXT NOT NULL DEFAULT '',      -- markdown (SNAPSHOT-ONLY per D12)
    status        TEXT NOT NULL CHECK (status IN ('draft','active','completed')),
    version       INTEGER NOT NULL DEFAULT 0,
    approved_at   TEXT,
    completed_at  TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    produced_via_pattern_id TEXT REFERENCES collaboration_patterns(id),
    UNIQUE (project_id, short_code)
);
INSERT INTO plans_new (id, project_id, short_code, title, body, status, version,
                       approved_at, completed_at, created_at, updated_at,
                       produced_via_pattern_id)
SELECT id, project_id, short_code, title, body, status, version,
       approved_at, completed_at, created_at, updated_at,
       produced_via_pattern_id
FROM plans;
DROP TABLE plans;
ALTER TABLE plans_new RENAME TO plans;
CREATE INDEX idx_plans_project_status ON plans(project_id, status);
CREATE UNIQUE INDEX uniq_one_active_plan_per_project
    ON plans(project_id) WHERE status = 'active';
CREATE INDEX idx_plans_pattern ON plans(produced_via_pattern_id)
    WHERE produced_via_pattern_id IS NOT NULL;

-- ── requirements — UNIQUE (plan_id, short_code) ─────────────────────────
CREATE TABLE requirements_new (
    id           TEXT NOT NULL PRIMARY KEY,
    plan_id      TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    short_code   TEXT NOT NULL,
    title        TEXT NOT NULL,
    body         TEXT NOT NULL,
    source       TEXT NOT NULL DEFAULT 'snapshot',
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    produced_via_pattern_id TEXT REFERENCES collaboration_patterns(id),
    UNIQUE (plan_id, short_code)
);
INSERT INTO requirements_new (id, plan_id, short_code, title, body, source,
                              created_at, updated_at, produced_via_pattern_id)
SELECT id, plan_id, short_code, title, body, source,
       created_at, updated_at, produced_via_pattern_id
FROM requirements;
DROP TABLE requirements;
ALTER TABLE requirements_new RENAME TO requirements;
CREATE INDEX idx_requirements_plan ON requirements(plan_id);
CREATE INDEX idx_requirements_pattern ON requirements(produced_via_pattern_id)
    WHERE produced_via_pattern_id IS NOT NULL;

-- ── decisions — UNIQUE (plan_id, short_code); FTS mirror rebuilt ────────
CREATE TABLE decisions_new (
    id              TEXT PRIMARY KEY,
    plan_id         TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    short_code      TEXT NOT NULL,
    title           TEXT NOT NULL,
    body            TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'accepted'
                      CHECK (status IN ('proposed','accepted','superseded')),
    supersedes_id   TEXT REFERENCES decisions(id) ON DELETE SET NULL,
    created_at      TEXT NOT NULL,
    kind            TEXT NOT NULL DEFAULT 'consensus',
    proposal_id     TEXT REFERENCES decisions(id) ON DELETE SET NULL,
    agent_name      TEXT,
    escalated_at    TEXT,
    produced_via_pattern_id TEXT REFERENCES collaboration_patterns(id),
    reversal_plan       TEXT,
    blast_radius_score  INTEGER NOT NULL DEFAULT 5,
    reversal_of         TEXT REFERENCES decisions(id),
    UNIQUE (plan_id, short_code)
);
INSERT INTO decisions_new (rowid, id, plan_id, short_code, title, body, status,
                           supersedes_id, created_at, kind, proposal_id,
                           agent_name, escalated_at, produced_via_pattern_id,
                           reversal_plan, blast_radius_score, reversal_of)
SELECT rowid, id, plan_id, short_code, title, body, status,
       supersedes_id, created_at, kind, proposal_id,
       agent_name, escalated_at, produced_via_pattern_id,
       reversal_plan, blast_radius_score, reversal_of
FROM decisions;
DROP TABLE decisions;
ALTER TABLE decisions_new RENAME TO decisions;
CREATE INDEX idx_decisions_plan ON decisions(plan_id);
CREATE INDEX idx_decisions_kind ON decisions(kind);
CREATE INDEX idx_decisions_proposal
    ON decisions(proposal_id) WHERE proposal_id IS NOT NULL;
CREATE INDEX idx_decisions_pattern ON decisions(produced_via_pattern_id)
    WHERE produced_via_pattern_id IS NOT NULL;
CREATE INDEX idx_decisions_reversal_of
    ON decisions(reversal_of) WHERE reversal_of IS NOT NULL;
CREATE TRIGGER decisions_ai AFTER INSERT ON decisions BEGIN
    INSERT INTO decisions_fts(rowid, title, body) VALUES (new.rowid, new.title, new.body);
END;
CREATE TRIGGER decisions_ad AFTER DELETE ON decisions BEGIN
    INSERT INTO decisions_fts(decisions_fts, rowid, title, body)
        VALUES ('delete', old.rowid, old.title, old.body);
END;
CREATE TRIGGER decisions_au AFTER UPDATE ON decisions BEGIN
    INSERT INTO decisions_fts(decisions_fts, rowid, title, body)
        VALUES ('delete', old.rowid, old.title, old.body);
    INSERT INTO decisions_fts(rowid, title, body) VALUES (new.rowid, new.title, new.body);
END;
INSERT INTO decisions_fts(decisions_fts) VALUES ('rebuild');

-- ── scenarios — UNIQUE (plan_id, short_code); FTS mirror rebuilt ────────
CREATE TABLE scenarios_new (
    id               TEXT PRIMARY KEY,
    plan_id          TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    short_code       TEXT NOT NULL,
    given            TEXT NOT NULL,
    when_clause      TEXT NOT NULL,
    then_clause      TEXT NOT NULL,
    tags             TEXT NOT NULL DEFAULT '[]', -- JSON array
    origin_round_id  TEXT REFERENCES rounds(id) ON DELETE SET NULL,
    status           TEXT NOT NULL DEFAULT 'draft'
                       CHECK (status IN ('draft','confirmed')),
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    depends_on       TEXT NOT NULL DEFAULT '[]',
    produced_by      TEXT,
    verified_by      TEXT,
    produced_via_pattern_id TEXT REFERENCES collaboration_patterns(id),
    claimed_resources_json  TEXT NOT NULL DEFAULT '[]',
    claim_status            TEXT NOT NULL DEFAULT 'none',
    UNIQUE (plan_id, short_code),
    CHECK (length(trim(given)) > 0),
    CHECK (length(trim(when_clause)) > 0),
    CHECK (length(trim(then_clause)) > 0)
);
INSERT INTO scenarios_new (rowid, id, plan_id, short_code, given, when_clause,
                           then_clause, tags, origin_round_id, status,
                           created_at, updated_at, depends_on, produced_by,
                           verified_by, produced_via_pattern_id,
                           claimed_resources_json, claim_status)
SELECT rowid, id, plan_id, short_code, given, when_clause,
       then_clause, tags, origin_round_id, status,
       created_at, updated_at, depends_on, produced_by,
       verified_by, produced_via_pattern_id,
       claimed_resources_json, claim_status
FROM scenarios;
DROP TABLE scenarios;
ALTER TABLE scenarios_new RENAME TO scenarios;
CREATE INDEX idx_scenarios_plan ON scenarios(plan_id);
CREATE INDEX idx_scenarios_pattern ON scenarios(produced_via_pattern_id)
    WHERE produced_via_pattern_id IS NOT NULL;
CREATE INDEX idx_scenarios_claim
    ON scenarios(claim_status) WHERE claim_status != 'none';
CREATE TRIGGER scenarios_ai AFTER INSERT ON scenarios BEGIN
    INSERT INTO scenarios_fts(rowid, given, when_clause, then_clause, tags)
        VALUES (new.rowid, new.given, new.when_clause, new.then_clause, new.tags);
END;
CREATE TRIGGER scenarios_ad AFTER DELETE ON scenarios BEGIN
    INSERT INTO scenarios_fts(scenarios_fts, rowid, given, when_clause, then_clause, tags)
        VALUES ('delete', old.rowid, old.given, old.when_clause, old.then_clause, old.tags);
END;
CREATE TRIGGER scenarios_au AFTER UPDATE ON scenarios BEGIN
    INSERT INTO scenarios_fts(scenarios_fts, rowid, given, when_clause, then_clause, tags)
        VALUES ('delete', old.rowid, old.given, old.when_clause, old.then_clause, old.tags);
    INSERT INTO scenarios_fts(rowid, given, when_clause, then_clause, tags)
        VALUES (new.rowid, new.given, new.when_clause, new.then_clause, new.tags);
END;
INSERT INTO scenarios_fts(scenarios_fts) VALUES ('rebuild');

-- ── rounds — UNIQUE (plan_id, short_code); (plan_id, round_number) kept ─
CREATE TABLE rounds_new (
    id                  TEXT PRIMARY KEY,
    plan_id             TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    short_code          TEXT NOT NULL,
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
    produced_via_pattern_id TEXT REFERENCES collaboration_patterns(id),
    UNIQUE (plan_id, round_number),
    UNIQUE (plan_id, short_code)
);
INSERT INTO rounds_new (id, plan_id, short_code, round_number, mode,
                        in_flight_policy, disruption_policy, status,
                        activated_at, completed_at, created_at, updated_at,
                        produced_via_pattern_id)
SELECT id, plan_id, short_code, round_number, mode,
       in_flight_policy, disruption_policy, status,
       activated_at, completed_at, created_at, updated_at,
       produced_via_pattern_id
FROM rounds;
DROP TABLE rounds;
ALTER TABLE rounds_new RENAME TO rounds;
CREATE INDEX idx_rounds_plan_status ON rounds(plan_id, status);
CREATE UNIQUE INDEX uniq_one_active_round_per_plan
    ON rounds(plan_id) WHERE status = 'active';
CREATE INDEX idx_rounds_pattern ON rounds(produced_via_pattern_id)
    WHERE produced_via_pattern_id IS NOT NULL;

-- ── tasks — gains plan_id; UNIQUE (plan_id, short_code); FTS rebuilt ────
CREATE TABLE tasks_new (
    id                       TEXT PRIMARY KEY,
    round_id                 TEXT NOT NULL REFERENCES rounds(id) ON DELETE CASCADE,
    plan_id                  TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    short_code               TEXT NOT NULL,
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
    updated_at               TEXT NOT NULL,
    produced_via_pattern_id  TEXT REFERENCES collaboration_patterns(id),
    UNIQUE (plan_id, short_code)
);
INSERT INTO tasks_new (rowid, id, round_id, plan_id, short_code, description,
                       status, parent_scenario_ids, parent_requirement_ids,
                       evidence, started_at, evidence_at, completed_at,
                       created_at, updated_at, produced_via_pattern_id)
SELECT t.rowid, t.id, t.round_id, r.plan_id, t.short_code, t.description,
       t.status, t.parent_scenario_ids, t.parent_requirement_ids,
       t.evidence, t.started_at, t.evidence_at, t.completed_at,
       t.created_at, t.updated_at, t.produced_via_pattern_id
FROM tasks t
JOIN rounds r ON r.id = t.round_id;
DROP TABLE tasks;
ALTER TABLE tasks_new RENAME TO tasks;
CREATE INDEX idx_tasks_round_status ON tasks(round_id, status);
CREATE INDEX idx_tasks_pattern ON tasks(produced_via_pattern_id)
    WHERE produced_via_pattern_id IS NOT NULL;
CREATE TRIGGER tasks_ai AFTER INSERT ON tasks BEGIN
    INSERT INTO tasks_fts(rowid, description) VALUES (new.rowid, new.description);
END;
CREATE TRIGGER tasks_ad AFTER DELETE ON tasks BEGIN
    INSERT INTO tasks_fts(tasks_fts, rowid, description)
        VALUES ('delete', old.rowid, old.description);
END;
CREATE TRIGGER tasks_au AFTER UPDATE ON tasks BEGIN
    INSERT INTO tasks_fts(tasks_fts, rowid, description)
        VALUES ('delete', old.rowid, old.description);
    INSERT INTO tasks_fts(rowid, description) VALUES (new.rowid, new.description);
END;
INSERT INTO tasks_fts(tasks_fts) VALUES ('rebuild');

-- ── collaboration_patterns — UNIQUE (plan_id, short_code) ───────────────
-- The 009 sentinel codes (`DIRECT-<plan short_code>`) are minted per plan,
-- so they remain unique within the narrowed scope by construction.
CREATE TABLE collaboration_patterns_new (
    id                      TEXT PRIMARY KEY,
    short_code              TEXT NOT NULL,
    plan_id                 TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    kind                    TEXT NOT NULL
                              CHECK (kind IN ('workflow','graph','swarm','agents-as-tools','direct')),
    applies_to              TEXT NOT NULL
                              CHECK (applies_to IN ('plan','requirement','scenario','task','decision','round')),
    scope_id                TEXT NOT NULL,
    parent_pattern_id       TEXT REFERENCES collaboration_patterns(id) ON DELETE SET NULL,
    depth                   INTEGER NOT NULL DEFAULT 0 CHECK (depth >= 0),
    lifecycle               TEXT NOT NULL DEFAULT 'pending'
                              CHECK (lifecycle IN ('pending','active','converged','dissensus','aborted')),
    steps_json              TEXT,
    reviewers_json          TEXT,
    fan_out_json            TEXT,
    peer_registration_json  TEXT,
    decided_at              TEXT,
    decided_reason          TEXT,
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL,
    UNIQUE (plan_id, short_code)
);
INSERT INTO collaboration_patterns_new (id, short_code, plan_id, kind, applies_to,
                                        scope_id, parent_pattern_id, depth,
                                        lifecycle, steps_json, reviewers_json,
                                        fan_out_json, peer_registration_json,
                                        decided_at, decided_reason,
                                        created_at, updated_at)
SELECT id, short_code, plan_id, kind, applies_to,
       scope_id, parent_pattern_id, depth,
       lifecycle, steps_json, reviewers_json,
       fan_out_json, peer_registration_json,
       decided_at, decided_reason,
       created_at, updated_at
FROM collaboration_patterns;
DROP TABLE collaboration_patterns;
ALTER TABLE collaboration_patterns_new RENAME TO collaboration_patterns;
CREATE INDEX idx_collab_pat_plan       ON collaboration_patterns (plan_id, created_at);
CREATE INDEX idx_collab_pat_kind       ON collaboration_patterns (kind, lifecycle);
CREATE INDEX idx_collab_pat_scope      ON collaboration_patterns (applies_to, scope_id);
CREATE INDEX idx_collab_pat_parent     ON collaboration_patterns (parent_pattern_id) WHERE parent_pattern_id IS NOT NULL;
CREATE INDEX idx_collab_pat_lifecycle  ON collaboration_patterns (lifecycle, updated_at);
