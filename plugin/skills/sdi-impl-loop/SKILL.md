---
name: sdi-impl-loop
description: Drive the inner loop — implementation convergence over rounds (D30/D31). After plan approve, open a round, fan out decompose → impl-coder → test-runner under a pattern, and on a failing/impacted verdict retry within the daemon's per-failure-signature budget, escalating on RETRY_BUDGET_EXHAUSTED (harness/environment failures are classified apart and never consume that budget). When all DetailScenarios pass, complete the round; when a regression against a prior round is detected, AUTO-OPEN the next round and replay — looping until regressions=0 and all pass. Use after sdi-converge has approved the plan, when the user wants implementation driven to green.
---

# /impl-loop — the inner loop (implementation convergence)

The **inner loop** is the round as a *verb* (D30): a deterministic orchestrator
runs `decompose → impl → test → verdict`, retries product failures within a
per-failure-signature budget,
**completes** a round when every DetailScenario passes, and **auto-opens the
next round** the moment a regression against a prior round appears — looping
until regressions = 0 and all pass (D34-4). The round is a wheel the loop
turns, not a status a human advances. This skill is the LLM-side authority on
the auto-rotation + retry-budget behaviour; it composes the existing round /
pattern / impl / test / regression agents rather than replacing them.

---

## When to invoke

Trigger after the spec is converged and the plan approved (D34):

- "drive this to green" / "implement the approved plan"
- "run the rounds until there's no regression" / "구현 루프 돌려서 끝까지 통과시켜"
- "verify R2 and auto-advance if anything regressed"

Don't invoke this for:
- **Spec building / decision questions** → that's the outer loop, `sdi-converge`.
  `completeness-critic` belongs to that loop only; it is never spawned inside
  a round. A round that uncovers a real spec gap escalates it, and the spec
  change re-enters `sdi-converge`.
- **A single round's manual create/activate semantics** → see `sdi-round` (this
  skill drives it on a loop; `sdi-round` documents the per-round mechanics).

---

## Preconditions

- Plan is `active` (approved through the D34 gate). If not, run `sdi-converge`
  first — implementing against an unconverged spec has no oracle to converge to.
- Daemon up (`sdi daemon start`). The inner loop uses the `sdi` CLI surface
  end-to-end (round / task / pattern / agent-note); the only HTTP seam it meets
  is the human-owned disruption-review resolution (see `sdi-round`).
- Every specialist reads its handoff and CLI signatures once (read-once rule:
  `sdi-overview` → "Read once: handoffs and CLI signatures").

---

## The loop (D31 inner — auto-rotate + retry budget)

```
round = open(plan)                       # R1 = new dev; R2+ = regression (D7)
loop:
  1. pattern   → pattern-orchestrator designs the fan-out over needs-verification; pattern-critic gates it
  2. decompose → sdi task create … --produced-via-pattern <PAT>  (one+ tasks per needs-verification scenario)
  3. fan out   → impl-coder (parallel) → test-runner: sdi run status → reuse fresh | sdi run verify → verdict@run:<RUN-ID>
  4. classify each failure:
        harness / environment → repair the run, re-verify   (no budget spent; blocks round complete)
        product {failing, impacted} → retry impl under the failure-signature budget
                                      | RETRY_BUDGET_EXHAUSTED → escalate
  5. all DetailScenarios pass, no unresolved harness failure? → sdi round complete <ROUND>
  6. regression vs a prior round?         → round = open(next)   (AUTO, D30)  → replay (regression-runner)
  7. regressions = 0 AND all pass         → exit (implementation converged)
```

### 1–2. Open, pattern, decompose

```bash
sdi round create <PLAN-ID> <SHORT-CODE>          # e.g. R1 ; defaults strict-regression@R≥2, pause-in-flight
sdi round activate <ROUND-ID>                     # carries prior verdicts; emits scenarios_needing_verification
```

Before the first `sdi task create`, spawn **pattern-orchestrator** on the
needs-verification set so the fan-out shape is a written decision (D13/D23) —
otherwise the daemon back-fills a `direct` sentinel and the whole round is
capped at L3. `pattern-critic` validates the shape (D26), then bind every task:

```bash
sdi task create <ROUND-ID> <SHORT-CODE> "<one-line desc>" \
  --scenario <SCN-ID> --produced-via-pattern <PAT-ID>
```

(Full per-round mechanics — modes, in-flight policy, disruption gate — live in
`sdi-round`; this skill drives them on a loop.)

### 3–4. Fan out, verdict, classify, retry budget, escalate

For each task: **impl-coder** implements the smallest change satisfying the
linked scenarios, then **test-runner** emits one evidence row per scenario with
a verdict from the fixed vocab `passing | failing | impacted | retired`.

Verification always goes through the daemon's run record:

```bash
sdi run status --scenario <SCN-ID>        # fresh | stale | none
sdi run verify --scenario <SCN-ID> --task <TASK-ID> [--input '<glob>' …] -- <test command…>
sdi task complete <TASK-ID> --evidence "<SCN-ID>=passing@run:<RUN-ID>"
```

- **Reuse a fresh run; never repeat an identical fingerprint.** Before
  running, check `sdi run status`. When it reports `fresh`, a passing run with
  the same input fingerprint already exists — cite it. Producer
  (`impl-coder`), verifier (`test-runner` / `regression-runner`), and
  integrator do not re-run a verification whose fingerprint is unchanged; only
  `stale` or `none` warrants `sdi run verify`.
- **Reuse does not merge roles.** The verifier still judges independently: it
  reads the run record (command, inputs, exit status, failure class) rather
  than the producer's claims, and the producer never cites its own unrecorded
  test output as evidence.

**Classify every failure before retrying.** A failed verification run is one
of two kinds, and they are handled differently:

- **Harness / environment failure** — the verification itself could not run
  as written: the runner crashed, a toolchain binary is missing, the daemon is
  unreachable, the harness timed out or was killed. It says nothing about the
  product, so it **does not consume the product retry budget** and is never
  recorded as a scenario `failing`. It still **blocks round completion** until
  a passing `sdi run verify` supersedes it: repair the environment or
  harness, then re-run the same verification. When the automatic
  classification is wrong, `sdi run verify --failure-class <class>` overrides
  it.
- **Product failure** (`failing` / `impacted`) — the code under test violates
  the scenario. Retry implementation within the retry budget.

The retry budget is owned by the daemon, not by the loop's own counter:

- **Keyed by primary cause.** The daemon derives a versioned identity from
  the scenario, failure class and normalized primary actionable diagnostic.
  Unsupported output uses an explicit coarse fallback. Renaming tasks or
  wrapper tests does not reset a counter; outstanding legacy or fallback
  authority remains conservative. Each failed execution charges one counter.
  See the [cause identity contract](https://github.com/scenario-driven/sdi-plugin/blob/main/docs/design/retry-cause-identity.md)
  for normalization, provenance and compatibility rules.
- **Cap.** The plan's autonomy policy sets the cap
  (`sdi autonomy set <PRJ-ID> --scope plan --plan-id <PLAN-ID> --mode <M>
  --retry-budget-cap <N>`); the default is 3 failures per signature. Inspect
  the counters with `sdi run budget [--scenario <SCN-ID>]`. Each attempt must
  consume the previous attempt's concrete failure (its run record and failing
  test id), not retry blindly.
- **On exhaustion the daemon reports `RETRY_BUDGET_EXHAUSTED`: escalate, do not
  loop.** New starts and completions on that scenario are refused until a
  decision releases the budget:
  `sdi run release-budget --scenario <SCN-ID> --decision <DEC-ID>
  [--signature <SIG>]`. Route the escalation by cause:
  - broken-by-a-deliberate-behaviour-change (`impacted`) → **disruption-analyst**
    (surface the cross-scenario blast; the user sees it) and, if it stems from a
    decision, **decision-resolver** to propose a supersession.
  - genuine `failing` the impl cannot satisfy → **decision-resolver** /
    **schema-architect** (if it needs a schema/architecture decision, D17
    forced-L4) — the spec or a decision is wrong, not just the code.

A scenario whose contract is itself wrong or untestable as written is a spec
failure, not a product failure: escalate it to the outer loop (`sdi-converge`)
instead of spending the retry budget on it.

A scenario must never be silently left red to "fix next round". The verdict is
sticky in both directions under strict-regression (D6).

### 5. Complete when green

When every DetailScenario in the needs-verification set has a `passing` verdict
with checkable evidence and no harness/environment failure is left unresolved:

```bash
sdi round complete <ROUND-ID>
```

The daemon rejects `sdi task complete` without evidence (`EVIDENCE_REQUIRED`)
and refuses round completion while a harness/environment failure is
unresolved. The loop itself never calls `sdi round complete` while any
scenario in the set is red or unverified — so "complete" genuinely means
"all green".

### 6. Auto-open the next round on regression (D30 — the defining behaviour)

After completing a round, check the prior round's passing scenarios against the
new code. Under strict-regression (D6 default at R≥2) the daemon carries prior
verdicts at the *next* activation — a carried `passing` whose evidence input
fingerprint no longer matches arrives as `needs_verification` and is
re-verified like any other unverified scenario — but the **decision to open that next round is
the orchestrator's, and it is automatic**: if a previously-`passing` scenario
now regresses, open the next round without waiting for a human:

```bash
sdi round create <PLAN-ID> <NEXT-SHORT-CODE>      # e.g. R2 ; strict-regression default at R≥2
sdi round activate <ROUND-ID>                      # auto-carries prior verdicts
```

Then **regression-runner** replays every prior-`passing` scenario under the new
code and posts verdicts:

```bash
sdi round result <ROUND-ID> --scenario <SCN-ID> --result <verdict> --evidence run:<RUN-ID>
```

A regressed scenario is marked `failing` (not softened to `impacted` — that is
reserved for a deliberate behaviour change), which re-enters the loop at step 3.

### 7. Exit when stable

The loop terminates only when a completed round shows **all DetailScenarios
passing AND zero regression** against the prior round (D34-4). Then the
implementation has converged.

---

## Invariants

- **The wheel turns itself.** Regression detection auto-opens the next round
  (D30); never stop at R1 and wait for the user to type `sdi round create`.
- **Budgeted retry, then escalate — never infinite, never silent-red.** The
  daemon's per-failure-signature budget bounds product retries; each attempt
  consumes the prior failure; `RETRY_BUDGET_EXHAUSTED` means escalate to the
  right specialist. Leaving a scenario red "for next round" is a protocol
  violation.
- **Harness failures are not product failures.** They never spend the
  product retry budget and never become a scenario `failing`, but the round
  does not complete until a passing run supersedes them.
- **Pattern before fan-out.** Decide the CollaborationPattern before the first
  task create, or the round silently L3-caps via the `direct` sentinel (D23).
- **Evidence is a run.** Test verdicts cite `run:<RUN-ID>` from
  `sdi run verify`; the daemon holds the report digest and validates the ref.
  Under strict-regression carry-over, a carried `passing` whose run is stale
  (input fingerprint no longer matches) becomes `needs_verification` — it is
  neither kept as `passing` nor turned into `failing`; a fresh run decides it.
- **One fingerprint, one run.** Check `sdi run status` and reuse a `fresh`
  run; no role re-runs an unchanged fingerprint.
- **Effort is a fact, not a brake.** "This round is taking long" is reported,
  never a reason to complete a round with a red scenario.

---

## Failure recovery

| Code | Meaning | Recovery |
|---|---|---|
| `EVIDENCE_REQUIRED` | `sdi task complete` arrived without evidence for a linked scenario. | Record a checkable evidence row per linked scenario (`sdi-evidence`), then retry the complete. |
| `INVALID_TRANSITION` | Another round on the plan is already `active`. | `sdi round complete <PREDECESSOR>` first, then activate the next. |
| `DISRUPTION_PENDING` | The plan has a pending disruption review. | A human resolves it (`POST /disruption-reviews/<REVIEW-ID>/resolve`, see `sdi-round`), then re-activate. |
| `INVALID_ROUND_MODE` | `--mode` is not `strict-regression` / `forward-only` (alias `additive`). | Use a valid mode; at R1 omit `--mode` (strict-regression carries nothing there). |
| `RETRY_BUDGET_EXHAUSTED` | The same failure signature hit the cap for this scenario — renaming or re-creating the task does not reset it. | Stop retrying. Escalate — `disruption-analyst` / `decision-resolver` / `schema-architect`; release only with `sdi run release-budget --scenario <SCN-ID> --decision <DEC-ID>`. A persistent red means the spec or a decision is wrong. |
| `HARNESS_FAILURE_UNRESOLVED` | Round complete refused: a scenario's latest run failed for harness/environment reasons. | Repair the runner / toolchain / environment, record a passing `sdi run verify` for each listed scenario, then complete. The product retry budget is untouched. |
| `EVIDENCE_RUN_INVALID` | A `run:` ref does not exist, is not linked to the scenario, has not finished, or contradicts the verdict. | Cite the run that verified this scenario, with the verdict its outcome supports (`sdi-evidence`). |
| `EVIDENCE_STALE` | The cited run's input fingerprint no longer matches the workspace. | Re-run `sdi run verify` against the current inputs and cite the new run. |
