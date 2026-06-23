-- SDI 2.0 oracle layer (PRD-v2 D32 / D33 / D35).
-- Adds the product-definition graph (L0), user-flow tier (L1), the scenario→flow
-- anchor (L2), and the decision-question engine. Idempotent (CREATE … IF NOT EXISTS).
-- New work entities carry produced_via_pattern_id (D23). dangling/openness are
-- representable so the deterministic verify (D34) can judge completeness.

-- =========================================================================
-- L0: SSoT product-definition graph (D32). `kind` covers Persona / Capability /
-- Domain / Concept / Invariant / … — one uniform node table (ssot-studio model).
-- =========================================================================
CREATE TABLE IF NOT EXISTS ssot_nodes (
    id                      TEXT PRIMARY KEY,
    project_id              TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    short_code              TEXT NOT NULL UNIQUE,
    kind                    TEXT NOT NULL,                 -- Persona | Capability | Domain | Concept | Invariant | ...
    title                   TEXT NOT NULL,
    facets_json             TEXT NOT NULL DEFAULT '{}',    -- 4-axis: business / domain / system / governance
    open_markers_json       TEXT NOT NULL DEFAULT '[]',    -- [{id, field, description, question_id?}] — unresolved = facet-incomplete
    confidence              TEXT NOT NULL DEFAULT 'unverified'
                              CHECK (confidence IN ('unverified','inferred','high')),
    produced_via_pattern_id TEXT REFERENCES collaboration_patterns(id) ON DELETE SET NULL,  -- D23
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ssot_nodes_project_kind ON ssot_nodes(project_id, kind);

-- L0 edges (D32 link completeness). `to_ref` is a logical reference (node id or
-- short_code) NOT a hard FK, so an unresolved (dangling) edge is representable and
-- the verify pass can flag it. `from_node` always exists at author time → FK.
CREATE TABLE IF NOT EXISTS ssot_edges (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    from_node   TEXT NOT NULL REFERENCES ssot_nodes(id) ON DELETE CASCADE,
    to_ref      TEXT NOT NULL,                  -- target node id or short_code; resolved by verify
    rel         TEXT NOT NULL,                  -- servesPersona | relatesTo | dependsOn | ...
    created_at  TEXT NOT NULL,
    UNIQUE (from_node, to_ref, rel)
);
CREATE INDEX IF NOT EXISTS idx_ssot_edges_project ON ssot_edges(project_id);
CREATE INDEX IF NOT EXISTS idx_ssot_edges_from ON ssot_edges(from_node);

-- =========================================================================
-- L1: UserFlow (D33) — one persona × one purpose, the complete service journey.
-- This is the reference the outer (spec-convergence) loop drives toward.
-- =========================================================================
CREATE TABLE IF NOT EXISTS user_flows (
    id                       TEXT PRIMARY KEY,
    project_id               TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    short_code               TEXT NOT NULL UNIQUE,
    persona_id               TEXT NOT NULL REFERENCES ssot_nodes(id) ON DELETE CASCADE,
    purpose                  TEXT NOT NULL,
    steps_json               TEXT NOT NULL DEFAULT '[]',   -- ordered journey steps (finished-service basis)
    covers_capabilities_json TEXT NOT NULL DEFAULT '[]',   -- ssot_node ids (kind=Capability)
    status                   TEXT NOT NULL DEFAULT 'draft'
                               CHECK (status IN ('draft','confirmed')),
    produced_via_pattern_id  TEXT REFERENCES collaboration_patterns(id) ON DELETE SET NULL,
    created_at               TEXT NOT NULL,
    updated_at               TEXT NOT NULL,
    CHECK (length(trim(purpose)) > 0)
);
CREATE INDEX IF NOT EXISTS idx_user_flows_project ON user_flows(project_id);
CREATE INDEX IF NOT EXISTS idx_user_flows_persona ON user_flows(persona_id);

-- =========================================================================
-- L2 anchor (D33): each DetailScenario verifies one step of one UserFlow.
-- Nullable so existing scenarios stay valid; verify enforces coverage (D34).
-- =========================================================================
ALTER TABLE scenarios ADD COLUMN belongs_to_flow_id TEXT REFERENCES user_flows(id) ON DELETE SET NULL;
ALTER TABLE scenarios ADD COLUMN covers_flow_step TEXT;

-- =========================================================================
-- D35: decision-question engine. OPEN markers are promoted to SA-exam-style
-- blocking decision requests. qtype distinguishes fact (1 survivor → auto) vs
-- preference (2+ survivors → user choice). Adaptive branching via parent_question_id.
-- =========================================================================
CREATE TABLE IF NOT EXISTS decision_questions (
    id                  TEXT PRIMARY KEY,
    project_id          TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    short_code          TEXT NOT NULL UNIQUE,
    scope_ref           TEXT,                          -- node / flow / open-marker being filled
    qtype               TEXT NOT NULL DEFAULT 'preference'
                          CHECK (qtype IN ('fact','preference')),
    context_md          TEXT NOT NULL,                 -- SA-exam stem (detailed scenario context)
    parent_question_id  TEXT REFERENCES decision_questions(id) ON DELETE CASCADE,  -- adaptive tree
    status              TEXT NOT NULL DEFAULT 'open'
                          CHECK (status IN ('open','answered','auto_decided')),
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    CHECK (length(trim(context_md)) > 0)
);
CREATE INDEX IF NOT EXISTS idx_decision_questions_project_status ON decision_questions(project_id, status);
CREATE INDEX IF NOT EXISTS idx_decision_questions_parent ON decision_questions(parent_question_id);

CREATE TABLE IF NOT EXISTS question_options (
    id                 TEXT PRIMARY KEY,
    question_id        TEXT NOT NULL REFERENCES decision_questions(id) ON DELETE CASCADE,
    label              TEXT NOT NULL,
    body_md            TEXT NOT NULL DEFAULT '',
    rationale_md       TEXT NOT NULL DEFAULT '',       -- why this option is more / less correct
    is_llm_recommended INTEGER NOT NULL DEFAULT 0
                         CHECK (is_llm_recommended IN (0,1)),
    idx                INTEGER NOT NULL DEFAULT 0,
    created_at         TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_question_options_question ON question_options(question_id);

CREATE TABLE IF NOT EXISTS question_answers (
    id                  TEXT PRIMARY KEY,
    question_id         TEXT NOT NULL REFERENCES decision_questions(id) ON DELETE CASCADE,
    chosen_option_id    TEXT REFERENCES question_options(id) ON DELETE SET NULL,
    free_text           TEXT,                          -- +@ subjective fallback
    answered_by         TEXT NOT NULL DEFAULT 'user',
    generated_refs_json TEXT NOT NULL DEFAULT '[]',    -- entity ids this answer produced (provenance, D23/D35)
    created_at          TEXT NOT NULL,
    CHECK (chosen_option_id IS NOT NULL OR (free_text IS NOT NULL AND length(trim(free_text)) > 0))
);
CREATE INDEX IF NOT EXISTS idx_question_answers_question ON question_answers(question_id);
