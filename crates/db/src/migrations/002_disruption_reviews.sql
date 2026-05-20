-- PRD §6 #4 — Disruption needs-review queue.
--
-- The LLM records a row here when it detects that a new scenario / requirement
-- / decision invalidates or modifies existing scenarios. Round activation is
-- gated on every row for the plan being status='resolved'.

CREATE TABLE IF NOT EXISTS disruption_reviews (
    id                      TEXT PRIMARY KEY,
    plan_id                 TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    source_kind             TEXT NOT NULL
                              CHECK (source_kind IN ('scenario','requirement','decision')),
    source_id               TEXT NOT NULL,
    impacted_scenario_ids   TEXT NOT NULL,                   -- JSON array of scenario IDs
    status                  TEXT NOT NULL DEFAULT 'pending'
                              CHECK (status IN ('pending','resolved')),
    resolution              TEXT
                              CHECK (resolution IS NULL OR resolution IN ('retire','edit','keep')),
    note                    TEXT,
    created_at              TEXT NOT NULL,
    resolved_at             TEXT
);
CREATE INDEX IF NOT EXISTS idx_disruption_reviews_plan_status
    ON disruption_reviews(plan_id, status);
