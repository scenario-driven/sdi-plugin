// Shared hook bodies for the SDI Claude Code / Codex plugin shell.
//
// Host adapters delegate to functions exported here via tiny shims. This
// module is the single home for install-gate logic, daemon HTTP calls, and hook
// semantics. Zero runtime deps (Node 20+ only).
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
// Marker-file bypass surface — daemon-friendly substrate for emergency hook
// bypass. Inline `VAR=1 cmd` env prefixes never reach this hook (Claude Code
// spawns PreToolUse before any user shell expands the prefix), so env is the
// wrong substrate for one-shot overrides. A user-writable file in XDG cache
// works for both sides. Authored by `sdi bypass arm` (the CLI is on the D21
// read-only Bash whitelist so the main session can call it directly); the
// hook consumes it once, then deletes it.
//
// One marker unlocks every mutating gate: D21 delegation, active-task, D29
// claim overlap. Splitting the surface per gate would re-introduce the
// self-deadlock the marker exists to fix (user disarms one gate, next one
// blocks them again).
//
// #14 — KNOWN LIMITATION (machine-global scope under concurrent agents). The
// marker is a single machine-global file, so a marker armed by one agent can
// be consumed by a concurrently-waking PreToolUse of a *different* agent on
// the same machine. Per-(session, agent) scoping is NOT implementable: Claude
// Code exposes `session_id` / `agent_id` ONLY in the hook payload JSON, never
// as environment variables (official docs: code.claude.com/docs/en/hooks —
// "There is no $CLAUDE_SESSION_ID/$CLAUDE_AGENT_ID env var"). `sdi bypass arm`
// runs in the agent's Bash shell, which therefore cannot read its own
// session/agent id to key the marker, and a lease-token scheme fails for the
// same reason (lease acquisition is also a CLI call with no agent id). So
// neither a file nor a daemon-mediated marker can correlate arm↔consume by
// agent. This is a platform constraint, not a band-aid.
//
// Mitigation: the primary driver of routine bypass — the old active-task gate
// reading the unsatisfiable `SDI_ACTIVE_TASK` env (#9) — is gone; the gate now
// reads daemon state, so specialists no longer arm a marker per mutation.
// Routine bypass is therefore rare. For a deliberately-windowed emergency
// during a concurrent run, prefer the startup-time `SDI_HOOK_V05_DISABLE`
// switch over the one-shot marker.
function bypassOnceFile() {
  return path.join(xdgHome(), '.cache', 'sdi', 'bypass-once');
}

// Skill manifest — verified by the install gate fast-path. New skill entries
// here MUST be added in the same commit as the corresponding
// `skills/<name>/SKILL.md` AND the `skillsList` entry in
// `.claude-plugin/plugin.json` (same lock-step contract Clawket's
// `skill-file-integrity-on-install` rule enforces — three-way sync).
//
// Six skills, all `sdi-` prefixed:
//   - sdi-overview  : cold-read orientation (entities, lifecycle, MCP map,
//                     failure codes)
//   - sdi-scenario  : natural-language → GWT normalisation for scenarios
//   - sdi-round     : round create/activate/complete + mode, in-flight
//                     policy, disruption review, task auto-decomposition
//   - sdi-evidence  : structured TaskEvidence at task done
//   - sdi-converge  : outer loop — spec convergence to the completeness oracle
//                     (D31/D34/D35); §2a elimination, auto-decide / ask, loop-until-dry
//   - sdi-impl-loop : inner loop — implementation convergence over rounds
//                     (D30/D31); bounded retry + auto round-advance on regression
const SDI_SKILLS = ['sdi-overview', 'sdi-scenario', 'sdi-round', 'sdi-evidence', 'sdi-init', 'sdi-converge', 'sdi-impl-loop'];

// ────────────────────────────────────────────────────────────────────────────
// Plugin root resolution

function pluginRoot() {
  if (process.env.PLUGIN_ROOT) return process.env.PLUGIN_ROOT;
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

// Returns the bypass source if an emergency main-session bypass is armed for
// this hook invocation, else null. The marker file is consumed on hit
// (deleted before honoring) so the bypass is one-shot.
//
// Marker body shapes (kept in lock-step with `sdi bypass arm`):
//   - JSON `{reason, armed_at, expires_at, ttl_seconds}` (current shape).
//     Expired markers are deleted but return null — they don't unlock the
//     gate, they just clean themselves up.
//   - Plain text (legacy v0.1.4 `touch ~/.cache/sdi/bypass-once` shape).
//     Treated as armed-forever with the body as `reason` for backward
//     compatibility; the recommended surface is `sdi bypass arm`.
//
// The env path (SDI_DELEGATION_BYPASS=1) is non-consuming — useful when the
// user starts Claude Code from a shell that already exported the var, but it
// does not catch the `VAR=1 cmd` inline pattern (env never reaches the hook
// spawn). The marker exists for that gap.
//
// Renamed from `consumeDelegationBypass`: the same marker now unlocks every
// mutating gate (D21 delegation, active-task, D29 claim overlap), not just
// D21. Splitting the bypass surface per gate caused the self-deadlock this
// function exists to break.
function consumeBypassMarker() {
  if (process.env[DELEGATION_BYPASS_ENV] === '1') return { source: 'env' };
  const marker = bypassOnceFile();
  let stat;
  try {
    stat = fs.statSync(marker);
  } catch {
    return null;
  }
  let reason = null;
  let expiresAt = null;
  let isExpired = false;
  if (stat.size > 0 && stat.size < 8192) {
    try {
      const raw = fs.readFileSync(marker, 'utf8').trim();
      let parsed = null;
      if (raw.startsWith('{')) {
        try {
          parsed = JSON.parse(raw);
        } catch {
          // Body looked like JSON but didn't parse — fall through to plain-text.
        }
      }
      if (parsed && typeof parsed === 'object') {
        reason = typeof parsed.reason === 'string' && parsed.reason.length > 0 ? parsed.reason : null;
        expiresAt = typeof parsed.expires_at === 'string' ? parsed.expires_at : null;
        if (expiresAt) {
          const exp = Date.parse(expiresAt);
          if (Number.isFinite(exp) && exp <= Date.now()) isExpired = true;
        }
      } else {
        // Legacy plain-text shape from `touch` + optional reason body.
        reason = raw || null;
      }
    } catch {
      // Unreadable marker still counts as armed — its presence is the
      // signal; the body is an optional audit annotation.
    }
  }
  try {
    fs.unlinkSync(marker);
  } catch {
    // Persistent marker would silently turn one-shot into permanent.
    // Surface that to stderr so the user notices.
    process.stderr.write(
      `[sdi] WARNING: failed to consume ${marker} — bypass may repeat.\n`,
    );
  }
  if (isExpired) {
    appendHookLog('bypass_marker_expired', { reason, expires_at: expiresAt });
    return null;
  }
  return { source: 'marker', reason, expires_at: expiresAt };
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

// Locate the dashboard SPA bundle (plugin/web) for the active install layout.
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
    const health = await daemonHealth().catch(() => null);
    if (health) {
      const want = pluginVersion(root);
      if (!want || !health.version || health.version === want) {
        return true; // healthy and on the current plugin version (or unknown)
      }
      // #17 — the plugin updated but the daemon is still running the old binary:
      // it serves a stale dashboard bundle and reports an old /health version,
      // diverging silently from the CLI. Restart it so the new daemon + SPA take
      // over. State lives in SQLite and survives the restart.
      process.stderr.write(
        `[sdi] daemon is ${health.version} but plugin is ${want} — restarting to match ` +
          `(SQLite data preserved)…\n`,
      );
      appendHookLog('daemon_version_restart', { from: health.version, to: want });
      await stopDaemon();
      // fall through to setup, which spawns the new daemon.
    }
    // health unreachable OR version mismatch (now stopped) → run setup.
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

  // Spawn daemon if not running. Probe the PORT (/health), not the pidfile:
  // a stale pidfile with a reused pid would otherwise read as "running" and
  // skip the spawn, leaving no daemon. The daemon binary self-guards against a
  // second instance (#19), so a redundant spawn here is harmless either way.
  const running = await pingHealth();
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

// Parsed /health payload (`{ ok, service, version }`) or null if the daemon is
// unreachable. Used by the install gate's version check (#17).
async function daemonHealth() {
  const base = daemonBase();
  if (!base) return null;
  try {
    const r = await httpGet(`${base}/health`);
    if (r.status !== 200) return null;
    return JSON.parse(r.text);
  } catch {
    return null;
  }
}

function readManifestVersion(root, relPath) {
  try {
    return JSON.parse(fs.readFileSync(path.join(root || pluginRoot(), relPath), 'utf8')).version || null;
  } catch {
    return null;
  }
}

// The plugin's own version, from the host manifest. This is the version of the
// binaries the install gate just resolved; comparing it to the live daemon's
// `/health` version detects a daemon left running on an older plugin build
// (#17).
function pluginVersion(root) {
  const base = root || pluginRoot();
  return (
    readManifestVersion(base, path.join('.codex-plugin', 'plugin.json')) ||
    readManifestVersion(base, path.join('.claude-plugin', 'plugin.json'))
  );
}

// Stop the running daemon (SIGTERM, then SIGKILL if it overstays the bounded
// graceful-shutdown window) and clear its pid/port files so a fresh spawn is
// unambiguous. SQLite state is untouched. Best-effort: a missing/dead pid is a
// no-op.
async function stopDaemon() {
  const pf = pidFile();
  const ptf = portFile();
  let pid = null;
  try {
    pid = parseInt(fs.readFileSync(pf, 'utf8').trim(), 10);
  } catch {
    return;
  }
  if (!Number.isFinite(pid) || pid <= 0) return;
  try {
    process.kill(pid, 'SIGTERM');
  } catch {
    // already gone
    return;
  }
  const deadline = Date.now() + 5000;
  while (Date.now() < deadline) {
    try {
      process.kill(pid, 0);
    } catch {
      break; // exited
    }
    await sleep(100);
  }
  try {
    process.kill(pid, 0);
    process.kill(pid, 'SIGKILL');
  } catch {
    // exited within the grace window
  }
  for (const f of [pf, ptf]) {
    try {
      if (fs.existsSync(f)) fs.unlinkSync(f);
    } catch {
      // leave stale file; spawnDaemon re-checks anyway
    }
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

// #18 — In-flight chores for a project (the lightweight maintenance lane). A
// chore is a kind='chore' task under the per-project CHORE container, created
// already in_progress; `GET /projects/:id/chores` returns exactly that
// in_progress set. Distinct from `inFlightTasks` (which is scoped to a real
// active plan's rounds) because a chore exists precisely when no real plan is
// active.
async function inFlightChores(projectId) {
  const json = await getJson(`/projects/${encodeURIComponent(projectId)}/chores`);
  return json && Array.isArray(json.tasks) ? json.tasks : [];
}

function readActiveTaskHint() {
  // Explicit fast-path pin (set before Claude Code launches). Optional — the
  // active-task gate falls back to live daemon state when this is unset (#9).
  return process.env.SDI_ACTIVE_TASK || process.env.CLAUDE_ACTIVE_TASK || null;
}

// #9 / #18 — Is there active task context for `project`, judged from DAEMON
// STATE? True iff EITHER:
//   (a) the project's active plan has ≥1 in_progress task, OR
//   (b) the project has ≥1 in-flight chore (the maintenance lane).
// `sdi task start <id>` satisfies (a) from inside the session; `sdi chore
// "<desc>"` satisfies (b) in one step when there is no active plan — that is
// the #18 escape hatch for trivial consistency edits after a plan/round closes.
// Daemon unreachable → caller treats as false but the surrounding gate degrades
// gracefully (bypass / explicit env).
async function hasActiveTaskContext(project) {
  if (!project) return false;
  const plan = await activePlanForProject(project.id);
  if (plan) {
    const tasks = await inFlightTasks(plan.id);
    if (Array.isArray(tasks) && tasks.length > 0) return true;
  }
  const chores = await inFlightChores(project.id);
  return Array.isArray(chores) && chores.length > 0;
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

// Read-only Bash whitelist. Conservative on purpose, but quote-aware: the
// sdi CLI takes natural-language arguments (GWT clauses, `--reason` text) by
// design, so metacharacters inside quoted strings must NOT read as shell
// operators — otherwise the gate blocks its own escape hatch (`sdi bypass
// arm --reason "(…)"`) and ordinary scenario authoring. The rules:
//   1. Quoted spans are masked first. Single-quoted content is fully inert;
//      double-quoted content stays live for `$` and backtick because the
//      shell still expands those inside double quotes.
//   2. Substitution / redirection / subshell metacharacters (`$`, backtick,
//      `<`, `>`, `(`, `)`) outside quotes disqualify the whole command. The
//      one exception is pure fd duplication (`2>&1`, `>&2`) — no file is
//      touched, and agents append it to read-only commands routinely.
//   3. Chains split on unquoted `&&` / `||` / `;` / `|`. The command is
//      read-only iff EVERY segment's verb passes the whitelist — `ls &&
//      grep …` passes, `sdi … && rm -rf` does not. A lone `&` (background)
//      disqualifies.
//   4. Unbalanced quoting disqualifies — we cannot reason about the command.

// Mask quoted spans in-place (same length, so indices stay aligned with the
// original string). Returns null on unbalanced quotes.
function maskQuotedSpans(cmd) {
  const out = cmd.split('');
  let state = 'plain'; // 'plain' | 'single' | 'double'
  let i = 0;
  while (i < cmd.length) {
    const c = cmd[i];
    if (state === 'plain') {
      if (c === '\\' && i + 1 < cmd.length) {
        out[i] = '_';
        out[i + 1] = '_';
        i += 2;
        continue;
      }
      if (c === "'") state = 'single';
      else if (c === '"') state = 'double';
    } else if (state === 'single') {
      if (c === "'") state = 'plain';
      else out[i] = '_';
    } else {
      // double-quoted: `$` and backtick keep their meta meaning, `\` escapes.
      if (c === '\\' && i + 1 < cmd.length) {
        out[i] = '_';
        out[i + 1] = '_';
        i += 2;
        continue;
      }
      if (c === '"') state = 'plain';
      else if (c !== '$' && c !== '`') out[i] = '_';
    }
    i += 1;
  }
  return state === 'plain' ? out.join('') : null;
}

// Per-segment verb whitelist. `segment` is the original (unmasked) text of
// one chain segment — flag scans (e.g. find's -delete/-exec) must see the
// real argument text, since find acts on its args regardless of how the
// shell quoted them.
function segmentIsReadOnly(segment) {
  const trimmed = segment.trim();
  if (!trimmed) return false;
  const allTokens = trimmed.split(/\s+/);

  // Strip leading inline env assignments (`VAR=val … cmd`) and judge the REAL
  // verb — `FOO=bar sdi plan list` is fine, but `FOO=bar rm -rf /` is NOT, so
  // an assignment prefix must not whitelist whatever follows it.
  let i0 = 0;
  while (i0 < allTokens.length && /^[A-Za-z_][A-Za-z0-9_]*=/.test(allTokens[i0])) i0 += 1;
  const tokens = allTokens.slice(i0);
  if (tokens.length === 0) return true; // pure assignment segment, no command
  const verb = tokens[0];

  // Innocuous shell-state prefixes — change cwd or env, execute no payload and
  // touch no file. The #4 PATH-setup idiom (`cd repo; export PATH=…; sdi …`)
  // splits into these segments plus the payload verb, each judged on its own.
  if (verb === 'cd' || verb === 'pushd' || verb === 'popd') return true;
  if (verb === 'export' || verb === 'set' || verb === 'unset') {
    return tokens.slice(1).every((t) => /^[A-Za-z_][A-Za-z0-9_]*(=.*)?$/.test(t));
  }

  // SDI management CLI. Recognise the bare token AND an absolute/relative path
  // to the bundled binary (#4: delegated agents invoke `<plugin-cache>/bin/sdi`,
  // not bare `sdi`, because the bundle is not on a fresh shell's PATH).
  const base = verb.includes('/') ? verb.slice(verb.lastIndexOf('/') + 1) : verb;
  if (base === 'sdi' || base === 'sdid') {
    const sub = tokens[1] || '';
    const action = tokens[2] || '';
    // Task LIFECYCLE mutation is execution work (D3 — tasks are runtime
    // artifacts the LLM decomposes, not orchestration). The main session may
    // only READ tasks; create/start/complete/decompose/lease/… delegate to a
    // specialist (scenario-decomposer, impl-coder, …).
    if (sub === 'task') {
      return /^(list|view|stats|ancestors|descendants|subtree|relations|lease-info|preflight|--help|-h)$/.test(
        action,
      );
    }
    // Everything else is orchestration the main session owns (D2): plan /
    // scenario / round / decide / req authoring (the spec, per D5/D8), plus
    // reads, daemon control, and the `bypass` escape hatch. Destructive ops
    // (`sdi project delete`) are gated by the daemon's own confirmation
    // prompt, not this verb whitelist.
    return true;
  }

  // Read-only GitHub CLI (#4c / D3). Context-gathering reads only; any
  // mutation (`gh issue create`, `gh pr merge`, `gh api -X POST`) delegates.
  if (verb === 'gh') {
    const sub = tokens[1] || '';
    const action = tokens[2] || '';
    if (sub === 'auth') return /^(status|token)$/.test(action);
    if (sub === 'api') {
      // Default method is GET; an explicit mutating -X/--method disqualifies.
      return !/(^|\s)(-X|--method)[\s=]+(POST|PUT|PATCH|DELETE)\b/i.test(trimmed);
    }
    if (sub === 'browse') return true;
    if (/^(repo|issue|pr|run|release|search|workflow|label|gist|cache|status|org)$/.test(sub)) {
      return /^(list|view|status|diff|checks|download|ls|--help|-h)$/.test(action);
    }
    return false;
  }

  if (verb === 'git') {
    // Skip git's global options so the real subcommand is judged, not the flag:
    // `git -C <path> remote -v` and `git --no-pager log` are read-only (#18).
    let gi = 1;
    while (gi < tokens.length) {
      const t = tokens[gi];
      if (t === '-C' || t === '-c') {
        gi += 2; // these consume the following token
      } else if (t === '--no-pager' || t === '-P' || t === '--paginate' || t === '--no-replace-objects') {
        gi += 1;
      } else if (t.startsWith('--git-dir=') || t.startsWith('--work-tree=') || t.startsWith('-C=')) {
        gi += 1;
      } else {
        break;
      }
    }
    return /^(status|log|diff|show|branch|remote|config|rev-parse|describe|ls-files|blame|tag)$/.test(
      tokens[gi] || '',
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
  return /^(ls|cat|head|tail|grep|rg|wc|file|which|pwd|echo|stat|env|printenv|date|uname|hostname|whoami|tree|jq|sort|uniq|cut|comm|column|basename|dirname|realpath|readlink|true|test)$/.test(
    verb,
  );
}

// Quote-aware detector for command/process substitution (`$(…)`, backtick,
// `<(…)`, `>(…)`). These execute a nested command and are dangerous even
// inside double quotes. A bare `$VAR` / `${VAR}` is NOT substitution and is
// not flagged. Single-quoted spans are fully inert.
function hasCommandSubstitution(cmd) {
  let state = 'plain'; // 'plain' | 'single' | 'double'
  for (let i = 0; i < cmd.length; i++) {
    const c = cmd[i];
    const n = cmd[i + 1];
    if (state === 'single') {
      if (c === "'") state = 'plain';
      continue;
    }
    if (c === '\\') {
      i += 1; // skip the escaped char (in plain and double states)
      continue;
    }
    if (state === 'double') {
      if (c === '"') state = 'plain';
      else if (c === '`') return true;
      else if (c === '$' && n === '(') return true;
      continue;
    }
    // plain
    if (c === "'") state = 'single';
    else if (c === '"') state = 'double';
    else if (c === '`') return true;
    else if (c === '$' && n === '(') return true;
    else if ((c === '<' || c === '>') && n === '(') return true;
  }
  return false;
}

function isReadOnlyBash(cmd) {
  if (!cmd || typeof cmd !== 'string') return false;
  const trimmed = cmd.trim();
  if (!trimmed) return false;
  const masked = maskQuotedSpans(trimmed);
  if (masked === null) return false; // unbalanced quotes
  // Command/process substitution is dangerous even INSIDE double quotes (the
  // shell expands `$(…)` and backticks there), and maskQuotedSpans masks the
  // `(` of `$(` inside double quotes — so detect substitution on a separate
  // quote-aware walk. A BARE `$VAR` / `${VAR}` expansion is NOT flagged (no
  // execution), which lets the `export PATH="$P/bin:$PATH"` idiom survive (#4).
  if (hasCommandSubstitution(trimmed)) return false;
  // Same-length replacements keep indices aligned with the original.
  let neutral = masked;
  // Discarding redirects (`>/dev/null`, `2>/dev/null`, `&>/dev/null`,
  // `>>/dev/null`) touch no real file — agents append them to read-only
  // commands routinely (#10).
  neutral = neutral.replace(/(\d*|&)>>?\s*\/dev\/null/g, (m) => '_'.repeat(m.length));
  // Pure fd duplication (`2>&1`, `>&2`).
  neutral = neutral.replace(/(\d*|&)>&\d+/g, (m) => '_'.repeat(m.length));
  // Unquoted backticks / parens that survived masking are subshell grouping or
  // substitution (quoted ones were masked away).
  if (/[`()]/.test(neutral)) return false;
  // Any remaining real redirect (to a file) reads/writes the filesystem.
  if (/[<>]/.test(neutral)) return false;
  // Split on chain operators; every segment must independently pass.
  const segments = [];
  let start = 0;
  const opRe = /&&|\|\||;|\||&/g;
  let m;
  while ((m = opRe.exec(neutral)) !== null) {
    if (m[0] === '&') return false; // background execution — not read-only
    segments.push(trimmed.slice(start, m.index));
    start = m.index + m[0].length;
  }
  segments.push(trimmed.slice(start));
  return segments.every(segmentIsReadOnly);
}

// Claude Code dispatches plugin-namespaced agent types as "<plugin>:<bare>"
// (e.g. "sdi:impl-coder"), while our AgentSpec frontmatter stores the bare
// name. Strip any namespace prefix so registry comparisons, activity feed
// payloads, and daemon-bound identifiers all converge on the canonical bare
// form. Returns the input unchanged when no ':' is present.
function normalizeAgentType(value) {
  if (typeof value !== 'string' || value.length === 0) return value;
  const i = value.lastIndexOf(':');
  return i >= 0 ? value.slice(i + 1) : value;
}

// Agent registry — read from THREE roots so a user-defined specialist is
// recognised without copying it into the plugin tree (#4/#11):
//   1. <cwd>/.claude/agents      — project-local (team-shared, version-ctl)
//   2. ~/.claude/agents          — user-level (all projects)
//   3. <pluginRoot>/agents       — SDI built-in specialists
// These are exactly Claude Code's own subagent discovery roots (per the
// official docs) minus the session/managed scopes the hook cannot see, so a
// `name` Claude Code can spawn is a `name` this registry recognises.
//
// Cache is invalidated by directory mtime: adding/removing an agent file is
// picked up without restarting the long-lived hook process (the previous
// permanent cache never noticed new registrations).
function agentRegistryRoots(cwd) {
  const roots = [];
  if (cwd) roots.push(path.join(cwd, '.claude', 'agents'));
  roots.push(path.join(os.homedir(), '.claude', 'agents'));
  roots.push(path.join(pluginRoot(), 'agents'));
  return roots;
}

const _agentDirCache = new Map(); // dir → { mtimeMs, names: Set }
function loadAgentNamesFromDir(dir) {
  let mtimeMs = 0;
  try {
    const st = fs.statSync(dir);
    if (!st.isDirectory()) return new Set();
    mtimeMs = st.mtimeMs;
  } catch {
    return new Set();
  }
  const cached = _agentDirCache.get(dir);
  if (cached && cached.mtimeMs === mtimeMs) return cached.names;
  const names = new Set();
  try {
    for (const entry of fs.readdirSync(dir)) {
      if (!entry.endsWith('.md')) continue;
      try {
        const raw = fs.readFileSync(path.join(dir, entry), 'utf8');
        const fm = raw.match(/^---[\r\n]+([\s\S]*?)^---/m);
        if (!fm) continue;
        const nameMatch = fm[1].match(/^name:\s*(\S+)\s*$/m);
        if (nameMatch) names.add(nameMatch[1]);
      } catch {}
    }
  } catch {}
  _agentDirCache.set(dir, { mtimeMs, names });
  return names;
}

// Is `bareType` registered in any of the three discovery roots?
function isRegisteredAgent(bareType, cwd) {
  for (const dir of agentRegistryRoots(cwd)) {
    if (loadAgentNamesFromDir(dir).has(bareType)) return true;
  }
  return false;
}

// Union of all registered agent names across the three roots (for hints).
function allRegisteredAgents(cwd) {
  const all = new Set();
  for (const dir of agentRegistryRoots(cwd)) {
    for (const n of loadAgentNamesFromDir(dir)) all.add(n);
  }
  return all;
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
// D13 — Decompose-time pattern decision advisory
//
// Root cause of "every pattern is `direct`": nothing brings the LLM to the
// pattern decision before a round fans out into tasks. `patternShapeAdvisory`
// above only fired on Agent/Task dispatches whose prompt ALREADY carried a
// multi-agent intent token — a chicken-and-egg that meant ordinary decompose
// never tripped it. This advisory triggers on the STRUCTURAL seam instead:
//
//   - `sdi round activate <R>` — the main session owns this, and main is the
//     actor that can spawn the pattern-orchestrator specialist. Nudge here so
//     the choice happens before any sub-agent decomposes.
//   - `sdi task create <R> …` — the actual fan-out (run by a decomposer
//     sub-agent, since D21 blocks main from task-create). Last line of defence,
//     fired only on the FIRST task of the round to avoid per-task noise.
//
// Non-blocking (stderr only). Silent when a non-`direct` active pattern already
// governs the round's plan, when the create already carries
// `--produced-via-pattern`, or when the daemon can't resolve the round.

function parseRoundDecomposeIntent(cmd) {
  // Match `[path/]sdi <round|task> <activate|create> <rest…>` up to the next
  // shell separator. The round id is positional arg 1 for both forms
  // (`round activate <ROUND>` / `task create <ROUND> <CODE> <DESC>`), so it
  // precedes the quoted description and any flags.
  const m = cmd.match(
    /(?:^|[\s;&|(])(?:\S*\/)?sdi\s+(round|task)\s+(activate|create)\b([^\n;&|]*)/,
  );
  if (!m) return null;
  const [, sub, action, restRaw] = m;
  if (!((sub === 'round' && action === 'activate') || (sub === 'task' && action === 'create'))) {
    return null;
  }
  const rest = restRaw || '';
  let roundId = null;
  for (const t of rest.trim().split(/\s+/).filter(Boolean)) {
    if (t.startsWith('-')) break; // positionals come before flags
    roundId = t;
    break;
  }
  const hasPatternFlag = /(?:^|\s)--(?:produced-via-pattern|pattern)\b/.test(rest);
  return { kind: action, roundId, hasPatternFlag };
}

async function decomposePatternAdvisory(toolName, toolInput) {
  if (!/^(Bash|Monitor)$/.test(toolName)) return;
  const cmd = String((toolInput && toolInput.command) || '');
  if (!cmd) return;
  const act = parseRoundDecomposeIntent(cmd);
  if (!act || !act.roundId) return;
  if (act.kind === 'create' && act.hasPatternFlag) return; // a pattern was chosen

  const round = await getJson(`/rounds/${encodeURIComponent(act.roundId)}`).catch(() => null);
  const planId =
    (round && (round.plan_id || (round.round && round.round.plan_id))) || null;
  if (!planId) return; // unresolvable round → stay quiet

  if (act.kind === 'create') {
    // Only nudge at the START of decompose (first task of the round).
    const tasks = await getJson(
      `/tasks?round_id=${encodeURIComponent(act.roundId)}`,
    ).catch(() => null);
    const existing = tasks && Array.isArray(tasks.tasks) ? tasks.tasks : [];
    if (existing.length > 0) return;
  }

  const active = await getJson('/patterns/active').catch(() => null);
  const rows = active && Array.isArray(active.patterns) ? active.patterns : [];
  const governed = rows.some(
    (p) => p && p.kind && p.kind !== 'direct' && p.plan_id === planId,
  );
  if (governed) return; // a real collaboration pattern already governs this plan

  process.stderr.write(
    '[sdi] D13 pattern decision: this round is about to decompose into tasks under a ' +
      '`direct` (solo) pattern — the L3-capped anti-pattern. Before fanning out, spawn the ' +
      'pattern-orchestrator specialist to choose a real collaboration pattern ' +
      '(workflow / swarm / graph / agents-as-tools), let pattern-critic validate it, then create ' +
      'tasks with `sdi task create … --produced-via-pattern <PAT-ID>`. If the work is genuinely ' +
      'solo, materialise an explicit `direct` pattern so the L3 choice is on the record. ' +
      '(non-blocking)\n',
  );
  appendHookLog('pre_tool_use_decompose_pattern_advisory', {
    tool: toolName,
    kind: act.kind,
    round_id: act.roundId,
    hint: 'no-real-pattern-governs-plan',
  });
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
// Clawket-style work summary for the SessionStart banner. One handoff fetch
// (active plan + scenarios + tasks + decisions + activity) plus one `next`
// fetch (the daemon-computed next step, #15) — both read-only and local, so
// the round-trips are cheap. Degrades gracefully: any field the daemon can't
// supply is simply omitted rather than failing the banner.
// ANSI palette for the terminal banner. Only applied to the user-facing
// `systemMessage`; the model-facing `additionalContext` stays plain so escape
// codes never pollute the assistant's context.
const BANNER_ANSI = {
  reset: '\x1b[0m',
  bold: '\x1b[1m',
  dim: '\x1b[2m',
  green: '\x1b[32m',
  yellow: '\x1b[33m',
  blue: '\x1b[34m',
  magenta: '\x1b[35m',
  cyan: '\x1b[36m',
  red: '\x1b[31m',
  gray: '\x1b[90m',
};

// One handoff fetch (active plan + scenarios + tasks + decisions + activity)
// plus one `next` fetch (the daemon-computed next step, #15) — both read-only
// and local. Fetch ONCE here; `formatSessionBanner` renders the data twice
// (plain for the model, coloured for the terminal) without re-querying.
async function gatherSessionData(project) {
  const handoff = await getJson(`/projects/${encodeURIComponent(project.id)}/handoff`).catch(
    () => null,
  );
  const plan = handoff && handoff.active_plan;
  if (!plan) return { plan: null };
  const next = await getJson(`/projects/${encodeURIComponent(project.id)}/next`).catch(() => null);
  return { handoff, plan, next };
}

// Render gathered data as a banner. `ansi=true` wraps segments in colour for the
// terminal (`systemMessage`); `ansi=false` returns plain text for the model
// (`additionalContext`). Degrades gracefully — missing fields are omitted.
function formatSessionBanner(project, data, ansi) {
  const C = BANNER_ANSI;
  const c = (value, ...tokens) => (ansi ? `${tokens.join('')}${value}${C.reset}` : value);
  const title = `${c('SDI', C.bold, C.cyan)} ${c('·', C.dim)} ${c(project.name, C.bold)} ${c(
    `(${project.key})`,
    C.dim,
  )}`;

  if (!data.plan) {
    let b = `${title}\n`;
    b += `${c('no active plan', C.yellow)} — author scenarios, then approve a plan:\n`;
    b += `  sdi plan create ${project.id} <SHORT> "<title>"\n`;
    b += `  sdi scenario create <PLAN-ID> <SHORT> --given "…" --when "…" --then "…" --confirmed\n`;
    b += `  sdi plan approve <PLAN-ID>\n`;
    return b;
  }

  const { handoff, plan, next } = data;
  const sep = c('·', C.dim);
  const lines = [title];

  lines.push(`${c('plan:', C.gray)} ${plan.short_code} ${sep} ${plan.title}`);

  // Scenario counts (confirmed / draft / retired) from the full list.
  const scenarios = Array.isArray(handoff.scenarios) ? handoff.scenarios : [];
  const live = scenarios.filter((s) => !s.retired_at);
  const confirmed = live.filter((s) => s.status === 'confirmed').length;
  const draft = live.filter((s) => s.status === 'draft').length;
  const retired = scenarios.length - live.length;
  let scnLine = `${c('scenarios:', C.gray)} ${c(`${confirmed} confirmed`, C.green)} ${sep} ${c(
    `${draft} draft`,
    C.yellow,
  )}`;
  if (retired > 0) scnLine += ` ${sep} ${c(`${retired} retired`, C.gray)}`;
  lines.push(scnLine);

  // Tasks: counts + in-progress detail.
  const inFlight = Array.isArray(handoff.in_flight_tasks) ? handoff.in_flight_tasks : [];
  const backlog = Array.isArray(handoff.backlog_tasks) ? handoff.backlog_tasks : [];
  lines.push(
    `${c('tasks:', C.gray)} ${c(`${inFlight.length} in-flight`, C.cyan)} ${sep} ${c(
      `${backlog.length} backlog`,
      C.gray,
    )}`,
  );
  for (const t of inFlight.slice(0, 3)) {
    const desc = String(t.description || '').slice(0, 60);
    lines.push(`  ${c('▸', C.cyan)} ${c(t.short_code, C.bold)} ${desc}`);
  }

  // Decisions: total + provisional (#16) flag.
  const decisions = Array.isArray(handoff.recent_decisions) ? handoff.recent_decisions : [];
  if (decisions.length > 0) {
    const provisional = decisions.filter((d) => d.supersede_when).length;
    let decLine = `${c('decisions:', C.gray)} ${decisions.length}`;
    if (provisional > 0) decLine += ` ${sep} ${c(`${provisional} provisional ⚠`, C.yellow)}`;
    lines.push(decLine);
  }

  // The daemon-computed next step (#15) — the headline of the banner.
  if (next && next.command) {
    lines.push(`${c('↳ next:', C.bold, C.magenta)} ${c(String(next.command).split('\n')[0], C.cyan)}`);
    if (next.reason) lines.push(c(`        ${next.reason}`, C.dim));
    const prov = Array.isArray(next.provisional_decisions) ? next.provisional_decisions : [];
    for (const d of prov.slice(0, 3)) {
      if (d.supersede_when) {
        lines.push(c(`        ⚠ revisit ${d.short_code} when: ${d.supersede_when}`, C.yellow));
      }
    }
  }

  // Recent activity (most recent first), up to 3.
  const activity = Array.isArray(handoff.recent_activity) ? handoff.recent_activity : [];
  if (activity.length > 0) {
    lines.push(c('recent:', C.gray));
    for (const a of activity.slice(0, 3)) {
      const summary = String(a.summary || a.kind || '').slice(0, 70);
      if (summary) lines.push(c(`  · ${summary}`, C.dim));
    }
  }
  return lines.join('\n') + '\n';
}

async function buildSessionSummary(project, { ansi = false } = {}) {
  return formatSessionBanner(project, await gatherSessionData(project), ansi);
}

// Soft-disabled (`sdi project disable` / dashboard settings) opts a project out
// of ALL SDI governance for its anchored cwds (the CLI `disable` contract).
// Every hook that resolves a project must honor it — not just PreToolUse (#20),
// or a disabled project keeps injecting banners/context and recording activity.
// Daemon serialises `enabled` as a bool; defensively accept the legacy 0/1 int.
function projectDisabled(project) {
  return !!project && (project.enabled === false || project.enabled === 0);
}

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

  // No registered project → a model-only hint (additionalContext); no visible
  // banner, since SDI's hook runs in every cwd and a banner in unrelated
  // directories would be session noise.
  if (!project) {
    let hint = `# SDI session\ncwd: ${cwd}\n`;
    hint += `\nNo SDI project registered for this cwd.\n`;
    hint += `Register: \`sdi project create <KEY> <name> --cwd ${cwd}\`\n`;
    hint += dashboardLine(root, false);
    appendHookLog('session_start', { cwd, project_id: null, web_state: webState(root) });
    process.stdout.write(JSON.stringify(sessionStartPayload(hint, null)) + '\n');
    return;
  }

  // Soft-disabled → SDI steps fully aside: no banner, no context (#20).
  if (projectDisabled(project)) {
    appendHookLog('session_start_skip', { cwd, reason: 'project-disabled', project_id: project.id });
    return;
  }

  // Registered project → gather once, render twice: plain for the model's
  // context, coloured for the terminal banner.
  const data = await gatherSessionData(project);
  const plain = formatSessionBanner(project, data, false) + dashboardLine(root, false);
  const coloured = formatSessionBanner(project, data, true) + dashboardLine(root, true);
  appendHookLog('session_start', { cwd, project_id: project.id, web_state: webState(root) });
  process.stdout.write(JSON.stringify(sessionStartPayload(plain, coloured)) + '\n');
}

function webState(root) {
  return process.env.SDI_WEB_DISABLE === '1' ? 'disabled' : resolveWebDist(root).state;
}

// Dashboard SPA advisory — single line, opt-out via SDI_WEB_DISABLE=1. The
// daemon-owned SPA is served at <port>/; status mirrors the daemon's resolver.
// `ansi` colours the URL for the terminal banner.
function dashboardLine(root, ansi) {
  if (process.env.SDI_WEB_DISABLE === '1') return '';
  const C = BANNER_ANSI;
  const c = (value, ...tokens) => (ansi ? `${tokens.join('')}${value}${C.reset}` : value);
  const web = resolveWebDist(root);
  if (web.state === 'ready') {
    const port = readDaemonPort();
    const url = port ? `http://127.0.0.1:${port}/` : '(daemon port unknown)';
    return `${c('dashboard:', C.gray)} ${c(url, C.blue)}\n`;
  }
  if (web.state === 'buildable') {
    return `${c('dashboard:', C.gray)} not built. Build once: \`pnpm --dir ${web.source} install && pnpm --dir ${web.source} build\` (or set SDI_WEB_DISABLE=1 to silence).\n`;
  }
  return ''; // state === 'absent' → bundle simply not shipped.
}

// `additionalContext` is injected into the MODEL's context but is invisible to
// the user. To render the work summary in the terminal — the way Clawket's
// SessionStart banner appears — the payload must ALSO carry a `systemMessage`,
// which Claude Code surfaces as the visible "SessionStart … says:" line. Pass
// `systemMessage = null` to keep a hint model-only (no visible banner).
function sessionStartPayload(additionalContext, systemMessage) {
  const out = {
    hookSpecificOutput: { hookEventName: 'SessionStart', additionalContext },
  };
  if (systemMessage) {
    out.systemMessage = systemMessage;
  }
  return out;
}

// UserPromptSubmit: inject active task / plan context. Warn if no active task.
async function runUserPromptSubmit(input) {
  // Best-effort context injection. If ensureInstalled wasn't called yet
  // (e.g. session-start failed), all daemon calls return null and we noop.
  const cwd = (input && input.cwd) || process.cwd();
  const project = await projectByCwd(cwd);
  if (!project) return;
  if (projectDisabled(project)) {
    appendHookLog('user_prompt_submit_skip', { reason: 'project-disabled', project_id: project.id });
    return;
  }
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

// Unified bypass surface hint — `sdi bypass arm` is the recommended path
// because the CLI is on the D21 read-only Bash whitelist (main session can
// call it directly, no specialist needed). One marker armed via this verb
// unlocks every mutating gate (D21 delegation, active-task, D29 claim
// overlap) on the next invocation.
const BYPASS_ARM_HINT =
  "One-shot override (audited): `sdi bypass arm --reason '<short reason>'`. " +
  'Quote the reason (single quotes keep $, backticks, and parens inert); ' +
  'run it as a single command — chained segments must each be read-only. ' +
  'Marker auto-expires in 60s (configurable via `--ttl`). ' +
  '`sdi bypass status` to inspect, `sdi bypass disarm` to clear.';

// Lazy marker consumer: at most one marker consumption per PreToolUse
// invocation, shared across all three mutating gates. The first gate that
// would block calls `tryBypass()` once; the result is reused by the others.
// Without this caching, a single armed marker would only unlock the first
// gate that fires and the next gate would still block the same invocation —
// the original self-deadlock in different clothes.
function makeBypassConsumer() {
  let consumed = false;
  let result = null;
  return function tryBypass() {
    if (consumed) return result;
    consumed = true;
    result = consumeBypassMarker();
    return result;
  };
}

function emitBypassWarning(gate, toolName, bypass, extra) {
  const label =
    bypass.source === 'env'
      ? `${DELEGATION_BYPASS_ENV}=1`
      : `marker ${bypassOnceFile()}`;
  process.stderr.write(
    `[sdi] WARNING: ${gate} bypass via ${label} — main session executing ${toolName}` +
      (extra ? ` ${extra}` : '') +
      (bypass.reason ? ` [reason: ${bypass.reason}]` : '') +
      `. Routine bypass is a protocol violation; audit log records every use.\n`,
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
//
// The bypass marker (armed via `sdi bypass arm`) unlocks every blocking gate
// for one invocation. A single PreToolUse call consumes the marker at most
// once, even if multiple gates would have blocked — see `makeBypassConsumer`.
async function runPreToolUse(input) {
  if (process.env[BYPASS_ENV] === '1') return;
  const toolName = (input && input.tool_name) || '';
  const watched = /^(Edit|Write|MultiEdit|Bash|Monitor|NotebookEdit|Agent|Task|TeamCreate|SendMessage)$/.test(toolName);
  if (!watched) return;

  // Project scope gate. SDI hooks only enforce gates inside cwds that resolve
  // to a registered SDI project. When the daemon is unreachable or the cwd is
  // not registered, projectByCwd returns null and every downstream gate is
  // skipped — matching Clawket's allow-on-`isProjectDisabled`/missing-context
  // pattern (claude-hooks.cjs:2074-2077) so the hook never bleeds onto repos
  // SDI does not own.
  //
  // `project.enabled === false` (soft-disabled via `sdi project disable` or
  // the dashboard settings) collapses to the same skip path: the user has
  // explicitly opted the project out of SDI governance, so the hook layer
  // steps aside until they re-enable. The daemon serialises `enabled` as a
  // bool; defensively handle the legacy 0/1 integer shape too.
  const cwd = (input && input.cwd) || process.cwd();
  const project = await projectByCwd(cwd).catch(() => null);
  if (!project) {
    appendHookLog('pre_tool_use_skip', { tool: toolName, reason: 'cwd-not-in-sdi-project' });
    return;
  }
  if (projectDisabled(project)) {
    appendHookLog('pre_tool_use_skip', {
      tool: toolName,
      reason: 'project-disabled',
      project_id: project.id,
    });
    return;
  }

  // One bypass consumption budget per PreToolUse invocation.
  const tryBypass = makeBypassConsumer();

  // D21 — delegation gate.
  const agentId = (input && input.agent_id) || null;
  const agentType = (input && input.agent_type) || null;
  const isMain = !agentId;
  // Monitor runs an arbitrary shell command just like Bash, so it gets the
  // same read-only check — otherwise it is a silent hole in the gate (#11: the
  // paradox where the gate blocked honest work but left Monitor wide open).
  const isShellTool = toolName === 'Bash' || toolName === 'Monitor';
  if (isMain && (isExecutionTool(toolName) || isShellTool)) {
    let bashCmd = null;
    let blocked = true;
    if (isShellTool) {
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
      const bypass = tryBypass();
      if (bypass) {
        emitBypassWarning(
          'D21',
          toolName,
          bypass,
          bashCmd ? `(cmd preview: "${bashCmd.slice(0, 80)}")` : null,
        );
        appendHookLog('pre_tool_use_delegation_bypass', {
          tool: toolName,
          bash_cmd: bashCmd ? bashCmd.slice(0, 200) : null,
          source: bypass.source,
          reason: bypass.reason || null,
        });
      } else {
        const reason = bashCmd != null
          ? `[sdi] D21 delegation gate: main session may not run a mutating ${toolName} command. ` +
            `Delegate to a specialist sub-agent via the Agent tool. ` +
            `cmd preview: "${bashCmd.slice(0, 80)}". ` +
            BYPASS_ARM_HINT
          : `[sdi] D21 delegation gate: main session may not call ${toolName}. ` +
            `Delegate to a specialist sub-agent via the Agent tool. ` +
            BYPASS_ARM_HINT;
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
  // D21 — specialist registration is ADVISORY, not a hard block (#11). An
  // unregistered agent_type (e.g. Claude Code's built-in `general-purpose`) is
  // permitted to act at L3: it can read AND do execution work, but D26
  // consensus autonomy (L4/L5) is structurally unreachable because that unlock
  // requires a registered (AgentSpec.name, stance) tuple it lacks. The
  // previous hard-deny was a deadlock — it blocked read-only Bash, the bypass
  // marker, and Agent re-delegation, leaving an unregistered sub-agent with no
  // escape hatch at all. Registration is read from three roots (project, user,
  // plugin) so a user-defined specialist counts without living in the plugin.
  if (!isMain && agentType) {
    const bareType = normalizeAgentType(agentType);
    if (!isRegisteredAgent(bareType, cwd)) {
      appendHookLog('pre_tool_use_unregistered_agent', {
        tool: toolName,
        reason: 'l3-autonomy-cap',
        agent_id: agentId,
        agent_type: agentType,
      });
      process.stderr.write(
        `[sdi] note: agent_type "${agentType}" is unregistered — acting at L3 ` +
          `(autonomy capped; register in .claude/agents or ~/.claude/agents to ` +
          `unlock D26 consensus autonomy).\n`,
      );
      // No return — the agent proceeds through the normal gates below.
    }
  }

  // D26 advisory — non-blocking, surfaces missing pattern row to stderr so the
  // L3 auto-fallback isn't silently inherited. Honored even before the
  // active-task gate so the warning lands alongside any subsequent block.
  // SDI_HOOK_V05_DISABLE=1 turns off both v0.5 gates (D26 advisory, D29 block).
  const v05Disabled = process.env[V05_DISABLE_ENV] === '1';
  if (!v05Disabled) {
    await patternShapeAdvisory(toolName, (input && input.tool_input) || {}).catch(() => {});
    await decomposePatternAdvisory(toolName, (input && input.tool_input) || {}).catch(() => {});
  }

  // Active-task gate. Only direct file-mutation tools require an active task —
  // Bash / Agent / Task / TeamCreate / SendMessage are bootstrap-capable
  // (creating the first plan, registering specialists, spawning the first
  // session) and have their own enforcement through D21 + D26 + D29. Without
  // this narrowing, a fresh project deadlocks: making the first task requires
  // a task to already be in_progress.
  //
  // #9 — "active task" is judged from DAEMON STATE (an in_progress task in the
  // active plan), not the `SDI_ACTIVE_TASK` env. That env can only be set
  // BEFORE Claude Code launches, so a sub-agent could never satisfy it from
  // inside a session — the gate was unsatisfiable and pushed every specialist
  // onto the bypass marker (or Bash heredocs). The env still works as an
  // explicit fast-path pin; absent it, we ask the daemon.
  const activeTaskId = readActiveTaskHint();
  if (isExecutionTool(toolName)) {
    const hasContext = activeTaskId ? true : await hasActiveTaskContext(project).catch(() => false);
    if (!hasContext) {
      const bypass = tryBypass();
      if (bypass) {
        emitBypassWarning('active-task', toolName, bypass, null);
        appendHookLog('pre_tool_use_active_task_bypass', {
          tool: toolName,
          source: bypass.source,
          reason: bypass.reason || null,
        });
      } else {
        emitDeny(
          `[sdi] no active task — pick one up before mutating files. ` +
            `Run \`sdi task list\` then \`sdi task start <TASK-ID>\` (todo → in_progress); ` +
            `the daemon then reports it as active work. ` +
            BYPASS_ARM_HINT,
        );
        appendHookLog('pre_tool_use_blocked', { tool: toolName, reason: 'no-active-task' });
        return;
      }
    }
  }

  // D29 — Resource claim overlap gate. Blocks Edit/Write/NotebookEdit when a
  // different scenario's active claim covers the target file path. Daemon
  // unreachable / no claim / no overlap → proceed.
  if (!v05Disabled) {
    const gate = await claimOverlapGate(toolName, (input && input.tool_input) || {}, agentId).catch(
      () => ({ block: false }),
    );
    if (gate && gate.block) {
      const bypass = tryBypass();
      if (bypass) {
        emitBypassWarning(
          'D29',
          toolName,
          bypass,
          `(target: ${gate.payload.target_path})`,
        );
        appendHookLog('pre_tool_use_claim_bypass', {
          tool: toolName,
          task_id: activeTaskId,
          target_path: gate.payload.target_path,
          my_scenario: gate.payload.my_scenario,
          holders: gate.payload.holders,
          source: bypass.source,
          reason: bypass.reason || null,
        });
      } else {
        process.stdout.write(
          JSON.stringify({
            hookSpecificOutput: {
              hookEventName: 'PreToolUse',
              permissionDecision: 'deny',
              permissionDecisionReason:
                `[sdi] D29 claim overlap on ${gate.payload.target_path}. ` +
                `Holders: ${gate.payload.holders.map((h) => h.scenario_id).join(', ')}. ` +
                `${gate.payload.hint}. ` +
                BYPASS_ARM_HINT,
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
  if (projectDisabled(project)) return; // governance off — don't record activity (#20)
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
  const rawAgent = (input && input.subagent_type) || (input && input.agent_name) || 'unknown';
  const agent = normalizeAgentType(rawAgent);
  appendHookLog('subagent_start', { task_id: activeTaskId, agent });
  const cwd = (input && input.cwd) || process.cwd();
  const project = await projectByCwd(cwd).catch(() => null);
  if (!project) return;
  if (projectDisabled(project)) return; // governance off — don't bind subagents (#20)
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
  const rawAgent = (input && input.subagent_type) || (input && input.agent_name) || 'unknown';
  const agent = normalizeAgentType(rawAgent);
  const result = (input && input.result) || (input && input.summary) || null;
  appendHookLog('subagent_stop', { task_id: activeTaskId, agent, result });
  const cwd = (input && input.cwd) || process.cwd();
  const project = await projectByCwd(cwd).catch(() => null);
  if (!project) return;
  if (projectDisabled(project)) return; // governance off — don't record subagent result (#20)
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
    isReadOnlyBash,
    pluginVersion,
    readManifestVersion,
    parseRoundDecomposeIntent,
    decomposePatternAdvisory,
    buildSessionSummary,
    sessionStartPayload,
    resolveSdiBin,
    resolveSdidBin,
    resolveWebDist,
    findWorkspaceRoot,
    verifySdiSkills,
    daemonBase,
    appendHookLog,
    bypassOnceFile,
    consumeBypassMarker,
    inFlightChores,
    hasActiveTaskContext,
    SDI_SKILLS,
  },
};
