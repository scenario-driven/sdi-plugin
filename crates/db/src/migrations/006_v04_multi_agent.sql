-- SDI v0.4 multi-agent governance schema.
-- New first-class entities (D14 AutonomyPolicy, M1 AgentNote, Layer-2 AgentSpec)
-- plus M3/M4 contract columns on scenarios + decisions.
--
-- Idempotent (CREATE TABLE IF NOT EXISTS + per-column existence guard via
-- application-side check for ALTER cases; SQLite has no IF NOT EXISTS on
-- ADD COLUMN, so the runner detects "duplicate column name" and continues).

-- =========================================================================
-- autonomy_policies (D14 sixth first-class entity; D17 forced L4 kinds)
-- Per-scope autonomy mode. `scope_kind` ∈ {plan, decision_kind, global}.
-- `decision_kind` is non-null when scope_kind = 'decision_kind' (the gate
-- targets a specific Decision.kind such as 'architecture' / 'schema' /
-- 'naming-canonical' that D17 forces to L4).
-- =========================================================================
CREATE TABLE IF NOT EXISTS autonomy_policies (
    id              TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    plan_id         TEXT REFERENCES plans(id) ON DELETE CASCADE,
    scope_kind      TEXT NOT NULL
                      CHECK (scope_kind IN ('plan','decision_kind','global')),
    decision_kind   TEXT,                                 -- non-null iff scope_kind='decision_kind'
    mode            TEXT NOT NULL DEFAULT 'L5'
                      CHECK (mode IN ('L3','L4','L5')),
    set_at          TEXT NOT NULL,
    set_by          TEXT NOT NULL DEFAULT 'agent',       -- 'agent' | 'human' | actor name
    reason          TEXT,                                 -- free-form: circuit-breaker / user-edit / default
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    CHECK (
        (scope_kind = 'plan'          AND plan_id IS NOT NULL AND decision_kind IS NULL)
     OR (scope_kind = 'decision_kind' AND decision_kind IS NOT NULL)
     OR (scope_kind = 'global'        AND plan_id IS NULL AND decision_kind IS NULL)
    )
);
CREATE INDEX IF NOT EXISTS idx_autonomy_project ON autonomy_policies(project_id, scope_kind);
CREATE INDEX IF NOT EXISTS idx_autonomy_plan    ON autonomy_policies(plan_id) WHERE plan_id IS NOT NULL;

-- Only one row per (project, scope_kind, plan_id, decision_kind) tuple.
-- COALESCE flattens NULL distinctness so we get the natural uniqueness.
CREATE UNIQUE INDEX IF NOT EXISTS uniq_autonomy_scope
    ON autonomy_policies(
        project_id,
        scope_kind,
        COALESCE(plan_id, ''),
        COALESCE(decision_kind, '')
    );

-- =========================================================================
-- agent_notes (M1 Blackboard, append-only journal).
-- `kind` ∈ {hypothesis, observation, question, dissent, evidence, handoff}.
-- `scope_kind` ∈ {plan, round, scenario, task} anchors the note; exactly one
-- of (plan_id / round_id / scenario_id / task_id) is non-null and must match.
-- Retirement is non-destructive: set `retired_at` instead of DELETE.
-- =========================================================================
CREATE TABLE IF NOT EXISTS agent_notes (
    id                          TEXT PRIMARY KEY,
    project_id                  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    plan_id                     TEXT REFERENCES plans(id)     ON DELETE CASCADE,
    round_id                    TEXT REFERENCES rounds(id)    ON DELETE CASCADE,
    scenario_id                 TEXT REFERENCES scenarios(id) ON DELETE CASCADE,
    task_id                     TEXT REFERENCES tasks(id)     ON DELETE CASCADE,
    scope_kind                  TEXT NOT NULL
                                  CHECK (scope_kind IN ('plan','round','scenario','task')),
    kind                        TEXT NOT NULL
                                  CHECK (kind IN ('hypothesis','observation','question',
                                                  'dissent','evidence','handoff')),
    from_agent                  TEXT NOT NULL,
    to_agent                    TEXT,                              -- non-null required for kind='handoff'
    body                        TEXT NOT NULL,                     -- markdown
    payload                     TEXT NOT NULL DEFAULT '{}',        -- JSON sidecar
    receipt_acknowledged_at     TEXT,                              -- ack time for handoff
    retired_at                  TEXT,                              -- non-destructive retirement
    retired_reason              TEXT,
    created_at                  TEXT NOT NULL,
    CHECK (
        (scope_kind = 'plan'     AND plan_id     IS NOT NULL AND round_id IS NULL AND scenario_id IS NULL AND task_id IS NULL)
     OR (scope_kind = 'round'    AND round_id    IS NOT NULL AND plan_id IS NULL AND scenario_id IS NULL AND task_id IS NULL)
     OR (scope_kind = 'scenario' AND scenario_id IS NOT NULL AND plan_id IS NULL AND round_id    IS NULL AND task_id IS NULL)
     OR (scope_kind = 'task'     AND task_id     IS NOT NULL AND plan_id IS NULL AND round_id    IS NULL AND scenario_id IS NULL)
    ),
    CHECK ((kind <> 'handoff') OR (to_agent IS NOT NULL))
);
CREATE INDEX IF NOT EXISTS idx_agent_notes_project_created
    ON agent_notes(project_id, created_at);
-- M2 Hand-off pending receipts: agent + unacknowledged handoff lookups
CREATE INDEX IF NOT EXISTS agent_note_handoff_idx
    ON agent_notes(to_agent, receipt_acknowledged_at)
    WHERE kind = 'handoff';
-- M1 Blackboard active reads: scope-bound non-retired notes
CREATE INDEX IF NOT EXISTS agent_note_active_idx
    ON agent_notes(scope_kind, plan_id, round_id, scenario_id, task_id, kind, created_at)
    WHERE retired_at IS NULL;

-- =========================================================================
-- agent_specs (Layer 2 specialist sub-agents enumerated; 8 seeded by app).
-- Eight stock specs: gwt-converter, scenario-decomposer, impl-coder,
-- test-runner, regression-runner, disruption-analyst, decision-resolver,
-- schema-architect (D-7).
-- M5 self-organization changes `instance_count`, never adds new roles.
-- =========================================================================
CREATE TABLE IF NOT EXISTS agent_specs (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,             -- e.g. 'gwt-converter'
    role            TEXT NOT NULL,                    -- short role label
    system_prompt   TEXT NOT NULL DEFAULT '',         -- canonical body
    instance_count  INTEGER NOT NULL DEFAULT 1
                      CHECK (instance_count BETWEEN 0 AND 16),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_specs_role ON agent_specs(role);

-- =========================================================================
-- M4 scenario-as-contract columns on scenarios.
-- depends_on:  JSON array of scenario short_codes (DAG predecessor edges)
-- produced_by: agent name responsible for satisfying this scenario
-- verified_by: agent name responsible for verifying it (often distinct)
-- ALTER ADD COLUMN cannot carry CHECK; format enforced in domain layer.
-- =========================================================================
ALTER TABLE scenarios ADD COLUMN depends_on  TEXT NOT NULL DEFAULT '[]';
ALTER TABLE scenarios ADD COLUMN produced_by TEXT;
ALTER TABLE scenarios ADD COLUMN verified_by TEXT;

-- =========================================================================
-- D20 Decision negotiation stage on decisions.
-- kind ∈ {proposal, critique, consensus, dissensus}.
-- M3 enforces ordering: every consensus must be preceded by ≥ 1 critique
-- against the same proposal_id (linked via supersedes_id chain).
-- CHECK cannot be added through ALTER; domain layer validates the enum.
-- =========================================================================
ALTER TABLE decisions ADD COLUMN kind     TEXT NOT NULL DEFAULT 'consensus';
ALTER TABLE decisions ADD COLUMN proposal_id  TEXT REFERENCES decisions(id) ON DELETE SET NULL;
ALTER TABLE decisions ADD COLUMN agent_name   TEXT;     -- which agent emitted this stage
ALTER TABLE decisions ADD COLUMN escalated_at TEXT;     -- non-null when dissensus surfaces to human

CREATE INDEX IF NOT EXISTS idx_decisions_kind ON decisions(kind);
CREATE INDEX IF NOT EXISTS idx_decisions_proposal
    ON decisions(proposal_id) WHERE proposal_id IS NOT NULL;
