---
name: sdi-init
description: Cold-start the spec for a new/empty project — turn a requirement prompt into the first product-definition documents (the oracle backbone) so the converge loop has something to drive. Detects an empty oracle (and its vacuous "complete" verdict), spawns oracle-author to scaffold Persona/Capability/Domain/Flow… nodes with OPEN markers from the requirement, then hands off to sdi-converge. Use when starting planning from "I want to build X" with no existing oracle.
---

# /init — the cold-start (front half of the planning loop)

`sdi-converge` fills the blanks of an existing product definition. But a new
project **has no definition to fill** — no Persona, no Capability, no Flow. This
skill produces that first definition from the requirement prompt, then hands the
result to converge. It is the **front half** (produce the documents);
`sdi-converge` is the **back half** (interview the blanks closed).

> Whole loop: requirement prompt → **① document (sdi-init)** → ② oracle (verify)
> → ③ question (converge / question-author) → ④ more prompt → repeat until
> `completeness-critic` returns `unasked = 0`. See
> `docs/design/planning-cold-start.md`.

## When to invoke

- "I want to build X" / "이걸 만들겠다" with no oracle yet.
- "start the spec / let's define this product".
- A `sdi-converge` run that scans an empty (or near-empty) oracle — converge
  should call this first instead of declaring a blank project complete.

Don't invoke for:
- A project that **already has** an oracle → go straight to `sdi-converge`.
- Restating one behaviour as GWT → `sdi-scenario`.

## The daemon seam (HTTP)

```bash
SDI="http://127.0.0.1:$(cat ~/.cache/sdi/sdid.port)"
```

Missing port file ⇒ daemon down ⇒ stop and report it; never invent a port.

## The cold-start

### 1. Detect the empty oracle (and the vacuous-complete trap)

```bash
curl -s "$SDI/projects/<PROJECT-ID>/ssot-nodes"        # → {"nodes":[…]}
curl -s "$SDI/projects/<PROJECT-ID>/oracle/verify"     # → … "oracle_complete": …
```

A new project returns `{"nodes":[]}` **and** `oracle_complete: true`. That
`true` is **vacuous** — `l1_uncovered` is the cross-product of zero personas and
zero capabilities, so it is empty, so the verdict reads "complete" while nothing
is defined (`crates/daemon/src/router/oracle.rs:476-508`). Do **not** trust it:
zero nodes ⇒ cold-start is required, regardless of the flag.

If nodes already exist, this skill is a no-op — hand off to `sdi-converge`
(idempotent: never overwrite an existing backbone).

### 2. Scaffold the backbone — spawn `oracle-author`

Hand the **requirement prompt + active plan/requirements** (and, optionally, the
codebase) to the `oracle-author` specialist. It authors the initial nodes by
interview:

- Personas, Capabilities, Domains, Concepts, Components, Integrations,
  Invariants, Screens, Endpoints — and **Flows** for the persona journeys.
- Code-verifiable facets at `confidence: inferred`; everything human-only as an
  **OPEN marker** at `confidence: unverified`.

The output is a backbone *full of explicit blanks* — that is correct. The blanks
are exactly what the back half interviews closed.

### 3. Hand off to `sdi-converge`

With a non-empty oracle, the existing loop takes over: `oracle/verify` now
reports real uncovered pairs, the OPEN markers are real decision-points, and
`question-author` turns each into an interview question (web cards / chat, D36).
`completeness-critic` decides when `unasked = 0`.

```
sdi-init  →  oracle-author (scaffold)  →  sdi-converge (interview the blanks)
   front half                                back half
```

## Invariants

- **Never declare a blank project complete.** Zero nodes ⇒ cold-start, not
  "done". The vacuous `oracle_complete: true` is a trap, not a verdict.
- **Idempotent.** A project with an existing oracle is left untouched; this skill
  only lays the *first* backbone.
- **Scaffold, do not finish.** The goal is a *sufficient backbone with marked
  blanks*, not a fully-decided spec. Deciding the blanks is the back half
  (`question-author`); judging sufficiency is `completeness-critic`. Authoring
  decisions here would conflate the halves.
- **Sufficient, not minimal.** Author the rich taxonomy (domains, concepts,
  invariants, flows), not just Persona×Capability — you cannot interview, or
  implement, against a definition that was never written
  (product-quality-first §4).
