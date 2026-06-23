---
name: sdi-converge
description: Drive the outer loop — spec convergence toward the completeness oracle (D31/D34/D35). Scans oracle/verify, collects OPEN and uncovered decision-points, runs §2a elimination per point (1 survivor → auto-decision, 2+ → preference DecisionQuestion), collects answers, compiles them deterministically into the graph, then calls completeness-critic — looping until uncovered=0, unanswered=0, AND unasked=0. On a dry loop the plan can be approved (D34). Use when building or completing the product definition before implementation.
---

# /converge — the outer loop (spec convergence)

The **outer loop** drives the product definition toward the completeness
**oracle** (D32): it loops until "uncovered (Persona×Capability) = 0 / flow-step
uncovered GWT = 0 / unanswered decision = 0 / **unasked** decision = 0". When
the loop runs dry, the plan can be approved (D34) and the **inner loop**
(`sdi-impl-loop`) takes over. This skill is the LLM-side authority on the
spec-convergence loop; the daemon owns the deterministic verdict and the gates.

> **Lineage**: v1 approved a plan on `confirmed ≥ 1` scenario (D8). v2 replaces
> that with a hard, all-tier completeness gate (D34). The loop is what makes the
> gate reachable instead of just blocking.

---

## When to invoke

Trigger this skill when the user wants to **build or complete the spec** before
implementing:

- "let's nail down the full product definition"
- "what decisions are still open?" / "이 제품 정의 빈칸 다 채우자"
- "is this plan ready to approve?" / "make the spec complete"

Don't invoke this for:
- **Implementation / regression** → that's the inner loop, `sdi-impl-loop`.
- **A single GWT restatement** → use `sdi-scenario`.
- **Round lifecycle** → use `sdi-round`.

---

## The daemon seam (HTTP, no CLI subcommand)

The v2 oracle endpoints have no `sdi` CLI subcommand — call the daemon HTTP API
directly. Resolve the base once:

```bash
SDI="http://127.0.0.1:$(cat ~/.cache/sdi/sdid.port)"
```

Missing port file ⇒ daemon down ⇒ stop and report it; never invent a port.
Plan / round / scenario lifecycle that *does* have a CLI (`sdi plan …`,
`sdi round …`) keeps using the CLI.

---

## The loop (D31 outer — loop-until-dry)

```
while not dry:
  1. scan      → GET /projects/<id>/oracle/verify
  2. collect   → OPEN markers, l1.uncovered pairs, uncovered flow-steps
  3. per point → §2a elimination (theoretically-wrong removed FIRST)
                 1 survivor  → auto_decided (answer auto:true + apply, closes OPEN)
                 2+ survivor → preference DecisionQuestion (options + rationale + recommend + +@)
  4. answer    → collect user answers (web/CLI) → compile deterministically into graph
  5. critic    → completeness-critic: any uncovered / uncovered-step / UNASKED point?
  6. dry?      → uncovered=0 AND unanswered=0 AND unasked=0 → exit; else continue
on exit: plan approve becomes possible (D34)
```

### 1. Scan — the deterministic verdict

```bash
curl -s "$SDI/projects/<PROJECT-ID>/oracle/verify"
```

returns
`{ l0:{facet_incomplete_nodes,dangling_edges,complete},
   l1:{uncovered_persona_capability_pairs,complete},
   questions:{open,clear}, l2:{enforced}, oracle_complete }`.

`oracle_complete` is necessary but **not sufficient** — it cannot see a decision
nobody encoded. The loop terminates on the critic's `converged`, not on this
flag alone (see step 5).

### 2. Collect decision-points

- **L0 OPEN markers** — list nodes, read each `open_markers_json`; every
  unresolved marker is a decision-point.
  ```bash
  curl -s "$SDI/projects/<PROJECT-ID>/ssot-nodes"
  ```
- **L1 uncovered** — `l1.uncovered_persona_capability_pairs` from verify; each
  is a (persona, capability) needing a UserFlow.
- **L2 uncovered steps** — for each confirmed flow, every `steps_json` step with
  no covering DetailScenario (see "Authoring" below).

### 3. Per decision-point — §2a elimination FIRST, then classify

Do **not** turn a point into a question before eliminating. Spawn the
**question-author** specialist on each point; it runs §2a (blacklist →
theoretically-wrong-with-cited-principle → "best now") and emits:

- **1 survivor → `fact` auto-decision** (NOT a question). The author answers it
  itself with `auto:true` and applies the decision to the node in one call.
- **2+ survivors → `preference` DecisionQuestion** — options as trade-off cards,
  each with a `rationale_md` ("why more right / where wrong"), one
  `is_llm_recommended`, plus the web `+@` free-text. Left for the user.
- **0 survivors → `premise-broken`** — reframe the point; do not author.

This is the anti-fake-question rule (D35): a 1-option "question" is forbidden.

### 4. Collect answers → deterministic compile

`fact` auto-decisions are already applied by question-author. For `preference`
questions, the user answers via the web surface (D36 batch or conversational)
or CLI; each answer compiles deterministically into the graph in the same call
that records it:

```bash
curl -s "$SDI/decision-questions/<QID>/answer" -H 'content-type: application/json' \
  -d '{"chosen_option_id":"<OPT-ID>","answered_by":"user","apply_node_id":"<NODE-ID>","resolve_marker_id":"<MARKER-ID>","apply_facets_json":"<decided facets JSON>"}'
```

(`free_text` replaces `chosen_option_id` for a `+@` answer.) The answer closes
the OPEN marker, writes the decided facets, and records provenance
(`generated_refs_json`) — one transaction. An answer that *produces a flow or
scenario* (not just facets) is followed by the authoring calls below, and the
produced rows are the answer's provenance.

### 5. Authoring from coverage gaps

- **Uncovered (Persona×Capability) → UserFlow.** Author the completed-service
  journey for that persona×purpose, then confirm it (L1 coverage counts only
  **confirmed** flows):
  ```bash
  FID=$(curl -s "$SDI/projects/<PROJECT-ID>/user-flows" -H 'content-type: application/json' \
    -d '{"short_code":"UF-3","persona_id":"<NODE-ID>","purpose":"<goal>","steps_json":"[{\"idx\":0,\"...\":\"...\"}]","covers_capabilities_json":"[\"<CAP-NODE-ID>\"]"}' | jq -r '.id')
  curl -s "$SDI/user-flows/$FID/confirm" -X POST
  ```
- **Uncovered flow-step → DetailScenario GWT.** Anchor the scenario to the flow
  step. The flow-anchor fields (`belongs_to_flow_id`, `covers_flow_step`) are
  **HTTP-only** — there is no `sdi scenario create` flag for them, so author
  flow-anchored DetailScenarios via the daemon scenario endpoint, keeping the
  D5 non-empty G/W/T discipline (see `sdi-scenario`):
  ```bash
  curl -s "$SDI/scenarios" -H 'content-type: application/json' \
    -d '{"plan_id":"<PLAN-ID>","short_code":"SC-FLOW-3-0","given":"…","when":"…","then":"…","belongs_to_flow_id":"<FID>","covers_flow_step":"0","confirmed":true}'
  ```
  (`covers_flow_step` is the step idx as a **string**.) Ordinary,
  non-flow-anchored scenarios keep using the `sdi scenario create` CLI.

### 6. Critic — the meta-completeness gate (the real terminator)

Spawn the **completeness-critic** after each turn. It re-scans verify and
sweeps the five lenses (normal / failure / boundary / concurrency / security)
for **unasked** decision-points. It returns:

```
gaps: { uncovered_persona_capability[], uncovered_flow_step[], unasked_decision_point[] }
verdict: "converged" | "gaps-remain"
```

- `gaps-remain` → feed each gap back: uncovered pairs → author flows (step 5),
  uncovered steps → author scenarios (step 5), unasked points → question-author
  (step 3). Loop.
- `converged` → all three lists empty **and** `oracle_complete:true`. Only now
  is the spec dry. "Unanswered = 0" alone is never enough — "**unasked = 0**" is
  the binding guarantee (D35).

---

## Linking the plan to flows + approving (D34)

The D34 approve gate fires only once the plan is **oracle-scoped** — i.e. it
targets ≥1 UserFlow. Link each confirmed target flow, then approve:

```bash
curl -s "$SDI/plans/<PLAN-ID>/target-flows/<FID>" -X POST
sdi plan approve <PLAN-ID>
```

A flow-scoped approve enforces, in the daemon: every targeted flow is
`confirmed`, every flow step is covered by a DetailScenario, and zero open
decision questions scoped to the plan/flows. It rejects with
`plan approve blocked (D34): N L2 gap(s), M open decision question(s) — …` —
that rejection means the loop is **not** dry yet; resolve the named gaps and
re-run the loop, do not retry approve blindly. A plan that targets **no** flow
falls back to the legacy D8 gate (`confirmed ≥ 1`) during the v1→v2 transition,
so always link target flows for a true oracle-scoped plan.

After approve, hand off to `sdi-impl-loop` for the inner (implementation)
convergence loop.

---

## Invariants

- **Loop-until-dry, no early exit.** Terminate on the critic's `converged`,
  never on "the user seems satisfied" or "this is taking long". Effort/time/diff
  are facts to report, not reasons to stop (product-quality-first §2).
- **No fake questions.** 1 survivor ⇒ auto-decision, not a question (D35).
- **Elimination before authoring.** §2a runs before any decision-point becomes a
  question (`solo-builder-context §2a`).
- **Deterministic compile.** Every answer applies through the daemon
  `answer` endpoint so the OPEN marker closes and provenance is recorded — never
  hand-edit `facets_json` to "close" a marker out of band.
- **Confirmed flows only count.** A draft UserFlow does not satisfy L1; confirm
  it before re-scanning.

---

## Failure recovery

| Symptom | Meaning | Recovery |
|---|---|---|
| `curl` connection refused / no port file | Daemon down. | `sdi daemon start`, then re-run. Never invent a port. |
| `plan approve blocked (D34): … L2 gap(s) …` | Uncovered flow steps or open scoped questions remain. | Resolve the named gaps via the loop (author scenarios / answer questions), then re-approve. |
| verify `oracle_complete:true` but critic says `gaps-remain` | An unasked decision-point exists the daemon can't see. | Trust the critic — author the missing question(s); meta-completeness outranks the flag. |
| `CONFLICT` on a `short_code` | Code already taken on this project/plan. | Bump the suffix within the same family. |
