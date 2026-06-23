-- SDI 2.0 plan↔flow targeting (PRD-v2 D34). A plan declares which UserFlows it
-- must satisfy; approve then enforces L2 step coverage over the targeted flows.
-- Plans with no targeted flow fall back to the legacy D8 gate during the v1→v2
-- transition — D8 becomes a dead path once every plan is flow-scoped.
CREATE TABLE IF NOT EXISTS plan_flows (
    plan_id    TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    flow_id    TEXT NOT NULL REFERENCES user_flows(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    PRIMARY KEY (plan_id, flow_id)
);
CREATE INDEX IF NOT EXISTS idx_plan_flows_flow ON plan_flows(flow_id);
