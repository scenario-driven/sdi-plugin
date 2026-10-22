# Project command exceptions

Projects declare shell-command exceptions in `.sdi/settings.json`. Personal
exceptions use `.sdi/settings.local.json`, which the project excludes from Git.
Both Claude Code and Codex adapters use the same policy contract. Native host
permission files remain independent and are not imported.

```json
{
  "permissions": {
    "allow": [
      "orca skills get orchestration",
      "orca orchestration send *"
    ]
  }
}
```

## Ownership and loading

Users own the execution and write effects of allowed commands. Exceptions do
not require effect declarations, role declarations, or an additional approval
registry. SDI guarantees matching boundaries, not the safety of an allowed tool.

The owning root is the deepest registered project cwd containing the invocation
cwd after real-path normalization. Configuration is read only from that root;
caller subdirectories, unrelated projects, and ancestor policies do not supply
exceptions. Each invocation reloads both files and combines their allow arrays
as a deduplicated union. Neither file overrides or removes the other's entries.
Missing files add no permissions and preserve existing behavior.

A present file must contain a JSON object. An optional `permissions` object may
contain an `allow` array of valid nonempty strings. Malformed JSON, invalid
field types, invalid rules, and unreadable files produce a diagnostic naming
the file and reason; that invocation receives no policy permissions from
either file. Existing independently allowed behavior remains available.

Each invocation reads, parses, and validates the complete contents of both
files before granting any exception. Partial writes and invalid content grant
no policy permissions; a previous successful load is never a fallback. A
complete JSON file published by atomic rename is consumed on the next read.
The two files do not form a transaction, so concurrent updates can produce a
valid combination of independently read file versions.

## Matching

Rules use the existing shell lexer to compare parsed command arguments.
Quotation groups argument values; it does not become part of the value.

| Rule | Meaning |
|---|---|
| `tool action value` | Exact executable and argument vector |
| `tool action *` | Exact executable and argument prefix, with zero or more remaining arguments |

Only an unquoted terminal space-star suffix is a wildcard. Regex patterns,
middle wildcards, and executable-name substring matching are unsupported.
Quoted literal values retain their ordinary argument meaning. Rules cannot
contain outer shell chaining, pipelines, substitutions, or redirection.

Every segment of a supported compound command is checked independently under
the normal classifier and policy. A match cannot absorb another command or
redirection. Unsupported outer syntax remains rejected. Nested shell payloads
receive no recursive policy exception: a shell invocation is allowed only if
the user explicitly matches that invocation and its argument vector.

Directory-changing segments cannot carry an earlier project's permissions into
another root. The evaluator tracks provable literal directory changes or
declines policy permission when the effective project scope is uncertain.

## Gate boundary

A policy match overrides only the main session's shell-command delegation
denial. The classifier's `readOnly` result remains truthful and separate from
the policy permission. A user may allow a mutating command such as message
delivery or directory creation without describing it as a read.

The exception does not grant `Edit` or `Write`, impersonate a specialist,
authenticate an external worker, bind a task identity, or disable active-task,
resource-claim, autonomy, and other applicable gates. Unconfigured main
restrictions retain their current behavior. External implementation uses native
specialist dispatch and independent verification; command exceptions authorize
communication, not external-worker identity claims.

Diagnostics and authorization records identify the matched rule and source
file. They do not add raw runtime argument values or secrets to logs. Existing
bounded event retention semantics continue to apply.

## Acceptance baseline

1. Missing policy preserves classifier and gate decisions.
2. Shared and personal files merge, reload, reject invalid configuration, and
   remain isolated to the registered real-path project root.
3. Exact and terminal-prefix rules honor argument boundaries, quotations,
   supported compound segments, and unsupported syntax restrictions.
4. A matching mutating command passes the delegation exception without changing
   `readOnly`; remaining gates, main edits, and native identities remain intact
   in both host adapters, with bounded diagnostics.
5. An external supported host demonstrates policy-authorized communication,
   native task delegation, implementation, independent verification, and report
   without disabling gates or accepting fabricated specialist identities.
6. The distributed release includes the shared policy module, adapter wiring,
   documentation, aligned versions, and evidence for the acceptance baseline.
