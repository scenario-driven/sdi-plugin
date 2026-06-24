# Planning cold-start — requirement → oracle document bootstrap

Status: design (P1 implemented in plugin layer; P2/P3 pending)
Scope: the **front half** of the spec-convergence loop (D31–D36) — producing the
initial product-definition documents a new project has none of.

---

## Problem

The converge loop (`sdi-converge`) fills two kinds of blanks: **coverage**
(UserFlows / DetailScenarios) and **decisions** (DecisionQuestions). Both are
computed *against an existing oracle* — they assume Persona and Capability nodes
already exist.

A brand-new project has none. With zero nodes the deterministic verdict is
vacuously "complete":

- `crates/daemon/src/router/oracle.rs:476-486` — `l1_uncovered` is the double
  loop `for persona in personas { for capability in capabilities { … } }`. With
  `personas = []` and `capabilities = []` it yields zero pairs.
- `crates/daemon/src/router/oracle.rs:455-456` — `facet_incomplete` and
  `dangling` are both 0 when there are no nodes/edges.
- `crates/daemon/src/router/oracle.rs:508` —
  `oracle_complete = l0_complete && l1_complete && questions_clear` →
  `true && true && true` = **`true`**.

So an empty project reports `oracle_complete: true`. The engine that exists to
drive a product definition to completeness instead waves an empty project
through. There is no path from a requirement prompt to a first set of documents.

This is not a data problem of any one project — it is a missing stage in the
tool.

---

## The loop (target)

```
requirement prompt  ("I want to build X")
  └─ ① document    : scaffold + interview-author the initial product-definition
  │                   nodes; everything undecided is marked OPEN / unverified
  │        ── MISSING (this work) ──
  └─ ② oracle      : the node graph is the standard "what is still missing" is
  │                   judged against
  │        ── implemented: oracle/verify + completeness-critic ──
  └─ ③ question    : each OPEN / coverage gap becomes an interview question;
  │                   the answer closes the OPEN and compiles into the graph
  │        ── implemented: question-author + D36 web question cards / chat ──
  └─ ④ more prompt : the user adds intent → derive more nodes → back to ①
           ── loop until completeness-critic returns unasked = 0 ──
```

Stages ②③④ exist. Stage ① is the gap this design closes.

---

## Design precedent (reference level, not a runtime dependency)

`ssot-studio` (a separate tool in the user's monorepo) already does stage ① for
its own document model. It is studied here **only as a precedent for the shape
and the quality bar** — SDI does not call it, import its data, or depend on it at
runtime. The two ideas worth carrying over:

1. **`init` = scaffold, not one-shot generation.** It lays down the kind
   taxonomy, the per-kind document skeleton, and the schema — then leaves
   structured blanks. Documents are filled incrementally, not dumped in one call.

2. **`add <Kind>` is itself an interview.** Authoring a node means: extract what
   is verifiable from code/docs (record at `confidence: inferred`), and *ask the
   human* what only they know — purpose, which persona, the invariant's
   justification — leaving it `confidence: unverified` with an OPEN marker until
   answered. A blank is a first-class state, never an error to paper over.

The same confidence ladder maps onto SDI's existing node fields:
`unverified` (scaffolded, empty) → `inferred` (filled from code, unconfirmed) →
`high` (human-verified).

---

## Taxonomy decision — rich, not minimal

The product-definition documents must be *sufficient* — you cannot interview
against, or implement from, requirements that were never written down. A single
`Persona × Capability` axis is too thin to be a sufficient definition.

The bootstrap therefore authors a **rich** node taxonomy (the kinds the
precedent uses): Persona, Capability, Domain, Concept, SystemComponent,
Integration, Invariant, Decision, Screen, Endpoint — plus **Flow** for the
larger persona journey. The daemon's current verify only judges completeness on
`Persona × Capability` (L1) and facet/link presence (L0); raising the bar to a
per-kind, multi-axis completeness standard is **P2** (daemon work). P1 authors
the rich taxonomy using the existing generic `kind` field, so the richer nodes
land now even though the stricter verdict arrives in P2.

### Flow ↔ GWT — two layers, already wired

The "large scenario" — *a persona role doing such-and-such across a journey* —
is the **Flow** layer, distinct from and above the detailed Given/When/Then
**DetailScenario**. SDI already carries the join: `scenarios.belongs_to_flow_id`
and `scenarios.covers_flow_step` anchor a GWT scenario to one step of a Flow.
The bootstrap authors Flows (the journeys); converge's stage ③/coverage authors
the per-step GWT scenarios underneath them. P3 makes this two-layer descent
explicit in the converge step-0 hand-off.

---

## What P1 ships (plugin layer, no Rust change)

Reuses the existing daemon HTTP surface
(`crates/daemon/src/router/oracle.rs`): `POST /projects/:id/ssot-nodes`,
`POST /projects/:id/user-flows`, `POST /projects/:id/ssot-edges`,
`GET /projects/:id/oracle/verify`, `GET /projects/:id/ssot-nodes`.

- **`plugin/agents/oracle-author.md`** — the interview-driven authoring
  specialist. Extracts the verifiable, asks the human-only, writes nodes/flows
  with OPEN markers and the right confidence. Authoring only; it does not turn
  blanks into questions (`question-author`) or judge gaps (`completeness-critic`).

- **`plugin/skills/sdi-init/SKILL.md`** — the cold-start orchestration. Detects
  an empty oracle (and the vacuous-complete trap), runs `oracle-author` to lay
  the backbone from the requirement, then hands off to `sdi-converge`.
  Idempotent: a non-empty oracle is left untouched.

Boundary: **`sdi-init` is the front half (produce the documents);
`sdi-converge` is the back half (fill the blanks).**

---

## Phases

| Phase | Layer | Content |
|---|---|---|
| **P1** | plugin | `oracle-author` agent + `sdi-init` skill; rich taxonomy authored via existing write API; OPEN/confidence markers. No daemon change. |
| **P2** | daemon (Rust) | Raise the completeness standard: per-kind multi-axis facet completeness + the richer taxonomy reflected in `oracle/verify`, so a sufficient definition is *enforced*, not just authored. Fixes the vacuous-complete verdict for the empty case too. |
| **P3** | plugin + daemon | Flow ↔ GWT two-layer descent made explicit in a converge **step-0**: bootstrap Flows, then drive per-step DetailScenario coverage; wire `sdi-init` → `sdi-converge` as one continuous planning entry. |
