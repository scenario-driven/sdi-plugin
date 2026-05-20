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
const HOME_ENV = 'SDI_HOME';

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

  // Make daemon binary discoverable by `sdi daemon start` (which does its own
  // sibling-lookup from the running `sdi` binary — already satisfied).
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
  appendHookLog('session_start', { cwd, project_id: project && project.id });
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

// PreToolUse: block Edit/Write/Bash if no active task in_progress.
function runPreToolUse(input) {
  if (process.env[BYPASS_ENV] === '1') return;
  const toolName = (input && input.tool_name) || '';
  const watched = /^(Edit|Write|MultiEdit|Bash|NotebookEdit|Agent|Task|TeamCreate|SendMessage)$/.test(toolName);
  if (!watched) return;
  const activeTaskId = readActiveTaskHint();
  if (!activeTaskId) {
    process.stdout.write(
      JSON.stringify({
        hookSpecificOutput: {
          hookEventName: 'PreToolUse',
          permissionDecision: 'deny',
          permissionDecisionReason:
            `[sdi] no active task — set one before mutating files. ` +
            `Run \`sdi task list\` and \`sdi task update <TASK-ID> --status in_progress\`, ` +
            `or set ${BYPASS_ENV}=1 to bypass.`,
        },
      }) + '\n',
    );
    appendHookLog('pre_tool_use_blocked', { tool: toolName, reason: 'no-active-task' });
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
  runPreToolUse: () => dispatchSync(runPreToolUse),
  runPostToolUse: () => dispatchAsync(runPostToolUse),
  runSubagentStart: () => dispatchAsync(runSubagentStart),
  runSubagentStop: () => dispatchAsync(runSubagentStop),
  // Internals exposed for tests
  _internals: {
    resolveSdiBin,
    resolveSdidBin,
    findWorkspaceRoot,
    verifySdiSkills,
    daemonBase,
    appendHookLog,
    SDI_SKILLS,
  },
};
