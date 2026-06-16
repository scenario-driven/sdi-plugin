-- =========================================================================
-- 014 — round verification baseline (#15).
--
-- A round gains a nullable `baseline_json` blob: the verification baseline
-- captured for the round (e.g. {"green_tests": 412, "ref": "<sha>"}). The
-- daemon stores it and surfaces it in `sdi task brief` so a weak model is
-- handed the baseline it would otherwise forget — the daemon does not, so a
-- regression shows up as a count delta. Free-form JSON; the shape is the
-- caller's contract (the test-runner records it), not enforced here.
ALTER TABLE rounds ADD COLUMN baseline_json TEXT;
