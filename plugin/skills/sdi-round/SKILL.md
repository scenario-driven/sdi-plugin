---
name: sdi-round
description: Create and activate SDI rounds. Defaults to strict-regression mode (verdicts auto-carry from prior rounds). Handles in-flight task policy (pause/abort/continue-on-noimpact), disruption review gate, and LLM task auto-decomposition at activation time. Use this skill whenever the user wants to start, activate, or complete a round.
---

# /round — start, activate, complete an SDI round

A **round** is one iteration (R1, R2, …) of verifying scenarios. R2+ default
to strict-regression so prior verdicts carry automatically — that auto-
regression property is what distinguishes SDI from TDD/BDD. This skill is the
LLM-side authority on round mode, in-flight task policy, the disruption
review gate, and task auto-decomposition at activation time.

---

## When to invoke

Trigger this skill when the user says any of:

- "let's start round R2" / "kick off the next iteration"
- "activate the regression run"
- "open R3" / "다음 라운드 시작하자"
- "complete this round and move on"

Don't invoke this skill for scenario authoring (use `sdi-scenario`) or for
recording task evidence at `done` time (use `sdi-evidence`).

---

## The two round modes (D6)

Pick one at `sdi round create … --mode <mode>`:

- **`strict-regression`** (default at R≥2) — every prior verdict carries into
  the new round. Failing scenarios stay failed until you re-record a passing
  verdict. New scenarios verify fresh. This is the auto-regression property
  that distinguishes SDI from TDD/BDD; pick anything else only when the user
  explicitly asks.

- **`forward-only`** — skip carry-over; only new scenarios verify this round.
  Surface the consequence to the user before choosing this: prior regressions
  go untracked this round, so a previously failing scenario will appear absent
  rather than failing. Use only when the user accepts that trade-off, or at
  R1 where there is nothing to carry. `additive` is accepted as an alias for
  `forward-only`.

Disruption review is **not a mode** — it is a separate `--disruption <policy>`
(`needs-review` default | `auto`) plus the disruption-review gate (see below).
A round whose plan has unresolved scenario changes returns `DISRUPTION_PENDING`
on activation regardless of mode.

Default to strict-regression at R≥2. Omit `--mode` at R1 (or use
`forward-only`) — strict-regression is rejected at R1 because there are no
prior verdicts to carry; the daemon returns `MODE_REJECTED_AT_R1`.

---

## In-flight task policy (chosen at round-create time)

When a new round starts, tasks in `in_progress` from the previous round need
a disposition. Pick one with `--in-flight <policy>` (default `pause`):

- **`pause`** (default) — those tasks flip to `blocked`. Resume them after the
  new round establishes verdicts. Safe choice when the carried scenarios may
  redirect ongoing work.

- **`abort`** — cancel them. Use when scenarios changed and the work no
  longer applies.

- **`continue-on-noimpact`** — continue tasks whose parent scenarios didn't
  change between rounds. The daemon computes the impact set; tasks whose
  parent scenarios *did* change still flip to `blocked`. Use when most of the
  in-flight work is orthogonal to the scenario delta.

Document the reason in a decision (`/decide create`) when you override the
default.

---

## Round lifecycle (`planning → active → completed`)

1. **Create**
   ```bash
   sdi round create <PLAN-ID> --label R2 \
     [--mode strict-regression] \
     [--in-flight pause]
   ```
   The round enters `planning`. Multiple rounds can sit in `planning`, but
   only one round per plan can be `active` at a time.

2. **Activate**
   ```bash
   sdi round activate <ROUND-ID>
   ```
   The daemon flips status to `active`. Under strict-regression, every prior
   round's verdict (pass/fail/blocked) is copied into the new round at this
   step. The daemon then emits the list of scenarios needing fresh
   verification (new scenarios + carried-failing ones).

   The LLM **auto-decomposes** that list into tasks — tasks are runtime
   artifacts, not human-authored upfront. The arguments are positional
   (`<ROUND-ID> <SHORT-CODE> <DESCRIPTION>`); each task links its parent
   scenario with a repeatable `--scenario <SCN-ID>`:
   ```bash
   sdi task create <ROUND-ID> <SHORT-CODE> "<one-line description>" \
     --scenario <SCN-ID>
   ```
   There is no `--title` or `--tier` flag: the description IS the title, and a
   task carries no priority column (priority is the LLM's decomposition order,
   not persisted state per D3). Encode any priority hint in the scenario's
   `tags` instead. Do not pre-author tasks before activation; do not author
   tasks that don't trace back to a scenario in the round's needs-verification
   set.

3. **Verify**
   Implement, then record evidence on each task's `done` transition — the
   daemon rejects `done` without evidence (`EVIDENCE_REQUIRED`). See the
   `sdi-evidence` skill.

4. **Complete**
   ```bash
   sdi round complete <ROUND-ID>
   ```
   Status flips to `completed`. All verdicts persist. Completed rounds cannot
   be re-activated — create a new round for further work.

---

## Disruption review gate

If `sdi round activate <ROUND-ID>` returns `DISRUPTION_PENDING`, the plan has
unresolved scenario changes that require human review. The daemon will not
activate the round until the review is resolved.

Resolve with:
```bash
sdi disruption resolve <REVIEW-ID> --approve   # accept the scenario change
sdi disruption resolve <REVIEW-ID> --reject    # discard the scenario change
```
Then retry `sdi round activate <ROUND-ID>`.

Disruption review is triggered by the `--disruption` policy and the plan's
scenario-change state (a confirmed change to an existing scenario opens a
review), independent of the round `--mode`. It is not a round mode.

---

## Verdict carry semantics (strict-regression)

Under strict-regression, the daemon copies every prior round's verdict
(pass/fail/blocked) into the new round at activation time. Concretely:

- **Passing** scenarios from R(N-1) start R(N) as passing. The daemon
  re-checks them via the recorded evidence ref (see `sdi-evidence` —
  "checkable" matters here); a broken evidence ref demotes the verdict.
- **Failing** scenarios stay failing UNLESS you re-record a passing verdict
  via task done + evidence. There is no "I'll fix this next round" sticky
  exemption — the verdict is sticky in both directions.
- **Blocked** scenarios carry as blocked and surface as needs-attention.

Under `forward-only` (alias `additive`), none of the above happens — only new
scenarios verify and no carry-over occurs. Independently, when a disruption
review is open the daemon refuses to activate until the human resolves it.

---

## Task auto-decomposition at activation

When you activate a round, the daemon's response contains a
`scenarios_needing_verification` list:
- All scenarios newly added since the last completed round.
- Under strict-regression: all scenarios whose carried verdict is failing or
  blocked.

The LLM decomposes each item into one or more tasks. Tasks are *not* the
spec — scenarios are. A task is a runtime work unit that produces evidence
demonstrating the parent scenario now passes (or remains failing for a
documented reason). Each task carries a tier (`low | med | high`) advisory:
in the current SDI baseline tier is a warning surface, not a hard block.

---

## Failure recovery

| Code | Meaning | Recovery |
|---|---|---|
| `INVALID_TRANSITION` | Another round on the same plan is already `active`. | `sdi round complete <PREDECESSOR-ID>` first, then retry activation. |
| `DISRUPTION_PENDING` | A disruption review is open on the plan. | `sdi disruption resolve <REVIEW-ID> --approve\|--reject`, then retry. |
| `MODE_REJECTED_AT_R1` | `strict-regression` was requested at R1 (nothing to carry). | Use `--mode forward-only` (or its alias `additive`), or omit `--mode`. |
| `EVIDENCE_REQUIRED` | A task tried to transition to `done` without evidence — blocks round completion. | See the `sdi-evidence` skill; record at least one checkable evidence item, then retry. |
| `NOT_FOUND` | Round id or plan id wrong. | Re-resolve via `sdi round list <PLAN-ID>` or `sdi plan active <PROJECT-ID>`. |
