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

- **`strict-regression`** (default at R≥2) — the latest completed round's
  verdicts carry into the new round. Failing scenarios stay failed until you
  re-record a passing verdict; a carried pass whose evidence inputs changed
  arrives as `needs_verification`. New scenarios verify fresh. This is the auto-regression property
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

Default to strict-regression at R≥2. At R1 omit `--mode` (or use
`forward-only`): there is no completed prior round, so strict-regression has
nothing to carry and every scenario verifies fresh. An unknown mode string is
rejected with `INVALID_ROUND_MODE`.

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

Read handoffs and CLI signatures once (read-once rule: `sdi-overview` → "Read once: handoffs and CLI signatures").

## Round lifecycle (`planning → active → completed`)

1. **Create**
   ```bash
   sdi round create <PLAN-ID> R2 \
     [--mode strict-regression] \
     [--in-flight pause] \
     [--disruption needs-review]
   ```
   The round enters `planning`. Multiple rounds can sit in `planning`, but
   only one round per plan can be `active` at a time.

2. **Activate**
   ```bash
   sdi round activate <ROUND-ID>
   ```
   The daemon flips status to `active`. Under strict-regression, the latest
   completed round's verdicts are carried into the new round at this step
   (see "Verdict carry semantics" below). It records the round's fixed
   baseline (see "Verification baseline" below), then emits the list of
   scenarios needing fresh verification (`scenarios_needing_verification`) and
   the carried passes demoted as stale (`stale_carried`).

3. **Decide the collaboration pattern (D13) — before decomposing**
   The needs-verification set is the work about to fan out. *How* it fans out
   is a first-class decision (D13: multi-agent orchestration is the body, not a
   by-product), and it has to be made **before** the first `sdi task create` —
   otherwise the daemon back-fills a `direct` sentinel (D23) and the whole round
   is silently capped at L3.

   Spawn the **pattern-orchestrator** specialist on the needs-verification set.
   It picks a kind from the work shape and materialises the pattern:

   | Work shape | Kind | Autonomy unlocked (D25) |
   |---|---|---|
   | Sequential hand-offs (draft → critique → verify) | `workflow` | L5 |
   | Decision needing ≥2 distinct (name, stance) reviewers | `graph` | L5 |
   | N specialists running the tasks in parallel | `swarm` | L4 |
   | Caller agent needs a specific callee specialist as a tool | `agents-as-tools` | L4 |
   | Single leaf task — one task, one specialist, no fan-out or consensus need | `direct` | L3 |

   `pattern-critic` then validates the manifest (D26 shape gate: `workflow`
   steps ≥ 2, `graph` distinct tuples ≥ 2, `swarm` fan-out ≥ 2, …) and the
   pattern transitions `pending → active`. `direct` is the correct kind when the
   needs-verification set really is a single leaf task; multi-agent kinds are
   for work with ≥ 2 independent steps or perspectives. Choose `direct` as a
   written decision, never by omission — and never dress a single leaf task up
   as a 1-step workflow or 1-instance swarm to escape the L3 cap.

4. **Decompose under the chosen pattern**
   The LLM **auto-decomposes** the needs-verification set into tasks — tasks are
   runtime artifacts, not human-authored upfront. Bind each task to the active
   pattern from step 3 with `--produced-via-pattern`. The arguments are
   positional (`<ROUND-ID> <SHORT-CODE> <DESCRIPTION>`); each task links its
   parent scenario with a repeatable `--scenario <SCN-ID>`:
   ```bash
   sdi task create <ROUND-ID> <SHORT-CODE> "<one-line description>" \
     --scenario <SCN-ID> \
     --produced-via-pattern <PAT-ID>
   ```
   The daemon rejects a `--produced-via-pattern` that isn't `active` or belongs
   to another plan/round, so a stale or unshaped reference surfaces instead of
   silently degrading to `direct`. Omit the flag **only** when the work is a
   single leaf task that runs `direct`. There is no `--title` or `--tier`
   flag: the description IS the title, and a task carries no priority column
   (priority is the LLM's decomposition order, not persisted state per D3).
   Encode any priority hint in the scenario's `tags` instead. Do not pre-author tasks before activation; do
   not author tasks that don't trace back to a scenario in the round's
   needs-verification set.

5. **Verify**
   `sdi task start <TASK-ID>`, implement, verify with
   `sdi run verify --scenario <SCN-ID> --task <TASK-ID> -- <test command…>`
   (reusing a run `sdi run status` reports `fresh`), then complete with
   `sdi task complete <TASK-ID> --evidence "<SCN-ID>=passing@run:<RUN-ID>" …`
   — the daemon rejects completion without evidence (`EVIDENCE_REQUIRED`). See
   the `sdi-evidence` skill.

6. **Complete**
   ```bash
   sdi round complete <ROUND-ID>
   ```
   Status flips to `completed`. All verdicts persist. Completed rounds cannot
   be re-activated — create a new round for further work. Completion is
   refused with `HARNESS_FAILURE_UNRESOLVED` while any scenario's latest run
   failed for harness/environment reasons.

---

## Verification baseline (fixed at activation)

Activation records the round's **fixed baseline**: the activation commit plus
every confirmed scenario's verdict right after carry-over. That set is the
denominator of the round's progress, so the goal cannot move mid-round.

- **Progress is reported in two groups.** `sdi round results <ROUND-ID>` (and
  `sdi next`) return `progress.baseline` — the scenarios in the activation
  snapshot — and `progress.added_after_baseline` — scenarios confirmed after
  activation. Each group counts `passing` / `failing` / `impacted` /
  `needs_verification` / `unverified` and a percent; baseline scenarios retired
  since activation leave the denominator. Report the two groups separately;
  never fold added scenarios into the baseline percent.
- **The baseline is locked once the round is active.**
  `sdi round baseline <ROUND-ID>` reads it (caller blob + activation commit and
  snapshot). `--set '<JSON>'` stores a caller blob (for example
  `'{"green_tests":412}'`, surfaced in `sdi task brief`); on an active round an
  existing baseline refuses the overwrite with `BASELINE_LOCKED`.
- **Changing it takes a decision.** Record a superseding decision on the plan,
  then pass it: `sdi round baseline <ROUND-ID> --set '<JSON>' --decision <DEC-ID>`
  replaces the blob, and `sdi round baseline <ROUND-ID> --rebase --decision <DEC-ID>`
  re-captures the activation snapshot (commit + verdicts).

---

## Disruption review gate

If `sdi round activate <ROUND-ID>` returns `DISRUPTION_PENDING`, the plan has
a pending disruption review — a change that impacts existing scenarios and
needs human review. The daemon will not activate the round until every review
on the plan is resolved. Even `--disruption auto` requires the human confirm.

Disruption reviews have no `sdi` CLI subcommand; they live on the daemon HTTP
seam. List the pending reviews, show them to the user, and record the user's
verdict (`retire` the impacted scenarios, `edit` them, or `keep` them as-is):

```bash
SDI="http://127.0.0.1:$(cat ~/.cache/sdi/sdid.port)"
curl -s "$SDI/plans/<PLAN-ID>/disruption-reviews?status=pending"
curl -s "$SDI/disruption-reviews/<REVIEW-ID>/resolve" -H 'content-type: application/json' \
  -d '{"resolution":"keep","note":"<reason the user gave>"}'     # retire | edit | keep
```

Then retry `sdi round activate <ROUND-ID>`.

A disruption review is opened when a new or changed scenario, requirement, or
decision impacts existing scenarios: the LLM records the review with the
impacted list (`POST /disruption-reviews`). It is independent of the round `--mode`
and is not a round mode; the `--disruption` policy only decides whether the LLM
proposes a resolution up front — the human confirm is required either way.

---

## Verdict carry semantics (strict-regression)

Under strict-regression, the daemon carries the latest completed round's
verdicts (`passing` / `failing` / `impacted` / `retired`) into the new round at
activation time. Only scenarios that received a verdict there are carried;
scenarios that never had one verify fresh. Concretely:

- **Passing** scenarios from R(N-1) start R(N) as `passing` only while their
  evidence still holds. The daemon recomputes the input fingerprint of the
  evidencing run (the hash of everything that run consumed) against the
  workspace at carry time; a carried `passing` whose run is stale becomes
  **`needs_verification`** (reported in `stale_carried`). It is not
  silently kept as `passing`, and it is not turned into `failing` — it joins
  the needs-verification set and a fresh run decides it.
- **Failing** and **impacted** scenarios stay as carried UNLESS a task
  re-records a passing verdict via `sdi task complete` + evidence. There is no
  "I'll fix this next round" sticky exemption — the verdict is sticky in both
  directions.
- **Retired** scenarios carry as retired and stay out of verification.

`needs_verification` is daemon-assigned; callers never submit it.

Under `forward-only` (alias `additive`), none of the above happens — only new
scenarios verify and no carry-over occurs. Independently, when a disruption
review is open the daemon refuses to activate until the human resolves it.

---

## Task auto-decomposition at activation

When you activate a round, the daemon's response contains a
`scenarios_needing_verification` list:
- All scenarios newly added since the last completed round.
- Under strict-regression: all scenarios whose carried verdict is `failing`,
  `impacted`, or `needs_verification`.

The LLM decomposes the set into tasks. Tasks are *not* the spec — scenarios
are. A task is a runtime work unit that produces evidence demonstrating the
parent scenario now passes (or remains failing for a documented reason). Gaps
serving the same scenario's acceptance criterion go into one task; split only
along independent verification boundaries (see `scenario-decomposer`), never
one task per file or per tiny gap.

---

## Failure recovery

| Code | Meaning | Recovery |
|---|---|---|
| `INVALID_TRANSITION` | Another round on the same plan is already `active`. | `sdi round complete <PREDECESSOR-ID>` first, then retry activation. |
| `DISRUPTION_PENDING` | A disruption review is pending on the plan. | Resolve it with the user's verdict via `POST /disruption-reviews/<REVIEW-ID>/resolve` (see "Disruption review gate"), then retry. |
| `INVALID_ROUND_MODE` | `--mode` was not `strict-regression` / `forward-only` (alias `additive`). | Use a valid mode, or omit `--mode`. |
| `EVIDENCE_REQUIRED` | `sdi task complete` arrived without evidence for a linked scenario. | See the `sdi-evidence` skill; record a checkable evidence row per linked scenario, then retry. |
| `HARNESS_FAILURE_UNRESOLVED` | Round complete refused: a scenario's latest run failed for harness/environment reasons. | Repair the runner / toolchain / environment and record a passing `sdi run verify` for each listed scenario; it does not consume the product retry budget. |
| `BASELINE_LOCKED` | `sdi round baseline --set` / `--rebase` on an active round without a superseding decision. | Record a superseding decision on the plan, then re-run with `--decision <DEC-ID>`; otherwise leave the baseline as recorded. |
| `EVIDENCE_STALE` | A cited `run:` ref no longer matches the workspace. | Re-run `sdi run verify` and cite the new run (`sdi-evidence`). |
| `NOT_FOUND` | Round id or plan id wrong. | Re-resolve via `sdi round list <PLAN-ID>` or `sdi plan active <PROJECT-ID>`. |
