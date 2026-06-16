-- =========================================================================
-- 012 — scenario retirement (#8).
--
-- Scenarios gain a nullable `retired_at` timestamp. A scenario is "retired"
-- iff `retired_at IS NOT NULL`. Retirement is reversible (un-retire sets it
-- back to NULL) and orthogonal to the authoring `status` (draft/confirmed),
-- which is PRESERVED across retire/un-retire — so restoring a confirmed
-- scenario brings it back as confirmed, with no reconstruction guesswork.
--
-- Why a flag column, not a `retired` value in the status CHECK: a status
-- enum value would (a) require a full table rebuild to widen the CHECK
-- (the migration-010 surgery, with FK + FTS hazards) and (b) overwrite the
-- draft/confirmed state, losing the information needed to restore it. A
-- nullable column is the economical, reversible, history-preserving form
-- (D12 append-only — past round verdicts are untouched).
--
-- Retired scenarios are excluded from the approve-gate count, the
-- needs-verification set, and strict-regression carry-over (enforced in the
-- repo queries). Their historical scenario_results rows stay intact.
ALTER TABLE scenarios ADD COLUMN retired_at TEXT;

-- Partial index: the hot path is "active (non-retired) scenarios in a plan".
CREATE INDEX IF NOT EXISTS idx_scenarios_active
    ON scenarios(plan_id) WHERE retired_at IS NULL;
