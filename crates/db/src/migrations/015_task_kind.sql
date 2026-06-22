-- =========================================================================
-- 015 — chore task kind (#18).
--
-- Trivial consistency edits after a round/plan closes (no active plan) were
-- blocked by the active-task PreToolUse gate: with no active plan there is no
-- round to decompose a task from, so the gate could never be satisfied for a
-- one-line cleanup. The chore lane is the escape hatch — a kind='chore' task
-- created + started in one step, completed with a free-text note instead of
-- scenario evidence (a chore has no GWT scenario to evidence).
--
-- Tasks gain a `kind` discriminator. 'task' is the existing scenario-decomposed
-- runtime artifact (D3); 'chore' is the lightweight maintenance lane. Additive
-- ALTER — no table rebuild.
ALTER TABLE tasks ADD COLUMN kind TEXT NOT NULL DEFAULT 'task';

-- The chore lane needs a per-project container Plan (short_code='CHORE') that
-- stays `active` permanently so its Round can hold in_progress chores and so
-- the active-task gate is satisfiable without a real plan. But D8's
-- `uniq_one_active_plan_per_project` partial index makes "active" a per-project
-- singleton, and `uniq_one_active_round_per_plan` does the same per plan —
-- so the maintenance container would collide with (or be blocked by) a user's
-- real active plan/round.
--
-- The fix mirrors the application-layer exclusion (`find_active_for_project`
-- filters `short_code NOT LIKE 'CHORE%'`): the partial uniqueness predicates
-- exclude the CHORE container too. D8's "one active plan per project" invariant
-- is preserved for real work plans; the maintenance container is an orthogonal
-- always-on lane that does not count as "the active plan".
DROP INDEX IF EXISTS uniq_one_active_plan_per_project;
CREATE UNIQUE INDEX uniq_one_active_plan_per_project
    ON plans(project_id)
    WHERE status = 'active' AND short_code NOT LIKE 'CHORE%';

DROP INDEX IF EXISTS uniq_one_active_round_per_plan;
CREATE UNIQUE INDEX uniq_one_active_round_per_plan
    ON rounds(plan_id)
    WHERE status = 'active' AND short_code NOT LIKE 'CHORE%';
