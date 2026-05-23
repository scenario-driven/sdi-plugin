-- SDI v0.5 pattern enforcement schema (D22–D29).
--
-- New first-class entity: CollaborationPattern (D22, 7th first-class entity).
-- Six work entities (plans/requirements/scenarios/tasks/decisions/rounds) gain
-- `produced_via_pattern_id` (D23 — `direct` is the explicit solo-flow marker).
-- Scenario gains multi-session resource claims (D29 — claimed_resources_json,
-- claim_status). Decision gains reversibility (D28 — reversal_plan,
-- blast_radius_score, reversal_of). AutonomyPolicy gains pattern_kind scope
-- plus l5_threshold / pattern_depth_cap / plan_single_session_lock /
-- external_surface / timeout_ms / forced (D24, D25, D28, D29). AgentSpec gains
-- stance (D26 sybil-fix) and decision-kind / lifecycle fields needed by D26
-- consensus gates and D28 reversal-runner.
--
-- Enum CHECKs that cannot be carried by ALTER ADD COLUMN are enforced in the
-- domain layer (see `crates/core/src/pattern.rs` and the existing AgentSpec /
-- AutonomyPolicy validators). Tables that need an extended CHECK on existing
-- columns are rebuilt under the SQLite table-rebuild pattern; the runner wraps
-- the whole file in a transaction (schema.rs), so a partial failure leaves the
-- pre-007 schema intact.

-- =========================================================================
-- collaboration_patterns (D22 — 7th first-class entity).
-- Lifecycle: pending → active → converged | dissensus | aborted.
-- `kind` ∈ {workflow, graph, swarm, agents-as-tools, direct}.
-- `applies_to` selects which work entity this pattern produces.
-- `parent_pattern_id` self-FK forms the DAG (D24, cycle blocked by daemon).
-- `depth` is materialized by the daemon (parent NULL → 0, else parent.depth+1)
-- so it can be capped via AutonomyPolicy.pattern_depth_cap without recursive
-- queries on hot paths.
-- Shape-specific manifests live in the *_json columns and are validated by
-- the domain layer at lifecycle transitions (D26).
-- =========================================================================
CREATE TABLE IF NOT EXISTS collaboration_patterns (
    id                      TEXT PRIMARY KEY,
    short_code              TEXT NOT NULL UNIQUE,
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
    updated_at              TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_collab_pat_plan       ON collaboration_patterns (plan_id, created_at);
CREATE INDEX IF NOT EXISTS idx_collab_pat_kind       ON collaboration_patterns (kind, lifecycle);
CREATE INDEX IF NOT EXISTS idx_collab_pat_scope      ON collaboration_patterns (applies_to, scope_id);
CREATE INDEX IF NOT EXISTS idx_collab_pat_parent     ON collaboration_patterns (parent_pattern_id) WHERE parent_pattern_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_collab_pat_lifecycle  ON collaboration_patterns (lifecycle, updated_at);

-- =========================================================================
-- D23 produced_via_pattern_id on all six work entities. NULL FK so legacy rows
-- migrate cleanly; the daemon back-fills a `direct` pattern row at the next
-- write of the entity. CHECK on non-null happens at the daemon write path
-- (ALTER ADD COLUMN cannot carry CHECK on existing tables).
-- =========================================================================
ALTER TABLE plans         ADD COLUMN produced_via_pattern_id TEXT REFERENCES collaboration_patterns(id);
ALTER TABLE requirements  ADD COLUMN produced_via_pattern_id TEXT REFERENCES collaboration_patterns(id);
ALTER TABLE scenarios     ADD COLUMN produced_via_pattern_id TEXT REFERENCES collaboration_patterns(id);
ALTER TABLE tasks         ADD COLUMN produced_via_pattern_id TEXT REFERENCES collaboration_patterns(id);
ALTER TABLE decisions     ADD COLUMN produced_via_pattern_id TEXT REFERENCES collaboration_patterns(id);
ALTER TABLE rounds        ADD COLUMN produced_via_pattern_id TEXT REFERENCES collaboration_patterns(id);

CREATE INDEX IF NOT EXISTS idx_plans_pattern        ON plans(produced_via_pattern_id)        WHERE produced_via_pattern_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_requirements_pattern ON requirements(produced_via_pattern_id) WHERE produced_via_pattern_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_scenarios_pattern    ON scenarios(produced_via_pattern_id)    WHERE produced_via_pattern_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_tasks_pattern        ON tasks(produced_via_pattern_id)        WHERE produced_via_pattern_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_decisions_pattern    ON decisions(produced_via_pattern_id)    WHERE produced_via_pattern_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_rounds_pattern       ON rounds(produced_via_pattern_id)       WHERE produced_via_pattern_id IS NOT NULL;

-- =========================================================================
-- D29 scenarios.claimed_resources_json + claim_status. JSON array of path
-- globs; status enum (none|requested|active|released) validated in the domain
-- layer. The partial index targets the hot daemon query for active overlap
-- detection on Edit/Write PreToolUse hooks.
-- =========================================================================
ALTER TABLE scenarios ADD COLUMN claimed_resources_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE scenarios ADD COLUMN claim_status           TEXT NOT NULL DEFAULT 'none';

CREATE INDEX IF NOT EXISTS idx_scenarios_claim
    ON scenarios(claim_status) WHERE claim_status != 'none';

-- =========================================================================
-- D28 decisions.reversal_plan + blast_radius_score + reversal_of. The plan is
-- JSON when present; format / per-type field validation lives in the domain
-- layer (see `validate_reversal_plan_json`). `blast_radius_score` default 5
-- means "untagged decisions are mid-radius" — gating logic stays explicit.
-- =========================================================================
ALTER TABLE decisions ADD COLUMN reversal_plan       TEXT;
ALTER TABLE decisions ADD COLUMN blast_radius_score  INTEGER NOT NULL DEFAULT 5;
ALTER TABLE decisions ADD COLUMN reversal_of         TEXT REFERENCES decisions(id);

CREATE INDEX IF NOT EXISTS idx_decisions_reversal_of
    ON decisions(reversal_of) WHERE reversal_of IS NOT NULL;

-- =========================================================================
-- D24 + D25 + D28 + D29 autonomy_policies rebuild. The scope_kind CHECK gains
-- 'pattern_kind' and the new columns carry pattern-scoped autonomy + L5 gate
-- thresholds + multi-session lock + external-surface marker + circuit-breaker
-- timeout + forced-mode flag. Uniqueness widens to include pattern_kind.
-- Rebuilt rather than ALTERed because CHECK constraints and the uniqueness
-- tuple can only be extended via table rebuild in SQLite.
-- =========================================================================
CREATE TABLE autonomy_policies_new (
    id                          TEXT PRIMARY KEY,
    project_id                  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    plan_id                     TEXT REFERENCES plans(id) ON DELETE CASCADE,
    scope_kind                  TEXT NOT NULL
                                  CHECK (scope_kind IN ('plan','decision_kind','pattern_kind','global')),
    decision_kind               TEXT,
    pattern_kind                TEXT
                                  CHECK (pattern_kind IS NULL OR pattern_kind IN
                                         ('workflow','graph','swarm','agents-as-tools','direct')),
    mode                        TEXT NOT NULL DEFAULT 'L5'
                                  CHECK (mode IN ('L3','L4','L5')),
    l5_threshold                INTEGER NOT NULL DEFAULT 5
                                  CHECK (l5_threshold BETWEEN 0 AND 10),
    pattern_depth_cap           INTEGER NOT NULL DEFAULT 3
                                  CHECK (pattern_depth_cap BETWEEN 1 AND 10),
    plan_single_session_lock    INTEGER NOT NULL DEFAULT 0
                                  CHECK (plan_single_session_lock IN (0,1)),
    external_surface            INTEGER NOT NULL DEFAULT 0
                                  CHECK (external_surface IN (0,1)),
    timeout_ms                  INTEGER,
    forced                      INTEGER NOT NULL DEFAULT 0
                                  CHECK (forced IN (0,1)),
    set_at                      TEXT NOT NULL,
    set_by                      TEXT NOT NULL DEFAULT 'agent',
    reason                      TEXT,
    created_at                  TEXT NOT NULL,
    updated_at                  TEXT NOT NULL,
    CHECK (
        (scope_kind = 'plan'          AND plan_id IS NOT NULL AND decision_kind IS NULL    AND pattern_kind IS NULL)
     OR (scope_kind = 'decision_kind' AND decision_kind IS NOT NULL                       AND pattern_kind IS NULL)
     OR (scope_kind = 'pattern_kind'  AND pattern_kind IS NOT NULL AND plan_id IS NULL    AND decision_kind IS NULL)
     OR (scope_kind = 'global'        AND plan_id IS NULL AND decision_kind IS NULL       AND pattern_kind IS NULL)
    )
);

INSERT INTO autonomy_policies_new (
    id, project_id, plan_id, scope_kind, decision_kind, pattern_kind, mode,
    l5_threshold, pattern_depth_cap, plan_single_session_lock,
    external_surface, timeout_ms, forced,
    set_at, set_by, reason, created_at, updated_at
)
SELECT
    id, project_id, plan_id, scope_kind, decision_kind, NULL, mode,
    5, 3, 0,
    0, NULL, 0,
    set_at, set_by, reason, created_at, updated_at
FROM autonomy_policies;

DROP INDEX IF EXISTS uniq_autonomy_scope;
DROP INDEX IF EXISTS idx_autonomy_plan;
DROP INDEX IF EXISTS idx_autonomy_project;
DROP TABLE autonomy_policies;
ALTER TABLE autonomy_policies_new RENAME TO autonomy_policies;

CREATE INDEX idx_autonomy_project ON autonomy_policies(project_id, scope_kind);
CREATE INDEX idx_autonomy_plan    ON autonomy_policies(plan_id)      WHERE plan_id IS NOT NULL;
CREATE INDEX idx_autonomy_pattern ON autonomy_policies(pattern_kind) WHERE pattern_kind IS NOT NULL;
CREATE UNIQUE INDEX uniq_autonomy_scope ON autonomy_policies(
    project_id,
    scope_kind,
    COALESCE(plan_id, ''),
    COALESCE(decision_kind, ''),
    COALESCE(pattern_kind, '')
);

-- =========================================================================
-- D26 + D28 agent_specs rebuild. The single-column UNIQUE(name) is replaced
-- by UNIQUE(name, stance) so the same role can have multiple stance instances
-- (impl-coder/proposer + impl-coder/devil_advocate) — distinctness for graph
-- consensus is the (name, stance) tuple, not the name alone (sybil fix). New
-- columns cover the D28 reversal-runner specialist + lifecycle/expiry needed
-- by D26 active-consensus gates.
-- =========================================================================
CREATE TABLE agent_specs_new (
    id                       TEXT PRIMARY KEY,
    name                     TEXT NOT NULL,
    stance                   TEXT NOT NULL DEFAULT 'neutral'
                               CHECK (stance IN ('proposer','devil_advocate','schema_guardian',
                                                 'performance_reviewer','security_reviewer','neutral')),
    role                     TEXT NOT NULL,
    system_prompt            TEXT NOT NULL DEFAULT '',
    instance_count           INTEGER NOT NULL DEFAULT 1
                               CHECK (instance_count BETWEEN 0 AND 16),
    created_by               TEXT,
    origin_plan_id           TEXT REFERENCES plans(id) ON DELETE SET NULL,
    tool_allowlist_json      TEXT,
    decision_kinds_json      TEXT,
    blast_radius_rules_json  TEXT,
    status                   TEXT NOT NULL DEFAULT 'active'
                               CHECK (status IN ('active','expired')),
    expires_at               TEXT,
    created_at               TEXT NOT NULL,
    updated_at               TEXT NOT NULL,
    UNIQUE (name, stance)
);

INSERT INTO agent_specs_new (
    id, name, stance, role, system_prompt, instance_count,
    created_by, origin_plan_id, tool_allowlist_json, decision_kinds_json,
    blast_radius_rules_json, status, expires_at, created_at, updated_at
)
SELECT
    id, name, 'neutral', role, system_prompt, instance_count,
    NULL, NULL, NULL, NULL,
    NULL, 'active', NULL, created_at, updated_at
FROM agent_specs;

DROP INDEX IF EXISTS idx_agent_specs_role;
DROP TABLE agent_specs;
ALTER TABLE agent_specs_new RENAME TO agent_specs;

CREATE INDEX idx_agent_specs_role   ON agent_specs(role);
CREATE INDEX idx_agent_specs_stance ON agent_specs(stance, status);
CREATE INDEX idx_agent_specs_status ON agent_specs(status, expires_at);
