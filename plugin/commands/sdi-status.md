---
description: Show SDI status — project, active plan, in-flight tasks, daemon health
argument-hint: (no arguments)
allowed-tools: Bash, Read
---

# /sdi-status — dashboard at a glance

Resolve the current cwd to its SDI project, surface the active plan, list
in-flight tasks, and report daemon health. This is read-only.

## What to do

1. Daemon health:
   ```bash
   sdi daemon status
   sdi doctor
   ```
   If the daemon is not running, suggest:
   ```bash
   sdi daemon start
   ```

2. Project + active plan:
   ```bash
   sdi project by-cwd "$(pwd)"          # → project.id
   sdi plan active <PROJECT-ID>         # → plan.id (404 if none)
   ```

3. Active round (if any):
   ```bash
   sdi round active <PLAN-ID>
   ```

4. In-flight tasks (per round):
   ```bash
   sdi task list <ROUND-ID>             # tasks for a round
   ```
   The MCP tool `get_plan_context` returns `tasks_in_flight` directly if you
   need the plan-scoped view in one call.

5. Summarize to the user as a short report:
   - Project key + name
   - Active plan title + status
   - Active round id + mode (or "no active round")
   - In-flight task ids + statuses
   - Daemon health line

## Failure modes

- No SDI project for the cwd → tell the user to register:
  ```bash
  sdi project create <KEY> "<name>" --cwd "$(pwd)"
  ```
- Daemon not responding → run `sdi doctor`, then `sdi daemon start`.

## Why not just call MCP?

The MCP tools (`sdi_get_plan_context`, etc.) are LLM-callable but show
**rag-scope knowledge only**. `/sdi-status` is the human-facing snapshot and
intentionally hits the daemon HTTP endpoints directly so it can include task
status, daemon liveness, and short codes that the rag-scoped MCP surface
filters out.
