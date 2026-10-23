# Retry cause identity

Retry accounting identifies the primary actionable cause of a failed execution independently of its failure classification. Classification determines the applicable policy; cause identity determines which retry counter is charged. Every failed execution charges one counter once.

## Primary diagnostic

Supported compiler output uses the first actionable diagnostic in source order. Diagnostic codes and meaningful property and type names remain part of its identity. Startup and assertion output uses the substantive inner error rather than an outer test or panic wrapper. Socket path limits, incompatible artifact protocols or schemas, and database ownership failures have distinct identities.

Normalization removes volatile absolute roots and temporary paths, source line and column numbers, UUIDs, timestamps, addresses and ports. Task identifiers and wrapper test or method names do not enter the identity. The same primary diagnostic retains its identity when secondary diagnostics change.

The identity describes one primary diagnostic, not every latent defect in a report. A different first actionable diagnostic may select a different cause. Retry identity never hashes the entire diagnostic set.

Supported identities use the `sig:v2:` namespace. If supported diagnostic evidence is unavailable, `sig:v2:fallback:` identifies an explicitly coarse fallback over the scenario contract, failure classification and stable broad error class. Arbitrary output text, stack positions and caller labels cannot make the fallback more specific.

When a report exceeds the diagnostic scan limit, identity uses the coarse fallback and records truncated, unavailable diagnostic evidence. A scanned tail cannot establish the first actionable cause. The complete original report and its digest remain preserved, and transitions between complete and truncated scans retain outstanding retry authority through the same conservative compatibility rules.

## Evidence and accounting authority

New run records expose optional `failure_identity` provenance: the computed identity version, extraction basis and normalized primary diagnostic, the signature actually booked, and any compatibility bridge or caller hint. `failure_signature` always names the counter actually charged. A computed diagnosis and a booked compatibility counter may differ.

For structured executions, daemon-derived identity controls accounting. A supplied signature is an advisory hint retained as provenance. It cannot replace the computed identity or claim computed provenance. Manual runs without execution evidence retain their explicit caller-signature contract and remain separate from computed execution identity.

The `sig:v2:` namespace is reserved for computed identities; manual caller input resembling that namespace is charged under a `caller:` prefix.

Run completion, provenance persistence and counter booking form one transaction. Repeated or concurrent completion cannot finish the run twice or leave a finished failure without its corresponding debit.

## Conservative compatibility

Original reports, digests, failure classifications, signatures and cumulative totals remain unchanged. Adding nullable cause provenance does not infer diagnoses for historical runs.

A precise counter requires matching computed provenance from its first run; its signature prefix or a later bridged diagnosis cannot establish that authority.

An outstanding legacy, caller-supplied or coarse fallback counter in the same plan, scenario and failure class remains accounting authority for a newly diagnosed execution. Selection prefers an exhausted counter, then the highest outstanding count, then lexical signature order. It does not depend on test, frame, task or method names.

If the current report lacks diagnostic evidence, the same selection also considers outstanding diagnosed counters of that class. Losing diagnostic detail cannot grant a fresh retry allowance. A later supported diagnosis remains subject to any outstanding coarse authority.

Each new execution increments only its selected counter and cumulative total. Compatibility creates no synthetic failures, inherited counters or duplicated totals. Zero-count counters do not supply compatibility authority. Fresh diagnosed causes use independent versioned counters when no outstanding compatibility authority applies.

Existing exhausted counters continue to block work through the existing scenario gate. Historical coarse restrictions remain until the existing decision-authorized release procedure explicitly clears them. Identity support performs no automatic release, reset, historical repartition, classification change or retry-cap increase.

## Verification contract

Verification covers distinct startup and compiler diagnoses, stable identity under volatile changes and secondary diagnostic churn, conservative unknown transitions in both directions, caller-hint isolation, deterministic compatibility selection, unchanged history and totals, persistent exhaustion across restarts, and atomic completion and debit.
