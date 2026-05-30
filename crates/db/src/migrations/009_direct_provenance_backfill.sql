-- =========================================================================
-- 009 — D23 direct-provenance back-fill.
--
-- Migration 007 added `produced_via_pattern_id` to the six work-entity tables
-- as a nullable FK so legacy rows could migrate. The daemon now resolves a
-- per-plan `direct` sentinel CollaborationPattern whenever a work entity is
-- created without a pattern (the solo-flow / CLI case). This migration applies
-- the same rule retroactively to rows that predate the write-path change, so
-- the Patterns dashboard reflects the real (solo) provenance instead of an
-- empty timeline.
--
-- `direct` is the absence of orchestration — a plan-level singleton, not a
-- per-outcome pattern instance — so each plan gets at most one sentinel,
-- shared by all of its solo-produced entities. The sentinel id is derived
-- deterministically (`CP-DIRECT-<plan_id>`) so this migration is idempotent
-- and never duplicates a row.
--
-- Timestamps are written in the RFC3339-with-millis shape the repo layer
-- parses (`fmt_ts` / `ts`): `strftime('%Y-%m-%dT%H:%M:%fZ', 'now')`.
-- =========================================================================

-- 1. One `direct` sentinel per plan that has at least one solo-produced
--    (NULL-provenance) work entity. Plans with no work — or whose work was all
--    produced under real patterns — get no sentinel, keeping their timeline
--    honestly empty. The plan shell itself is NOT a trigger: marking every
--    plan `direct` at birth would over-signal the anti-pattern.
INSERT INTO collaboration_patterns
    (id, short_code, plan_id, kind, applies_to, scope_id, depth, lifecycle,
     decided_reason, created_at, updated_at)
SELECT
    'CP-DIRECT-' || p.id,
    'DIRECT-' || p.short_code,
    p.id,
    'direct',
    'plan',
    p.id,
    0,
    'active',
    'solo-flow marker — back-filled (D23); entities produced without a collaboration pattern',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM plans p
WHERE NOT EXISTS (
        SELECT 1 FROM collaboration_patterns cp WHERE cp.id = 'CP-DIRECT-' || p.id
      )
  AND (
        EXISTS (SELECT 1 FROM scenarios    s  WHERE s.plan_id  = p.id AND s.produced_via_pattern_id  IS NULL)
     OR EXISTS (SELECT 1 FROM rounds       r  WHERE r.plan_id  = p.id AND r.produced_via_pattern_id  IS NULL)
     OR EXISTS (SELECT 1 FROM requirements rq WHERE rq.plan_id = p.id AND rq.produced_via_pattern_id IS NULL)
     OR EXISTS (SELECT 1 FROM decisions    d  WHERE d.plan_id  = p.id AND d.produced_via_pattern_id  IS NULL)
     OR EXISTS (
            SELECT 1 FROM tasks t JOIN rounds r2 ON t.round_id = r2.id
            WHERE r2.plan_id = p.id AND t.produced_via_pattern_id IS NULL
        )
      );

-- 2. Link each NULL-provenance work entity to its plan's sentinel. The
--    EXISTS guard skips entities whose plan got no sentinel (none such after
--    step 1, but it keeps each statement self-contained and FK-safe).
UPDATE scenarios
   SET produced_via_pattern_id = 'CP-DIRECT-' || plan_id
 WHERE produced_via_pattern_id IS NULL
   AND EXISTS (SELECT 1 FROM collaboration_patterns cp WHERE cp.id = 'CP-DIRECT-' || scenarios.plan_id);

UPDATE rounds
   SET produced_via_pattern_id = 'CP-DIRECT-' || plan_id
 WHERE produced_via_pattern_id IS NULL
   AND EXISTS (SELECT 1 FROM collaboration_patterns cp WHERE cp.id = 'CP-DIRECT-' || rounds.plan_id);

UPDATE requirements
   SET produced_via_pattern_id = 'CP-DIRECT-' || plan_id
 WHERE produced_via_pattern_id IS NULL
   AND EXISTS (SELECT 1 FROM collaboration_patterns cp WHERE cp.id = 'CP-DIRECT-' || requirements.plan_id);

UPDATE decisions
   SET produced_via_pattern_id = 'CP-DIRECT-' || plan_id
 WHERE produced_via_pattern_id IS NULL
   AND EXISTS (SELECT 1 FROM collaboration_patterns cp WHERE cp.id = 'CP-DIRECT-' || decisions.plan_id);

-- tasks reach their plan through the round.
UPDATE tasks
   SET produced_via_pattern_id = (
        SELECT 'CP-DIRECT-' || r.plan_id FROM rounds r WHERE r.id = tasks.round_id
   )
 WHERE produced_via_pattern_id IS NULL
   AND EXISTS (
        SELECT 1 FROM rounds r
        JOIN collaboration_patterns cp ON cp.id = 'CP-DIRECT-' || r.plan_id
        WHERE r.id = tasks.round_id
   );

-- The plan shell's own provenance is set only for plans that actually did
-- solo work (i.e. a sentinel was created above).
UPDATE plans
   SET produced_via_pattern_id = 'CP-DIRECT-' || id
 WHERE produced_via_pattern_id IS NULL
   AND EXISTS (SELECT 1 FROM collaboration_patterns cp WHERE cp.id = 'CP-DIRECT-' || plans.id);
