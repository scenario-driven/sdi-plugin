---
name: sdi-evidence
description: Record structured TaskEvidence when transitioning an SDI task to done. Evidence must be checkable — file:line for code, test transcript path for tests, run id for CI, URL for external checks. Use this skill whenever you complete a task in an SDI project. The daemon rejects EVIDENCE_REQUIRED otherwise.
---

# task done — structured TaskEvidence

When a task transitions to `done`, the daemon requires at least one
**structured evidence item**. Empty evidence triggers `EVIDENCE_REQUIRED`
and the transition is rejected. This skill is the LLM-side authority on what
evidence to produce and how to record it.

---

## When to invoke

Trigger this skill when:

- You're about to mark a task `done` (after implementing what the parent
  scenario requires).
- You received `EVIDENCE_REQUIRED` back from the daemon and need to recover.
- You're auto-carrying verdicts at round activation and an old evidence ref
  no longer resolves — re-record fresh evidence and the verdict heals.

Don't invoke this skill for round-level state changes (use `sdi-round`) or
for authoring scenarios (use `sdi-scenario`).

---

## The five evidence kinds

Exactly one `kind` per evidence item, chosen from:

- **`code`** — file:line(s) where the change lives. The reader (or the
  daemon's auto-regression run at the next round) must be able to open that
  file at that line and see the work.
  Example: `daemon/src/routes/health.rs:42-60`

- **`test`** — path to the test file plus the specific test name, plus a
  short transcript snippet showing the pass.
  Example: `tests/round.e2e.test.rs::test_r2_carries_failing_verdict — pass`

- **`ci`** — CI run id plus URL. The URL must resolve to a permanent record
  (not an expired job log).
  Example: `gh-actions:01HXYZ... → https://github.com/.../actions/runs/123`

- **`external`** — URL plus a brief description of what at that URL proves
  the work. Use for staging links, third-party dashboards, screenshots
  hosted at a stable URL.
  Example: `https://staging.example.com/login — manual smoke test shows the
  new validation`

- **`transcript`** — a literal command transcript pasted into the evidence
  body. Use when no file:line applies (e.g., a `sdi doctor` run that prints
  OK, a deploy command output, an interactive REPL session).

If none of the five fit, you are conflating evidence with rationale — see
"Evidence vs. decision" below.

---

## How to record via slash command

```bash
sdi task done <TASK-ID> \
  --evidence-kind <code|test|ci|external|transcript> \
  --evidence-ref  "<ref>" \
  --evidence-summary "<one-line>"
```

For multiple evidence items, repeat the three flags together in matching
order:

```bash
sdi task done <TASK-ID> \
  --evidence-kind code --evidence-ref "src/auth.rs:120-145" \
    --evidence-summary "validation added in login handler" \
  --evidence-kind test --evidence-ref "tests/auth.rs::test_login_validation — pass" \
    --evidence-summary "regression test covers the new branch"
```

---

## How to record via MCP (LLM is driving)

Use the `update_task_evidence` write tool (one of the 5 MCP write tools the
SDI server exposes).

Input shape:
```json
{
  "task_id": "TASK-...",
  "evidence": [
    { "kind": "code", "ref": "src/auth.rs:120-145", "summary": "validation added in login handler" },
    { "kind": "test", "ref": "tests/auth.rs::test_login_validation — pass", "summary": "regression test covers the new branch" }
  ]
}
```

The daemon validates the structure and persists. Then call the `done`
transition (slash command or whichever mutation tool is configured); the
recorded evidence satisfies the requirement.

---

## `EVIDENCE_REQUIRED` recovery

If you get `EVIDENCE_REQUIRED` on a `sdi task done` call, the daemon refused
because the evidence array was empty (or missing).

Recovery: re-send the command with at least one evidence item. **Never paper
over** with `"n/a"`, `"see logs"`, `"trust me"`, or other unchecked text —
those defeat the round's verdict integrity and will be flagged at the next
round's auto-carry step.

If no real evidence exists yet because the work isn't actually done, do not
transition the task to `done`. Leave it in `in_progress` (or move to
`blocked` if a dependency stalls it).

---

## What counts as "checkable"

A third party (or the daemon's auto-regression run) must be able to point at
the ref and confirm the claim. Concretely:

- **file:line that doesn't exist anymore** = not checkable. If you refactor,
  update the evidence ref.
- **Test name that's been renamed** = not checkable. The auto-regression run
  will fail to find it and demote the verdict.
- **URL behind auth that nobody can reach** = not checkable for the daemon.
  Use a public ref or an `external` item with a description that explains
  what the auth-gated artifact contains.
- **Transcript of a manual run with no reproducible command** = not
  checkable. Include the command verbatim alongside the output.

Checkability is what makes strict-regression possible — the daemon uses each
recorded evidence ref at the next round's auto-carry step to verify the prior
pass still holds. Sloppy evidence yields sloppy regression signal.

---

## Evidence vs. decision

Evidence proves "the work was done". A decision (`/decide`) records "we
chose X over Y because". Don't conflate them.

Edge case: when the work itself **is** the decision (e.g., "decided to defer
this scenario"), record a decision via `/decide create`, then point the
task's evidence at the decision id with a `transcript` or `external` kind
summarising "decision recorded as DEC-NNN — no implementation this round."
The task can then be `done` (the work was the act of recording the decision)
or `cancelled` (the scenario is being dropped).

---

## Failure recovery

| Code | Meaning | Recovery |
|---|---|---|
| `EVIDENCE_REQUIRED` | The `done` transition arrived without any evidence items. | Re-send with at least one evidence item. Never fill with placeholder text. |
| `EVIDENCE_FORMAT_INVALID` | `kind` was something other than `code \| test \| ci \| external \| transcript`. | Pick the correct kind; if none fit you're probably recording rationale (use `/decide`) not evidence. |
| `EVIDENCE_REF_EMPTY` | `--evidence-ref` was empty or whitespace-only. | Provide a real ref. If you truly have nothing to point at, the task isn't `done`. |
