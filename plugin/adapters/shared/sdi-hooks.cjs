// Shared hook bodies for the SDI Claude Code plugin shell.
//
// Adapters in adapters/claude/*.cjs delegate to functions exported here via
// 2-line shims. This module is the single home for install-gate logic, daemon
// HTTP calls, and hook semantics. Zero runtime deps (Node 20+ only).
//
// LM-8 invariant: plugin code may only write under `pluginRoot`. User data —
// SQLite, sockets, port file, logs — lives under XDG paths owned by the
// daemon. We READ the port file from `~/.cache/sdi/sdid.port` but never write
// it; the daemon writes it on startup.

'use strict';

const fs = require('fs');
const os = require('os');
const path = require('path');
const http = require('http');
const { spawn, spawnSync } = require('child_process');

// ────────────────────────────────────────────────────────────────────────────
// Constants

const BIN_ENV = 'SDI_BIN';
const BYPASS_ENV = 'SDI_BYPASS_HOOKS';
const DELEGATION_BYPASS_ENV = 'SDI_DELEGATION_BYPASS';
// v0.5 — disables the D26 pattern-shape advisory + D29 claim-overlap block.
// Routine bypass is a protocol violation (audit log records every use).
const V05_DISABLE_ENV = 'SDI_HOOK_V05_DISABLE';
const HOME_ENV = 'SDI_HOME';
// Heuristic: tool prompts that mention any of these tokens are presumed to
// be intentional multi-agent dispatch. The D26 advisory checks for an
// active pattern only when the orchestrator's intent is multi-agent.
const PATTERN_INTENT_TOKENS = [
  'specialist team',
  'parallel',
  'swarm',
  'graph review',
  'fan-out',
  'fan out',
  'agents-as-tools',
  'multi-agent',
];

function xdgHome() {
  return process.env[HOME_ENV] || os.homedir();
}
function portFile() {
  return path.join(xdgHome(), '.cache', 'sdi', 'sdid.port');
}
function pidFile() {
  return path.join(xdgHome(), '.cache', 'sdi', 'sdid.pid');
}
function stateDir() {
  return path.join(xdgHome(), '.local', 'state', 'sdi');
}
function hookLog() {
  return path.join(stateDir(), 'hook.log');
}

// Skill manifest — verified by the install gate fast-path. New skill entries
// here MUST be added in the same commit as the corresponding
// `skills/<name>/SKILL.md` AND the `skillsList` entry in
// `.claude-plugin/plugin.json` (same lock-step contract Clawket's
// `skill-file-integrity-on-install` rule enforces — three-way sync).
//
// Four skills, all `sdi-` prefixed:
//   - sdi-overview : cold-read orientation (entities, lifecycle, MCP map,
//                    failure codes)
//   - sdi-scenario : natural-language → GWT normalisation for scenarios
//   - sdi-round    : round create/activate/complete + mode, in-flight
//                    policy, disruption review, task auto-decomposition
//   - sdi-evidence : structured TaskEvidence at task done
const SDI_SKILLS = ['sdi-overview', 'sdi-scenario', 'sdi-round', 'sdi-evidence'];

// ────────────────────────────────────────────────────────────────────────────
// Plugin root resolution

function pluginRoot() {
  if (process.env.CLAUDE_PLUGIN_ROOT) return process.env.CLAUDE_PLUGIN_ROOT;
  // adapters/shared/sdi-hooks.cjs → plugin/
  return path.resolve(__dirname, '..', '..');
}

// ────────────────────────────────────────────────────────────────────────────
// Audit log (append to XDG state — appendHookLog is the single allowed write
// channel under user-data paths, mirroring Clawket's pattern).

function appendHookLog(event, payload) {
  try {
    fs.mkdirSync(stateDir(), { recursive: true });
    const line = JSON.stringify({ ts: new Date().toISOString(), event, ...payload }) + '\n';
    fs.appendFileSync(hookLog(), line);
  } catch {
    // Audit failure must not break the hook.
  }
}

function verifySdiSkills(root) {
  let ok = true;
  for (const name of SDI_SKILLS) {
    const skillFile = path.join(root, 'skills', name, 'SKILL.md');
    if (!fs.existsSync(skillFile)) {
      process.stderr.write(`[sdi] missing skill file: ${skillFile}\n`);
      ok = false;
    }
  }
  return ok;
}

// ────────────────────────────────────────────────────────────────────────────
// Binary resolution
//
// Priority:
//   1. SDI_BIN env (caller-supplied)
//   2. <pluginRoot>/bin/sdi (release-bundle layout)
//   3. workspace target/release/sdi
//   4. workspace target/debug/sdi
//   5. `which sdi` on PATH
//
// SDI ships as a single workspace, so cli/daemon/mcp share one source of
// truth (workspace [package].version). The hook is responsible only for
// finding the already-built `sdi`/`sdid` pair; build/distribution is the
// release pipeline's concern, not the install gate's.

function findWorkspaceRoot(startFrom) {
  // Walk up from pluginRoot looking for a Cargo.toml with [workspace].
  let dir = path.resolve(startFrom);
  for (let i = 0; i < 8; i++) {
    const cargoToml = path.join(dir, 'Cargo.toml');
    if (fs.existsSync(cargoToml)) {
      try {
        const raw = fs.readFileSync(cargoToml, 'utf8');
        if (raw.includes('[workspace]')) return dir;
      } catch {}
    }
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  return null;
}

function resolveSdiBin(root) {
  // 1. Explicit env override.
  if (process.env[BIN_ENV] && fs.existsSync(process.env[BIN_ENV])) {
    return { kind: 'env', bin: process.env[BIN_ENV] };
  }
  // 2. pluginRoot/bin/sdi (release tarball layout).
  const inPlugin = path.join(root, 'bin', 'sdi');
  if (fs.existsSync(inPlugin)) return { kind: 'plugin-bin', bin: inPlugin };
  // 3 & 4. Workspace target/release > target/debug.
  const ws = findWorkspaceRoot(root);
  if (ws) {
    const rel = path.join(ws, 'target', 'release', 'sdi');
    if (fs.existsSync(rel)) return { kind: 'target-release', bin: rel };
    const dbg = path.join(ws, 'target', 'debug', 'sdi');
    if (fs.existsSync(dbg)) return { kind: 'target-debug', bin: dbg };
  }
  // 5. PATH lookup.
  const r = spawnSync('sh', ['-c', 'command -v sdi'], { encoding: 'utf8' });
  if (r.status === 0) {
    const line = (r.stdout || '').trim();
    if (line && fs.existsSync(line)) return { kind: 'path', bin: line };
  }
  return null;
}

function resolveSdidBin(root, sdiBin) {
  // sdid sits next to sdi in every supported layout.
  if (sdiBin && sdiBin.bin) {
    const sib = path.join(path.dirname(sdiBin.bin), 'sdid');
    if (fs.existsSync(sib)) return sib;
  }
  const inPlugin = path.join(root, 'daemon', 'bin', 'sdid');
  if (fs.existsSync(inPlugin)) return inPlugin;
  const r = spawnSync('sh', ['-c', 'command -v sdid'], { encoding: 'utf8' });
  if (r.status === 0) {
    const line = (r.stdout || '').trim();
    if (line && fs.existsSync(line)) return line;
  }
  return null;
}

// Locate the sdi-web SPA bundle for the active install layout.
//
// Returns one of:
//   { state: 'ready',     dist: '<abs path to dist/>' }   // built bundle present
//   { state: 'buildable', source: '<abs path to web/>' }  // source present, dist missing
//   { state: 'absent' }                                   // no web/ tree shipped
//
// Lookup order (matches the daemon's `locate_web_bundle` in crates/daemon/src/router/mod.rs):
//   1. SDI_WEB_DIST env override
//   2. <pluginRoot>/web/dist (release bundle)
//   3. <workspace>/plugin/web/dist (dev mode)
function resolveWebDist(root) {
  const candidates = [];
  if (process.env.SDI_WEB_DIST) candidates.push(process.env.SDI_WEB_DIST);
  candidates.push(path.join(root, 'web', 'dist'));
  const ws = findWorkspaceRoot(root);
  if (ws) candidates.push(path.join(ws, 'plugin', 'web', 'dist'));

  for (const c of candidates) {
    const idx = path.join(c, 'index.html');
    if (fs.existsSync(idx)) return { state: 'ready', dist: c };
  }

  // dist absent — is the source tree present? Caller decides whether to nag.
  const sourceCandidates = [
    path.join(root, 'web'),
    ws ? path.join(ws, 'plugin', 'web') : null,
  ].filter(Boolean);
  for (const c of sourceCandidates) {
    if (fs.existsSync(path.join(c, 'package.json'))) {
      return { state: 'buildable', source: c };
    }
  }
  return { state: 'absent' };
}

// ────────────────────────────────────────────────────────────────────────────
// Install gate
//
// Fast-path 3-step:
//   1. binaries (sdi + sdid) resolved
//   2. skill file integrity (lock-step with .claude-plugin/plugin.json)
//   3. daemon /health responds
//
// On a miss we run setup, which here is "make sdid discoverable + spawn the
// daemon if needed". Distribution (release bundle download) is the release
// pipeline's concern; the install gate operates against whichever binary
// resolution wins from `resolveSdiBin`.

async function ensureInstalled(rootArg) {
  const root = rootArg || pluginRoot();

  const sdiBin = resolveSdiBin(root);
  const sdidBin = sdiBin ? resolveSdidBin(root, sdiBin) : null;
  const skillsOk = verifySdiSkills(root);

  if (sdiBin && sdidBin && skillsOk) {
    const healthy = await pingHealth().catch(() => false);
    if (healthy) return true;
    // Health failed — fall through to setup which will spawn the daemon.
  }

  return await runSetup({ sdiBin, sdidBin });
}

async function runSetup({ sdiBin, sdidBin }) {
  if (!sdiBin) {
    process.stderr.write(
      `[sdi] install gate: \`sdi\` binary not found. ` +
      `Set ${BIN_ENV}=/path/to/sdi or build the workspace with \`cargo build\`.\n`,
    );
    return false;
  }
  if (!sdidBin) {
    process.stderr.write(`[sdi] install gate: \`sdid\` binary not found alongside \`sdi\`.\n`);
    return false;
  }

  // Hand the resolved daemon path to `sdi daemon start`. SDI_DAEMON_BIN is the
  // primary contract in the Rust resolver (crates/cli daemon_cmd::resolve_sdid);
  // without it `sdi` falls back to a layout search (sibling, then
  // ../daemon/bin/sdid). In the dist tree `sdi` and `sdid` live in separate dirs
  // (bin/ vs daemon/bin/), so this env is what keeps the two resolvers in lock-step.
  process.env.SDI_DAEMON_BIN = sdidBin;

  // Spawn daemon if not running.
  const running = isDaemonRunning();
  if (!running) {
    const ok = await spawnDaemon(sdidBin);
    if (!ok) {
      process.stderr.write(`[sdi] install gate: daemon failed to start. See ${hookLog()}.\n`);
      return false;
    }
  }

  const healthy = await pingHealth().catch(() => false);
  if (!healthy) {
    process.stderr.write(`[sdi] install gate: daemon /health did not respond.\n`);
    return false;
  }

  appendHookLog('install_gate_ok', {
    sdi: sdiBin.bin,
    sdid: sdidBin,
    source: sdiBin.kind,
  });
  return true;
}

// ────────────────────────────────────────────────────────────────────────────
// Daemon lifecycle

function isDaemonRunning() {
  const pf = pidFile();
  const ptf = portFile();
  if (!fs.existsSync(pf) || !fs.existsSync(ptf)) return false;
  const pid = parseInt(fs.readFileSync(pf, 'utf8').trim(), 10);
  if (!Number.isFinite(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function spawnDaemon(sdidBin) {
  const pf = pidFile();
  const ptf = portFile();
  try {
    // Detached spawn; daemon will write its own pid/port file under XDG cache.
    fs.mkdirSync(path.dirname(pf), { recursive: true });
    const env = { ...process.env };
    // Hand the daemon a concrete SPA path so its ServeDir resolves the same
    // bundle the install gate is advertising. SDI_WEB_DISABLE=1 forces the
    // daemon to fall back to its built-in 404 (matches the user's opt-out).
    if (!env.SDI_WEB_DIST && env.SDI_WEB_DISABLE !== '1') {
      const web = resolveWebDist(pluginRoot());
      if (web.state === 'ready') env.SDI_WEB_DIST = web.dist;
    }
    const child = spawn(sdidBin, [], { stdio: 'ignore', detached: true, env });
    child.unref();
  } catch (err) {
    appendHookLog('daemon_spawn_error', { error: err.message });
    return false;
  }
  // Wait up to ~3s for port file to appear.
  const deadline = Date.now() + 3000;
  while (Date.now() < deadline) {
    if (fs.existsSync(ptf) && fs.existsSync(pf)) return true;
    await sleep(50);
  }
  return fs.existsSync(ptf);
}

function readDaemonPort() {
  const ptf = portFile();
  if (!fs.existsSync(ptf)) return null;
  const raw = fs.readFileSync(ptf, 'utf8').trim();
  const port = parseInt(raw, 10);
  return Number.isFinite(port) && port > 0 ? port : null;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// ────────────────────────────────────────────────────────────────────────────
// Daemon HTTP client

function daemonBase() {
  const port = readDaemonPort();
  if (!port) return null;
  return `http://127.0.0.1:${port}`;
}

function httpGet(urlString) {
  return new Promise((resolve, reject) => {
    let url;
    try {
      url = new URL(urlString);
    } catch (err) {
      reject(err);
      return;
    }
    const opts = {
      method: 'GET',
      hostname: url.hostname,
      port: url.port || 80,
      path: url.pathname + (url.search || ''),
    };
    const req = http.request(opts, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => {
        const text = Buffer.concat(chunks).toString('utf8');
        resolve({ status: res.statusCode || 0, text });
      });
    });
    req.on('error', reject);
    req.setTimeout(2000, () => req.destroy(new Error('daemon HTTP timeout')));
    req.end();
  });
}

function httpPostJson(urlString, body) {
  return new Promise((resolve, reject) => {
    let url;
    try {
      url = new URL(urlString);
    } catch (err) {
      reject(err);
      return;
    }
    const payload = Buffer.from(JSON.stringify(body), 'utf8');
    const opts = {
      method: 'POST',
      hostname: url.hostname,
      port: url.port || 80,
      path: url.pathname + (url.search || ''),
      headers: {
        'content-type': 'application/json',
        'content-length': payload.length,
      },
    };
    const req = http.request(opts, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => {
        const text = Buffer.concat(chunks).toString('utf8');
        resolve({ status: res.statusCode || 0, text });
      });
    });
    req.on('error', reject);
    req.setTimeout(2000, () => req.destroy(new Error('daemon HTTP timeout')));
    req.write(payload);
    req.end();
  });
}

async function pingHealth() {
  const base = daemonBase();
  if (!base) return false;
  try {
    const r = await httpGet(`${base}/health`);
    return r.status === 200;
  } catch {
    return false;
  }
}

async function getJson(pathAndQuery) {
  const base = daemonBase();
  if (!base) return null;
  try {
    const r = await httpGet(`${base}${pathAndQuery}`);
    if (r.status < 200 || r.status >= 300) return null;
    return JSON.parse(r.text);
  } catch {
    return null;
  }
}

async function postJson(pathAndQuery, body) {
  const base = daemonBase();
  if (!base) return null;
  try {
    const r = await httpPostJson(`${base}${pathAndQuery}`, body);
    if (r.status < 200 || r.status >= 300) return null;
    return r.text ? JSON.parse(r.text) : {};
  } catch {
    return null;
  }
}

// PRD §5.4: PostToolUse and SubagentStart/Stop record events as Task evidence
// candidates. The durable record is the daemon's `/activity` feed (Phase A
// `collab::record_activity`). We also keep the audit log on stderr-state path
// as a fallback so a transient daemon hiccup doesn't lose the signal.
async function recordActivity({ projectId, kind, summary, entityId, payload }) {
  if (!projectId) return;
  const body = { project_id: projectId, kind, summary };
  if (entityId) body.entity_id = entityId;
  if (payload && Object.keys(payload).length > 0) body.payload = payload;
  await postJson('/activity', body);
}

// ────────────────────────────────────────────────────────────────────────────
// Project / Plan / Task context resolution

async function projectByCwd(cwd) {
  const json = await getJson(`/projects/by-cwd?cwd=${encodeURIComponent(cwd)}`);
  return json && json.project ? json.project : null;
}

async function activePlanForProject(projectId) {
  const json = await getJson(`/projects/${encodeURIComponent(projectId)}/plans/active`);
  return json && json.plan ? json.plan : null;
}

async function inFlightTasks(planId) {
  const json = await getJson(`/plans/${encodeURIComponent(planId)}/tasks/in-flight`);
  return json && Array.isArray(json.tasks) ? json.tasks : [];
}

async function getTask(taskId) {
  const json = await getJson(`/tasks/${encodeURIComponent(taskId)}`);
  return json || null;
}

function readActiveTaskHint() {
  // The active task hint is set by Claude Code via env (analogous to Clawket).
  // SDI_ACTIVE_TASK can also be set explicitly.
  return process.env.SDI_ACTIVE_TASK || process.env.CLAUDE_ACTIVE_TASK || null;
}

// ────────────────────────────────────────────────────────────────────────────
// D21 — Mandatory delegation gate
//
// Orchestrator (main session, no agent_id in hook payload) is forbidden from
// calling execution tools. PreToolUse blocks Edit/Write/MultiEdit/NotebookEdit
// outright, and blocks Bash unless the command matches a read-only whitelist.
// Sub-agents (agent_id present) are allowed if their agent_type is registered
// in plugin/agents/. PRD §2 D21 + §5 Layer 1.5.

function isExecutionTool(toolName) {
  return /^(Edit|Write|MultiEdit|NotebookEdit)$/.test(toolName);
}

// Read-only Bash whitelist. Conservative on purpose: shell metacharacters
// (`;`, `&`, `|`, `<`, `>`, `` ` ``, `$`, `(`, `)`) disqualify the command —
// compound or substituting commands must go through a specialist. The verb
// (first token) must match an allow-list, and verb-specific subcommand
// restrictions apply (e.g. `cargo check` allowed, `cargo build` not — builds
// produce artifacts and should be delegated to test-runner).
function isReadOnlyBash(cmd) {
  if (!cmd || typeof cmd !== 'string') return false;
  const trimmed = cmd.trim();
  if (!trimmed) return false;
  // Disallow any shell metacharacter that can chain, substitute, or redirect.
  if (/[;&|<>`$()]/.test(trimmed)) return false;
  const tokens = trimmed.split(/\s+/);
  const verb = tokens[0];
  if (verb === 'git') {
    return /^(status|log|diff|show|branch|remote|config|rev-parse|describe|ls-files|blame|tag)$/.test(
      tokens[1] || '',
    );
  }
  if (verb === 'cargo') {
    return /^(check|clippy|fmt|tree|metadata|--version|-V)$/.test(tokens[1] || '');
  }
  if (verb === 'pnpm' || verb === 'npm') {
    // Only allow read-only/analysis subcommands. Script runners (run/exec/etc.)
    // can do anything, so they must be delegated.
    return /^(list|ls|view|outdated|--version|-v)$/.test(tokens[1] || '');
  }
  if (verb === 'find') {
    return !/-(delete|exec|execdir|ok|okdir)\b/.test(trimmed);
  }
  if (verb === 'node') {
    return /^(--version|-v)$/.test(tokens[1] || '');
  }
  // SDI's own management CLI (sdi / sdid) is exempt from D21 unconditionally.
  // D21's purpose is to force coding/file-mutation work onto specialist
  // sub-agents; SDI CLI invocations are workflow management against SDI's own
  // SQLite (scenarios, plans, rounds, daemon control) — orthogonal to the
  // delegation gate. Subcommand-level safety for destructive operations
  // (e.g. `sdi project delete`) belongs in the daemon's confirmation prompt,
  // not in this verb whitelist. Without this exemption the main session
  // cannot bootstrap any workflow (the very first `sdi plan create` would
  // be blocked), which is the bootstrap deadlock the v0.5 cleanup is fixing.
  if (verb === 'sdi' || verb === 'sdid') return true;
  return /^(ls|cat|head|tail|grep|rg|wc|file|which|pwd|echo|stat|env|printenv|date|uname|hostname|whoami|tree)$/.test(
    verb,
  );
}

const _agentRegistryCache = new Map();
function loadAgentSpecRegistry(root) {
  if (_agentRegistryCache.has(root)) return _agentRegistryCache.get(root);
  const result = new Set();
  const dir = path.join(root, 'agents');
  try {
    if (fs.existsSync(dir) && fs.statSync(dir).isDirectory()) {
      for (const entry of fs.readdirSync(dir)) {
        if (!entry.endsWith('.md')) continue;
        try {
          const raw = fs.readFileSync(path.join(dir, entry), 'utf8');
          const fm = raw.match(/^---[\r\n]+([\s\S]*?)^---/m);
          if (!fm) continue;
          const nameMatch = fm[1].match(/^name:\s*(\S+)\s*$/m);
          if (nameMatch) result.add(nameMatch[1]);
        } catch {}
      }
    }
  } catch {}
  _agentRegistryCache.set(root, result);
  return result;
}

function emitDeny(reason) {
  process.stdout.write(
    JSON.stringify({
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        permissionDecision: 'deny',
        permissionDecisionReason: reason,
      },
    }) + '\n',
  );
}

// ────────────────────────────────────────────────────────────────────────────
// D26 — Pattern shape advisory (PRD §5 Layer 2.6)
//
// When the orchestrator spawns specialists via Agent/Task, we ask the daemon
// whether an active CollaborationPattern row exists. We do NOT block: D27's
// server-side gate auto-creates a `direct` row if absent. The advisory just
// surfaces the auto-fallback to the user so the L3 cap (and red badge) isn't
// silently inherited.
//
// Pattern shape validation (steps ≥ 2, (name, stance) distinct ≥ 2, fan_out
// ≥ 2, peers ≥ 1) is enforced by the daemon at `pending → active`. The hook
// stays minimal — no client-side mirroring of the validator.

function looksLikePatternIntent(toolInput) {
  if (!toolInput || typeof toolInput !== 'object') return false;
  const prompt = String(toolInput.prompt || toolInput.description || '');
  if (toolInput.pattern_id) return true;
  if (!prompt) return false;
  const lc = prompt.toLowerCase();
  return PATTERN_INTENT_TOKENS.some((tok) => lc.includes(tok));
}

async function patternShapeAdvisory(toolName, toolInput) {
  if (!/^(Agent|Task)$/.test(toolName)) return;
  if (!looksLikePatternIntent(toolInput)) return;
  const active = await getJson('/patterns/active').catch(() => null);
  const rows = (active && Array.isArray(active.patterns) ? active.patterns : []) || [];
  if (rows.length === 0) {
    process.stderr.write(
      '[sdi] D26 advisory: no active CollaborationPattern found for this dispatch. ' +
        'Daemon will auto-create a `direct` row (anti-pattern marker, L3 cap). ' +
        'Materialise the right pattern first via /pattern create.\n',
    );
    appendHookLog('pre_tool_use_pattern_advisory', { tool: toolName, hint: 'no-active-pattern' });
  } else {
    appendHookLog('pre_tool_use_pattern_advisory', {
      tool: toolName,
      hint: 'active-pattern-found',
      pattern_count: rows.length,
    });
  }
}

// ────────────────────────────────────────────────────────────────────────────
// D29 — Resource claim gate (PRD §5 Layer 2.8)
//
// Edit/Write/NotebookEdit calls compute target_path then query the daemon's
// `/scenarios/active-claims` ledger. If any holding scenario_id differs from
// the agent's own active scenario, BLOCK with a structured JSON payload
// (`block: 'sdi_claim_overlap'`).
//
// Failure modes are PROCEED, not BLOCK:
// - daemon unreachable → warn on stderr, allow
// - no active claim on the calling agent → warn, allow (D26 advisory regime)
// - no overlap detected → audit "allow", allow

function targetPathOf(toolName, toolInput) {
  if (!toolInput || typeof toolInput !== 'object') return null;
  if (toolName === 'NotebookEdit') return toolInput.notebook_path || null;
  return toolInput.file_path || null;
}

async function resolveAgentScenarioId(agentId) {
  if (!agentId) return null;
  // Cheapest available path: the daemon does not yet index "agent → active
  // scenario", so we honor an explicit env override first. Real binding will
  // arrive when the daemon gains the AgentRun↔Scenario edge.
  if (process.env.SDI_ACTIVE_SCENARIO) return process.env.SDI_ACTIVE_SCENARIO;
  return null;
}

function pathOverlaps(targetPath, claimedResourcesJson) {
  // claimed_resources_json is an array of path globs (PRD §3.4). We do a
  // string-prefix + suffix-glob match — enough to catch the common
  // `crates/db/migrations/*.sql` shape without pulling in micromatch.
  if (!targetPath) return false;
  let globs = [];
  try {
    globs = JSON.parse(claimedResourcesJson || '[]');
  } catch {
    return false;
  }
  if (!Array.isArray(globs) || globs.length === 0) return false;
  for (const g of globs) {
    if (typeof g !== 'string' || !g) continue;
    if (g === targetPath) return true;
    if (g.endsWith('/*')) {
      const dir = g.slice(0, -2);
      if (targetPath.startsWith(dir + '/')) return true;
    } else if (g.endsWith('/**')) {
      const dir = g.slice(0, -3);
      if (targetPath.startsWith(dir + '/')) return true;
    } else if (g.includes('*')) {
      const re = new RegExp(
        '^' + g.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*') + '$',
      );
      if (re.test(targetPath)) return true;
    } else if (targetPath.startsWith(g + '/') || targetPath === g) {
      return true;
    }
  }
  return false;
}

async function claimOverlapGate(toolName, toolInput, agentId) {
  if (!/^(Edit|Write|NotebookEdit)$/.test(toolName)) return { block: false };
  const targetPath = targetPathOf(toolName, toolInput);
  if (!targetPath) return { block: false };

  const mine = await resolveAgentScenarioId(agentId);
  const ledger = await getJson('/scenarios/active-claims').catch(() => 'unreachable');
  if (ledger === 'unreachable' || ledger === null) {
    process.stderr.write(
      '[sdi] D29 advisory: daemon unreachable — claim overlap check skipped.\n',
    );
    appendHookLog('pre_tool_use_claim_skipped', { tool: toolName, reason: 'daemon-unreachable' });
    return { block: false };
  }
  const scenarios = (ledger && Array.isArray(ledger.scenarios) ? ledger.scenarios : []) || [];
  const holders = [];
  for (const s of scenarios) {
    const sid = s && (s.id || (s.scenario_id ?? null));
    if (!sid) continue;
    if (mine && sid === mine) continue;
    const cj = s.claimed_resources_json || (s.claimed_resources ? JSON.stringify(s.claimed_resources) : '[]');
    if (pathOverlaps(targetPath, cj)) {
      holders.push({ scenario_id: sid, claimed_resources_json: cj });
    }
  }
  if (holders.length === 0) {
    appendHookLog('pre_tool_use_claim_allow', { tool: toolName, file: targetPath });
    return { block: false };
  }
  return {
    block: true,
    payload: {
      block: 'sdi_claim_overlap',
      target_path: targetPath,
      my_scenario: mine,
      holders,
      hint: 'Wait or coordinate via /note handoff',
    },
  };
}

// ────────────────────────────────────────────────────────────────────────────
// Hook handlers
//
// The shims in adapters/claude/*.cjs are 2 lines and call these. Each handler
// returns a JSON payload for Claude Code (printed on stdout) or simply
// completes normally; throwing crashes the shim's wrap and exits 0 (allow).

// SessionStart: drive ensureInstalled, then inject minimal dashboard context.
async function runSessionStart(input) {
  const root = pluginRoot();
  const installed = await ensureInstalled(root);
  if (!installed) {
    // ensureInstalled has already explained on stderr. Don't inject context if
    // the daemon isn't up — downstream tools will fail loudly instead of
    // silently operating without state.
    return;
  }
  const cwd = (input && input.cwd) || process.cwd();
  const project = await projectByCwd(cwd);
  let banner = `# SDI session\n`;
  banner += `cwd: ${cwd}\n`;
  if (!project) {
    banner += `\nNo SDI project registered for this cwd.\n`;
    banner += `Register: \`sdi project create --key <KEY> --name "<name>" --cwd ${cwd}\`\n`;
  } else {
    banner += `project: ${project.name} (${project.key})\n`;
    const plan = await activePlanForProject(project.id);
    if (!plan) {
      banner += `No active plan. Create + approve one before starting work:\n`;
      banner += `  sdi plan create --project ${project.id} --title "<title>"\n`;
      banner += `  sdi scenario add --plan <PLAN-ID> --given "..." --when "..." --then "..."\n`;
      banner += `  sdi plan approve <PLAN-ID>\n`;
    } else {
      banner += `active plan: ${plan.title} (${plan.id})\n`;
      const tasks = await inFlightTasks(plan.id);
      if (tasks.length === 0) {
        banner += `In-flight tasks: 0\n`;
      } else {
        banner += `In-flight tasks (${tasks.length}):\n`;
        for (const t of tasks.slice(0, 5)) {
          banner += `  - ${t.id} ${t.title}\n`;
        }
      }
    }
  }
  // Dashboard SPA advisory — single line, opt-out via SDI_WEB_DISABLE=1.
  // Daemon-owned SPA at <port>/. Status mirrors the daemon's own resolver.
  if (process.env.SDI_WEB_DISABLE !== '1') {
    const web = resolveWebDist(root);
    if (web.state === 'ready') {
      const port = readDaemonPort();
      const url = port ? `http://127.0.0.1:${port}/` : '(daemon port unknown)';
      banner += `dashboard: ready at ${url}\n`;
    } else if (web.state === 'buildable') {
      banner += `dashboard: not built. Build once: \`pnpm --dir ${web.source} install && pnpm --dir ${web.source} build\` (or set SDI_WEB_DISABLE=1 to silence).\n`;
    }
    // state === 'absent' → no line; bundle simply not shipped.
  }
  appendHookLog('session_start', {
    cwd,
    project_id: project && project.id,
    web_state: process.env.SDI_WEB_DISABLE === '1' ? 'disabled' : resolveWebDist(root).state,
  });
  process.stdout.write(
    JSON.stringify({
      hookSpecificOutput: { hookEventName: 'SessionStart', additionalContext: banner },
    }) + '\n',
  );
}

// UserPromptSubmit: inject active task / plan context. Warn if no active task.
async function runUserPromptSubmit(input) {
  // Best-effort context injection. If ensureInstalled wasn't called yet
  // (e.g. session-start failed), all daemon calls return null and we noop.
  const cwd = (input && input.cwd) || process.cwd();
  const project = await projectByCwd(cwd);
  if (!project) return;
  const plan = await activePlanForProject(project.id);
  if (!plan) return;
  const tasks = await inFlightTasks(plan.id);
  const activeTaskId = readActiveTaskHint();
  let context = `# SDI context\n`;
  context += `project: ${project.key} (${project.id})\n`;
  context += `plan: ${plan.title} (${plan.id})\n`;
  if (activeTaskId) {
    const task = await getTask(activeTaskId);
    if (task) {
      context += `active task: ${task.id} ${task.title} [${task.status}]\n`;
    }
  } else if (tasks.length > 0) {
    context += `In-flight tasks (${tasks.length}):\n`;
    for (const t of tasks.slice(0, 5)) context += `  - ${t.id} ${t.title} [${t.status}]\n`;
    context += `\nNo active task pinned. Set one: \`sdi task update <TASK-ID> --status in_progress\`\n`;
  } else {
    context += `No in-flight tasks. Decompose scenarios into tasks first.\n`;
  }
  appendHookLog('user_prompt_submit', { project_id: project.id, plan_id: plan.id });
  process.stdout.write(
    JSON.stringify({
      hookSpecificOutput: { hookEventName: 'UserPromptSubmit', additionalContext: context },
    }) + '\n',
  );
}

// PreToolUse: three gates, evaluated in order.
//   1. D21 delegation gate — main session (no agent_id) cannot call execution
//      tools. Edit/Write/MultiEdit/NotebookEdit always blocked; Bash blocked
//      unless command matches the read-only whitelist. Sub-agents must have an
//      agent_type registered in plugin/agents/ (rogue-specialist guard).
//   2. Active-task gate — block Edit/Write/Bash without a task in_progress.
//   3. Autonomy gate (D14/D17/D18) — consult `/autonomy_policies/resolve`
//      for the active plan and downgrade to `permissionDecision: 'ask'`
//      when the effective mode is L3. The communication substrate (M1~M5)
//      stays mode-independent (D19); only the user-gate position moves.
async function runPreToolUse(input) {
  if (process.env[BYPASS_ENV] === '1') return;
  const toolName = (input && input.tool_name) || '';
  const watched = /^(Edit|Write|MultiEdit|Bash|NotebookEdit|Agent|Task|TeamCreate|SendMessage)$/.test(toolName);
  if (!watched) return;

  // Project scope gate. SDI hooks only enforce gates inside cwds that resolve
  // to a registered SDI project. When the daemon is unreachable or the cwd is
  // not registered, projectByCwd returns null and every downstream gate is
  // skipped — matching Clawket's allow-on-`isProjectDisabled`/missing-context
  // pattern (claude-hooks.cjs:2074-2077) so the hook never bleeds onto repos
  // SDI does not own.
  const cwd = (input && input.cwd) || process.cwd();
  const project = await projectByCwd(cwd).catch(() => null);
  if (!project) {
    appendHookLog('pre_tool_use_skip', { tool: toolName, reason: 'cwd-not-in-sdi-project' });
    return;
  }

  // D21 — delegation gate.
  const agentId = (input && input.agent_id) || null;
  const agentType = (input && input.agent_type) || null;
  const isMain = !agentId;
  if (isMain && (isExecutionTool(toolName) || toolName === 'Bash')) {
    let bashCmd = null;
    let blocked = true;
    if (toolName === 'Bash') {
      bashCmd = String((input && input.tool_input && input.tool_input.command) || '');
      if (isReadOnlyBash(bashCmd)) {
        blocked = false;
        appendHookLog('pre_tool_use_delegation_allow', {
          tool: toolName,
          reason: 'read-only-bash',
          cmd: bashCmd.slice(0, 200),
        });
      }
    }
    if (blocked) {
      if (process.env[DELEGATION_BYPASS_ENV] === '1') {
        process.stderr.write(
          `[sdi] WARNING: ${DELEGATION_BYPASS_ENV}=1 — main session executing ${toolName}` +
            (bashCmd ? ` (cmd preview: "${bashCmd.slice(0, 80)}")` : '') +
            `. Routine bypass is a D21 protocol violation; audit log records every use.\n`,
        );
        appendHookLog('pre_tool_use_delegation_bypass', {
          tool: toolName,
          bash_cmd: bashCmd ? bashCmd.slice(0, 200) : null,
        });
      } else {
        const reason = bashCmd != null
          ? `[sdi] D21 delegation gate: main session may not run mutating Bash. ` +
            `Delegate to a specialist sub-agent via the Agent tool. ` +
            `cmd preview: "${bashCmd.slice(0, 80)}". ` +
            `One-shot override (audited): ${DELEGATION_BYPASS_ENV}=1.`
          : `[sdi] D21 delegation gate: main session may not call ${toolName}. ` +
            `Delegate to a specialist sub-agent via the Agent tool. ` +
            `One-shot override (audited): ${DELEGATION_BYPASS_ENV}=1.`;
        emitDeny(reason);
        appendHookLog('pre_tool_use_blocked', {
          tool: toolName,
          reason: 'delegation-gate',
          bash_cmd: bashCmd ? bashCmd.slice(0, 200) : null,
        });
        return;
      }
    }
  }
  // D21 — rogue specialist guard. Sub-agents must come from a registered spec.
  if (!isMain && agentType) {
    const registry = loadAgentSpecRegistry(pluginRoot());
    if (registry.size > 0 && !registry.has(agentType)) {
      emitDeny(
        `[sdi] D21 rogue-specialist: agent_type "${agentType}" is not registered in ` +
          `plugin/agents/. Register an AgentSpec entry or use one of: ` +
          `${Array.from(registry).sort().join(', ')}.`,
      );
      appendHookLog('pre_tool_use_blocked', {
        tool: toolName,
        reason: 'rogue-specialist',
        agent_id: agentId,
        agent_type: agentType,
      });
      return;
    }
  }

  // D26 advisory — non-blocking, surfaces missing pattern row to stderr so the
  // L3 auto-fallback isn't silently inherited. Honored even before the
  // active-task gate so the warning lands alongside any subsequent block.
  // SDI_HOOK_V05_DISABLE=1 turns off both v0.5 gates (D26 advisory, D29 block).
  const v05Disabled = process.env[V05_DISABLE_ENV] === '1';
  if (!v05Disabled) {
    await patternShapeAdvisory(toolName, (input && input.tool_input) || {}).catch(() => {});
  }

  // Active-task gate. Only direct file-mutation tools require an active task —
  // Bash / Agent / Task / TeamCreate / SendMessage are bootstrap-capable
  // (creating the first plan, registering specialists, spawning the first
  // session) and have their own enforcement through D21 + D26 + D29. Without
  // this narrowing, a fresh project deadlocks: making the first task requires
  // a task to already be in_progress.
  const activeTaskId = readActiveTaskHint();
  if (!activeTaskId && isExecutionTool(toolName)) {
    emitDeny(
      `[sdi] no active task — set one before mutating files. ` +
        `Run \`sdi task list\` and \`sdi task update <TASK-ID> --status in_progress\`, ` +
        `or set ${BYPASS_ENV}=1 to bypass.`,
    );
    appendHookLog('pre_tool_use_blocked', { tool: toolName, reason: 'no-active-task' });
    return;
  }

  // D29 — Resource claim overlap gate. Blocks Edit/Write/NotebookEdit when a
  // different scenario's active claim covers the target file path. Daemon
  // unreachable / no claim / no overlap → proceed.
  if (!v05Disabled) {
    const gate = await claimOverlapGate(toolName, (input && input.tool_input) || {}, agentId).catch(
      () => ({ block: false }),
    );
    if (gate && gate.block) {
      process.stdout.write(
        JSON.stringify({
          hookSpecificOutput: {
            hookEventName: 'PreToolUse',
            permissionDecision: 'deny',
            permissionDecisionReason:
              `[sdi] D29 claim overlap on ${gate.payload.target_path}. ` +
              `Holders: ${gate.payload.holders.map((h) => h.scenario_id).join(', ')}. ` +
              `${gate.payload.hint}.`,
          },
          sdiBlock: gate.payload,
        }) + '\n',
      );
      appendHookLog('pre_tool_use_blocked', {
        tool: toolName,
        reason: 'claim-overlap',
        task_id: activeTaskId,
        target_path: gate.payload.target_path,
        my_scenario: gate.payload.my_scenario,
        holders: gate.payload.holders,
      });
      process.exit(2);
    }
  }

  // Autonomy gate. Best-effort: if the daemon can't resolve an active plan or
  // policy rows yet, we silently allow — the gate exists to *raise* friction,
  // never to invent it. The project was already resolved by the entry-level
  // scope gate above; reuse it instead of round-tripping the daemon twice.
  const plan = await activePlanForProject(project.id).catch(() => null);
  if (plan) {
    const resolved = await getJson(
      `/autonomy_policies/resolve?project_id=${encodeURIComponent(project.id)}` +
        `&plan_id=${encodeURIComponent(plan.id)}`,
    ).catch(() => null);
    const mode = resolved && resolved.policy && resolved.policy.mode;
    if (mode === 'L3') {
      process.stdout.write(
        JSON.stringify({
          hookSpecificOutput: {
            hookEventName: 'PreToolUse',
            permissionDecision: 'ask',
            permissionDecisionReason:
              `[sdi] autonomy=L3 on plan ${plan.id} — confirm before applying ${toolName}. ` +
              `Lift with \`/autonomy set ${project.id} --scope plan --mode L4 --plan-id ${plan.id}\`.`,
          },
        }) + '\n',
      );
      appendHookLog('pre_tool_use_ask', {
        tool: toolName,
        task_id: activeTaskId,
        mode,
        plan_id: plan.id,
      });
      return;
    }
    appendHookLog('pre_tool_use_allow', {
      tool: toolName,
      task_id: activeTaskId,
      mode: mode || 'unspecified',
      plan_id: plan.id,
    });
    return;
  }
  appendHookLog('pre_tool_use_allow', { tool: toolName, task_id: activeTaskId });
}

// PostToolUse: record file paths touched by Edit/Write as evidence
// candidates on the active task (PRD §5.4 "변경을 Task 의 evidence 후보로
// 자동 기록"). The durable record is the daemon's append-only `/activity`
// feed (Phase A collab::record_activity). We mirror to the XDG-state audit
// log too — if the daemon round-trip fails, the audit log keeps the signal.
async function runPostToolUse(input) {
  const toolName = (input && input.tool_name) || '';
  if (!/^(Edit|Write|MultiEdit|NotebookEdit)$/.test(toolName)) return;
  const activeTaskId = readActiveTaskHint();
  if (!activeTaskId) return;
  const params = (input && input.tool_input) || {};
  const file = params.file_path || params.notebook_path || null;
  if (!file) return;
  appendHookLog('post_tool_use', { tool: toolName, task_id: activeTaskId, file });
  const cwd = (input && input.cwd) || process.cwd();
  const project = await projectByCwd(cwd).catch(() => null);
  if (!project) return;
  await recordActivity({
    projectId: project.id,
    kind: 'task.file_touched',
    summary: `${toolName} ${file}`,
    entityId: activeTaskId,
    payload: { tool: toolName, file },
  });
}

// SubagentStart: bind the sub-agent run to the active task (PRD §5.4
// "sub-agent 를 Task 에 바인딩"). The daemon `/activity` feed is the
// durable record; the audit log is the fallback.
async function runSubagentStart(input) {
  const activeTaskId = readActiveTaskHint();
  if (!activeTaskId) return;
  const agent = (input && input.subagent_type) || (input && input.agent_name) || 'unknown';
  appendHookLog('subagent_start', { task_id: activeTaskId, agent });
  const cwd = (input && input.cwd) || process.cwd();
  const project = await projectByCwd(cwd).catch(() => null);
  if (!project) return;
  await recordActivity({
    projectId: project.id,
    kind: 'subagent.start',
    summary: `subagent start: ${agent}`,
    entityId: activeTaskId,
    payload: { agent },
  });
}

// SubagentStop: append result summary to the activity feed against the
// active task (PRD §5.4 "결과 요약 적재").
async function runSubagentStop(input) {
  const activeTaskId = readActiveTaskHint();
  if (!activeTaskId) return;
  const agent = (input && input.subagent_type) || (input && input.agent_name) || 'unknown';
  const result = (input && input.result) || (input && input.summary) || null;
  appendHookLog('subagent_stop', { task_id: activeTaskId, agent, result });
  const cwd = (input && input.cwd) || process.cwd();
  const project = await projectByCwd(cwd).catch(() => null);
  if (!project) return;
  const summary =
    typeof result === 'string' && result.length > 0
      ? `subagent stop: ${agent} — ${result.slice(0, 200)}`
      : `subagent stop: ${agent}`;
  await recordActivity({
    projectId: project.id,
    kind: 'subagent.stop',
    summary,
    entityId: activeTaskId,
    payload: { agent, result },
  });
}

// ────────────────────────────────────────────────────────────────────────────
// Input parsing (Claude Code pipes hook payload as JSON on stdin).

function readStdinJsonSync() {
  try {
    const data = fs.readFileSync(0, 'utf8');
    if (!data) return {};
    return JSON.parse(data);
  } catch {
    return {};
  }
}

// Wrappers expose the (input) → Promise/void contract used by the shims.
async function dispatchAsync(fn) {
  const input = readStdinJsonSync();
  await fn(input);
}

function dispatchSync(fn) {
  const input = readStdinJsonSync();
  fn(input);
}

// ────────────────────────────────────────────────────────────────────────────
// Public surface

module.exports = {
  // Install gate
  ensureInstalled,
  pluginRoot,
  // Hook bodies
  runSessionStart: () => dispatchAsync(runSessionStart),
  runUserPromptSubmit: () => dispatchAsync(runUserPromptSubmit),
  runPreToolUse: () => dispatchAsync(runPreToolUse),
  runPostToolUse: () => dispatchAsync(runPostToolUse),
  runSubagentStart: () => dispatchAsync(runSubagentStart),
  runSubagentStop: () => dispatchAsync(runSubagentStop),
  // Internals exposed for tests
  _internals: {
    resolveSdiBin,
    resolveSdidBin,
    resolveWebDist,
    findWorkspaceRoot,
    verifySdiSkills,
    daemonBase,
    appendHookLog,
    SDI_SKILLS,
  },
};
