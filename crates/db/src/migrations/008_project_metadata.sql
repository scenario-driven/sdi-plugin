-- 008 — Project metadata expansion for v0.3 lifecycle management.
--
-- Adds three SNAPSHOT fields (D2 first-class Project entity, no version
-- history kept inside the row):
--   description     — free-form long text (NULL = unset)
--   enabled         — soft-disable flag honored by the hook layer
--                     (0 = SDI hooks skip every gate for the project's cwds,
--                      letting Claude Code drive the repo without SDI
--                      governance), default 1 = active
--   wiki_paths_json — array of wiki roots ("docs", "wiki", or absolute
--                     paths). Default '["docs"]' mirrors Clawket's default
--                     so existing rows keep the standard root after backfill.
--
-- All three columns have defaults so the in-place ALTER backfills cleanly
-- across every existing row (enabled=1, description=NULL, wiki_paths='["docs"]').
-- Project FK cascade chain (plans → requirements/decisions/scenarios/rounds →
-- tasks, plus comments/activity/knowledge/autonomy_policies/agent_notes/
-- collaboration_patterns/disruption_reviews) is already declared with ON
-- DELETE CASCADE, so the daemon's `DELETE FROM projects WHERE id=?1` will
-- naturally tear down every PROJ-scoped row in a single statement once
-- PRAGMA foreign_keys=ON (already set in pool.rs).

ALTER TABLE projects ADD COLUMN description     TEXT;
ALTER TABLE projects ADD COLUMN enabled         INTEGER NOT NULL DEFAULT 1;
ALTER TABLE projects ADD COLUMN wiki_paths_json TEXT    NOT NULL DEFAULT '["docs"]';
