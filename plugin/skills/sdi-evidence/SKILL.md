---
name: sdi-evidence
description: Record structured TaskEvidence when completing an SDI task — `sdi task complete <TASK-ID> --evidence <SCN-ID>=<verdict>@<ref>`, one row per linked scenario. Test evidence is a recorded run — `sdi run verify … -- <cmd>` then `<SCN-ID>=passing@run:<RUN-ID>`, which the daemon validates; free-text refs (file:line, CI URL, external URL, decision id) are only for non-test evidence. Use this skill whenever you complete a task in an SDI project. The daemon rejects EVIDENCE_REQUIRED otherwise.
---

# task complete — structured TaskEvidence

A task reaches `done` only through `sdi task complete`, and the daemon requires
**one evidence row per linked scenario**: the scenario, its verdict, and a
checkable reference. Missing evidence triggers `EVIDENCE_REQUIRED` and the
completion is rejected. This skill is the LLM-side authority on what evidence
to produce and how to record it.

---

## When to invoke

Trigger this skill when:

- You're about to complete a task (after implementing what the parent
  scenarios require and an independent run has verified it).
- You received `EVIDENCE_REQUIRED` (or a `VALIDATION` error on evidence) back
  from the daemon and need to recover.
- A carried `passing` arrived as `needs_verification` at round activation
  because its run is stale (its input fingerprint no longer matches the
  workspace) — run `sdi run verify` again and evidence the new run.

Don't invoke this skill for round-level state changes (use `sdi-round`) or
for authoring scenarios (use `sdi-scenario`).

---

## The evidence row

Each row is `<SCN-ID>=<verdict>@<ref>`:

- **`<SCN-ID>`** — a parent scenario of the task, as its SCN id or its
  plan-scoped short code. A scenario the task does not own is rejected.
- **`<verdict>`** — `passing` | `failing` | `impacted` | `retired`.
  `needs_verification` is **daemon-assigned** (a carried-over pass whose run
  is stale) and is rejected if submitted.
- **`<ref>`** — everything after the first `@`.

### Test evidence: a recorded run

Every verdict that rests on running tests is evidenced by a **run**, never by a
hand-built ref:

```bash
sdi run status --scenario <SCN-ID>                 # fresh → reuse that run; stale | none → verify
sdi run verify --scenario <SCN-ID> [--task <TASK-ID>] \
  [--input '<glob>' …] [--tool-version <name>=<ver> …] [--timeout <secs>] \
  -- <test command…>
```

`sdi run verify` executes the command and records the run: the daemon
fingerprints the inputs (scenario GWT + command + declared tool versions +
content of every input path; the whole repository when no `--input` is given),
captures the output to a report and stores its sha256, classifies a failure
(`product` | `harness` | `environment` | `spec`), derives the failure signature,
and books it against the retry budget. It prints the run JSON (with its id) and
exits with the command's status. Cite that id:

```bash
--evidence "<SCN-ID>=passing@run:<RUN-ID>"     # or failing@run:<RUN-ID> for a failed run
```

The daemon validates a `run:` ref strictly: the run exists, belongs to the
plan, is linked to the scenario, has finished, exited 0 for `passing` (or
failed for `failing`), and its input fingerprint is still fresh. Never paste
the report or any output into the ref, the summary, or a note — the run record
already holds the digest.

### Non-test evidence: free-text refs

A free-text ref is accepted but unverified, so use it only where no test run
applies. It is refused on a scenario whose retry budget is exhausted.

| Evidence kind | Ref shape | Example |
|---|---|---|
| Code (no test applies) | `path:line` or `path:start-end` | `daemon/src/routes/health.rs:42-60` |
| CI run | permanent run URL (the run id is in the URL) | `https://github.com/<org>/<repo>/actions/runs/123` |
| External | stable URL + what it proves | `https://staging.example.com/login — validation visible` |
| Decision | decision id, when the work *is* the decision | `DEC-12` |

If none of these fit, you are conflating evidence with rationale — see
"Evidence vs. decision" below.

---

## How to record via the CLI

Take the evidence format from `sdi task brief <TASK-ID>` instead of re-reading
`--help` before each completion (read-once rule: `sdi-overview` → "Read once: handoffs and CLI signatures").

```bash
sdi task complete <TASK-ID> \
  --evidence "<SCN-ID>=<verdict>@<ref>" \
  [--summary "<one line>"]
```

`--evidence` is repeatable — one per linked scenario:

```bash
sdi task complete TASK-7 \
  --evidence "SCN-3=passing@run:01J9Z…RUN" \
  --evidence "SCN-4=retired@DEC-12" \
  --summary "login validation green; SCN-4 superseded by DEC-12"
```

The task must be `in_progress` (`sdi task start <TASK-ID>`) before it can be
completed.

---

## How to record via MCP (LLM is driving)

Use the `update_task_evidence` write tool. It **completes the task** with the
evidence in one call — there is no separate done transition afterwards.

Input shape:
```json
{
  "task_id": "TASK-...",
  "summary": "login validation green",
  "scenarios": [
    { "scenario_id": "SCN-3", "result": "passing", "evidence_ref": "run:01J9Z…RUN" }
  ]
}
```

The daemon validates every row (parent scenario, verdict vocab, non-empty ref,
`run:` refs strictly) and commits the completion, the evidence, and the
round-result mirror atomically — either all of it lands or none of it does.

---

## `EVIDENCE_REQUIRED` recovery

If you get `EVIDENCE_REQUIRED` on `sdi task complete`, the daemon refused
because the evidence was empty or missing.

Recovery: re-send the command with a row per linked scenario. **Never paper
over** with `"n/a"`, `"see logs"`, `"trust me"`, or other unchecked text —
those defeat the round's verdict integrity and surface as
`needs_verification` (or worse, a false pass) at the next round's carry-over.

If no real evidence exists yet because the work isn't actually done, do not
complete the task. Leave it `in_progress`, or `sdi task block <TASK-ID>` if an
external dependency stalls it.

---

## What counts as "checkable"

A third party (or the daemon's auto-regression run) must be able to point at
the ref and confirm the claim. Concretely:

- **file:line that doesn't exist anymore** = not checkable. If you refactor,
  update the evidence ref.
- **A test verdict without a run** = not checkable. Record it with
  `sdi run verify` and cite `run:<RUN-ID>`.
- **URL behind auth that nobody can reach** = not checkable for the daemon.
  Use a public ref, or an external ref with a description that explains what
  the auth-gated artifact contains.
- **A manual command transcript** = not checkable. Run the command through
  `sdi run verify` so the daemon records it.

Checkability is what makes strict-regression possible. At the next round's
carry-over the daemon recomputes the input fingerprint of every carried
`passing` run; when it no longer matches the workspace, the scenario becomes
`needs_verification` — it is not silently kept as `passing` and not turned
into `failing` — and a fresh run must decide it.
Sloppy evidence yields sloppy regression signal.

---

## Evidence vs. decision

Evidence proves "the work was done". A decision (`sdi decision create`)
records "we chose X over Y because". Don't conflate them.

Edge case: when the work itself **is** the decision (e.g., "this scenario is
superseded by a new flow"), record the decision first, then point the task's
evidence at the decision id (`--evidence "SCN-5=retired@DEC-12"`) with a
summary such as "decision recorded as DEC-12 — no implementation this round."
The task can then be completed (the work was the act of recording the
decision) or cancelled with `sdi task cancel <TASK-ID>` (the scenario is being
dropped).

---

## Failure recovery

| Code | Meaning | Recovery |
|---|---|---|
| `EVIDENCE_REQUIRED` | `sdi task complete` arrived without evidence. | Re-send with a row per linked scenario. Never fill with placeholder text. |
| `VALIDATION` (evidence) | A row's ref was empty, its scenario is unknown in the plan or not a parent of the task, or its verdict is outside `passing \| failing \| impacted \| retired`. | Fix the row: real ref, a parent scenario, a submittable verdict. If you truly have nothing to point at, the task isn't done. |
| CLI parse error | `--evidence` was not `<SCN-ID>=<verdict>@<ref>`. | Quote the whole row and include both `=` and `@`. |
| `VALIDATION` (`needs_verification`) | The row submitted the daemon-assigned verdict. | Run `sdi run verify` and submit `passing` or `failing` with its `run:` ref. |
| `EVIDENCE_RUN_INVALID` | The `run:` ref is wrong: the run does not exist, belongs to another plan, is not linked to the scenario, has not finished, or its outcome contradicts the verdict (a failed run cited as `passing`). | Cite the run that actually verified this scenario with the matching verdict; if none passed, the scenario is not passing. |
| `EVIDENCE_STALE` | The cited run's input fingerprint no longer matches the workspace — the code, inputs, command or tool versions changed after it ran. | Re-run `sdi run verify --scenario <SCN-ID> … -- <cmd>` against the current inputs and cite the new run. |
| `RETRY_BUDGET_EXHAUSTED` | The scenario's failure signature hit the retry cap; free-text refs and new attempts are refused. | Stop retrying. Escalate to a decision, then `sdi run release-budget --scenario <SCN-ID> --decision <DEC-ID>` (see `sdi-impl-loop`). |
| `HARNESS_FAILURE_UNRESOLVED` | Round complete refused: a scenario's latest run failed for harness/environment reasons. | Fix the runner / toolchain / environment and record a passing `sdi run verify` for that scenario. |
