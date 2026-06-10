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
-- Unlike 009, this pass must coexist with sentinels minted at RUNTIME by
-- `ensure_direct_pattern` (crates/daemon/src/router/provenance.rs), whose id
-- scheme is `CP-<ulid>` — NOT 009's deterministic `CP-DIRECT-<plan_id>`.
-- Guarding by id (009's approach) misses those rows and then collides with
-- the `(plan_id, short_code)` UNIQUE that migration 010 just introduced.
-- The semantic key for "this plan already has its solo-flow marker" is
-- `kind = 'direct'` (a plan-level singleton per D23), so every statement
-- below resolves the sentinel by (plan_id, kind) regardless of id scheme.

-- 1. One `direct` sentinel per plan that has at least one solo-produced
--    (NULL-provenance) work entity and no direct sentinel of EITHER id
--    scheme yet. Plans with no solo work get no sentinel, keeping their
--    timeline honestly empty.
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
    'solo-flow marker — back-filled (D23, second pass); entities produced without a collaboration pattern',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM plans p
WHERE NOT EXISTS (
        SELECT 1 FROM collaboration_patterns cp
        WHERE cp.plan_id = p.id AND cp.kind = 'direct'
      )
  AND (
        EXISTS (SELECT 1 FROM scenarios    s  WHERE s.plan_id  = p.id AND s.produced_via_pattern_id  IS NULL)
     OR EXISTS (SELECT 1 FROM rounds       r  WHERE r.plan_id  = p.id AND r.produced_via_pattern_id  IS NULL)
     OR EXISTS (SELECT 1 FROM requirements rq WHERE rq.plan_id = p.id AND rq.produced_via_pattern_id IS NULL)
     OR EXISTS (SELECT 1 FROM decisions    d  WHERE d.plan_id  = p.id AND d.produced_via_pattern_id  IS NULL)
     OR EXISTS (SELECT 1 FROM tasks        t  WHERE t.plan_id  = p.id AND t.produced_via_pattern_id  IS NULL)
      );

-- 2. Link each NULL-provenance work entity to its plan's direct sentinel —
--    whichever id scheme that sentinel carries. The oldest direct row wins
--    deterministically if a plan somehow holds more than one.
UPDATE scenarios
   SET produced_via_pattern_id = (
        SELECT cp.id FROM collaboration_patterns cp
        WHERE cp.plan_id = scenarios.plan_id AND cp.kind = 'direct'
        ORDER BY cp.created_at, cp.id LIMIT 1
   )
 WHERE produced_via_pattern_id IS NULL
   AND EXISTS (
        SELECT 1 FROM collaboration_patterns cp
        WHERE cp.plan_id = scenarios.plan_id AND cp.kind = 'direct'
   );

UPDATE rounds
   SET produced_via_pattern_id = (
        SELECT cp.id FROM collaboration_patterns cp
        WHERE cp.plan_id = rounds.plan_id AND cp.kind = 'direct'
        ORDER BY cp.created_at, cp.id LIMIT 1
   )
 WHERE produced_via_pattern_id IS NULL
   AND EXISTS (
        SELECT 1 FROM collaboration_patterns cp
        WHERE cp.plan_id = rounds.plan_id AND cp.kind = 'direct'
   );

UPDATE requirements
   SET produced_via_pattern_id = (
        SELECT cp.id FROM collaboration_patterns cp
        WHERE cp.plan_id = requirements.plan_id AND cp.kind = 'direct'
        ORDER BY cp.created_at, cp.id LIMIT 1
   )
 WHERE produced_via_pattern_id IS NULL
   AND EXISTS (
        SELECT 1 FROM collaboration_patterns cp
        WHERE cp.plan_id = requirements.plan_id AND cp.kind = 'direct'
   );

UPDATE decisions
   SET produced_via_pattern_id = (
        SELECT cp.id FROM collaboration_patterns cp
        WHERE cp.plan_id = decisions.plan_id AND cp.kind = 'direct'
        ORDER BY cp.created_at, cp.id LIMIT 1
   )
 WHERE produced_via_pattern_id IS NULL
   AND EXISTS (
        SELECT 1 FROM collaboration_patterns cp
        WHERE cp.plan_id = decisions.plan_id AND cp.kind = 'direct'
   );

-- tasks carry plan_id directly since migration 010 — no round join needed.
UPDATE tasks
   SET produced_via_pattern_id = (
        SELECT cp.id FROM collaboration_patterns cp
        WHERE cp.plan_id = tasks.plan_id AND cp.kind = 'direct'
        ORDER BY cp.created_at, cp.id LIMIT 1
   )
 WHERE produced_via_pattern_id IS NULL
   AND EXISTS (
        SELECT 1 FROM collaboration_patterns cp
        WHERE cp.plan_id = tasks.plan_id AND cp.kind = 'direct'
   );

-- The plan shell's own provenance is set only for plans that actually did
-- solo work (i.e. a direct sentinel exists for them).
UPDATE plans
   SET produced_via_pattern_id = (
        SELECT cp.id FROM collaboration_patterns cp
        WHERE cp.plan_id = plans.id AND cp.kind = 'direct'
        ORDER BY cp.created_at, cp.id LIMIT 1
   )
 WHERE produced_via_pattern_id IS NULL
   AND EXISTS (
        SELECT 1 FROM collaboration_patterns cp
        WHERE cp.plan_id = plans.id AND cp.kind = 'direct'
   );
