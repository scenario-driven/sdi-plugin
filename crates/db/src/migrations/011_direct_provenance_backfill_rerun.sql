-- =========================================================================
-- 011 — D23 direct-provenance back-fill, second pass.
--
-- Migration 009 back-filled NULL `produced_via_pattern_id` rows once. But a
-- database that 009 already visited could keep accumulating NULL-provenance
-- rows afterwards: any daemon built before the write-path change (≤ v0.3.0)
-- creates work entities without resolving the plan's `direct` sentinel.
-- Observed while dogfooding — entities created by the installed 0.3.0 daemon
-- after a workspace daemon had consumed 009 stayed NULL forever, leaving the
-- Patterns view empty while the entities clearly exist.
--
-- The statements are the same idempotent set as 009 (NOT EXISTS guards,
-- NULL-only updates), so re-running them repairs the stragglers and is a
-- no-op everywhere else. From this release on the write paths themselves
-- resolve the sentinel, so no third pass should ever be needed.
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
