-- =========================================================================
-- 013 — provisional decisions via supersede-when trigger (#16).
--
-- A decision gains a nullable `supersede_when` free-text condition. A
-- decision with `supersede_when IS NOT NULL` is "provisional": still
-- `accepted` and in effect, but flagged to be revisited when the condition
-- holds. This lets an autonomous run record a reasoned, reversible (Type-2)
-- choice and keep moving instead of stopping to ask — without inventing a new
-- status (the ADR for #16: economy of mechanism; the existing supersession
-- chain already models "this may change", per the ADR / Nygard's ADR
-- lifecycle, so provisional is a trigger column, not a status value).
ALTER TABLE decisions ADD COLUMN supersede_when TEXT;

-- Partial index for the "open provisional decisions" query (sdi next / dash).
CREATE INDEX IF NOT EXISTS idx_decisions_provisional
    ON decisions(plan_id) WHERE supersede_when IS NOT NULL;
