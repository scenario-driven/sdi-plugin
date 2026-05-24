// Smoke + integration tests for plugin hook handlers + install gate.
//
// Each test isolates user-data paths under a per-test temp directory pinned
// via SDI_HOME (honored by both the JS shared module and the Rust binaries).
// This keeps `~/.local/share/sdi`, `~/.cache/sdi`, etc. untouched on the dev
// machine.
//
// Run: `node --test plugin/tests/hooks.test.cjs`

'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const http = require('node:http');
const { spawn, spawnSync } = require('node:child_process');

const PLUGIN_ROOT = path.resolve(__dirname, '..');
const WORKSPACE_ROOT = path.resolve(PLUGIN_ROOT, '..');
const SHARED = path.join(PLUGIN_ROOT, 'adapters/shared/sdi-hooks.cjs');
const SHIM_DIR = path.join(PLUGIN_ROOT, 'adapters/claude');

// Stable fake project cwd used across PreToolUse tests. The mock daemon
// returns a registered project for ANY cwd, so this constant just keeps
// stdin payloads readable.
const PROJECT_CWD = '/tmp/sdi-mock-project';

function mkTempHome(prefix) {
  return fs.mkdtempSync(path.join(os.tmpdir(), `${prefix}-`));
}

function shimEnv(home, extra) {
  return {
    ...process.env,
    SDI_HOME: home,
    CLAUDE_PLUGIN_ROOT: PLUGIN_ROOT,
    ...extra,
  };
}

function runShim(name, env, stdin) {
  const shim = path.join(SHIM_DIR, name);
  return spawnSync('node', [shim], {
    env,
    input: stdin || '',
    encoding: 'utf8',
    timeout: 10000,
  });
}

// Async shim runner — required when the child shim calls back into a mock
// HTTP server hosted in this process. spawnSync deadlocks the mock (parent
// blocked → no accept → request times out).
function runShimAsync(name, env, stdin) {
  return new Promise((resolve) => {
    const shim = path.join(SHIM_DIR, name);
    const child = spawn('node', [shim], { env });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (c) => (stdout += c.toString('utf8')));
    child.stderr.on('data', (c) => (stderr += c.toString('utf8')));
    child.on('close', (code) => resolve({ status: code, stdout, stderr }));
    if (stdin) child.stdin.write(stdin);
    child.stdin.end();
  });
}

function pinDaemonPort(home, port) {
  fs.mkdirSync(path.join(home, '.cache/sdi'), { recursive: true });
  fs.writeFileSync(path.join(home, '.cache/sdi/sdid.port'), String(port));
}

// Full mock SDI daemon — answers every endpoint the PreToolUse hook touches.
// Pre-Change-A tests didn't need this because the hook ran unconditionally;
// now the hook short-circuits on `cwd-not-in-sdi-project` unless /projects/by-cwd
// returns a row. Caller can override any field; defaults give a "registered
// project, no active plan, no claims, no patterns" baseline that triggers no
// downstream gates of its own.
function startMockSdiDaemon(opts) {
  const o = opts || {};
  const project = o.project || {
    id: 'PROJ-test',
    key: 'TEST',
    name: 'Test',
    cwd: PROJECT_CWD,
  };
  const plan = o.plan === undefined ? null : o.plan; // null = no active plan
  const policy = o.policy || { mode: 'L5' };
  const scenarios = o.scenarios || [];
  const patterns = o.patterns || [];
  const claimsStatus = o.claimsStatus || 200;

  const server = http.createServer((req, res) => {
    const u = new URL(req.url, 'http://127.0.0.1');
    const send = (status, body) => {
      const buf = Buffer.from(JSON.stringify(body));
      res.writeHead(status, {
        'content-type': 'application/json',
        'content-length': buf.length,
      });
      res.end(buf);
    };
    if (u.pathname === '/health') return send(200, { ok: true });
    if (u.pathname === '/projects/by-cwd') return send(200, { project });
    if (u.pathname === `/projects/${project.id}/plans/active`) {
      return send(200, plan ? { plan } : {});
    }
    if (u.pathname === '/autonomy_policies/resolve') return send(200, { policy });
    if (u.pathname === '/scenarios/active-claims') {
      if (claimsStatus !== 200) {
        res.writeHead(claimsStatus);
        return res.end();
      }
      return send(200, { scenarios });
    }
    if (u.pathname === '/patterns/active') return send(200, { patterns });
    res.writeHead(404);
    res.end();
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () =>
      resolve({ server, port: server.address().port, project }),
    );
  });
}

// ────────────────────────────────────────────────────────────────────────────
test('shared module: SDI_SKILLS is in lock-step with skills/ and plugin.json#skillsList', () => {
  delete require.cache[require.resolve(SHARED)];
  const { _internals } = require(SHARED);
  // Four `sdi-` prefixed skills, all self-contained. Lock-step contract
  // enforced across three sources: SDI_SKILLS array, skills/<name>/SKILL.md
  // files, plugin.json#skillsList.
  assert.deepEqual(_internals.SDI_SKILLS, [
    'sdi-overview',
    'sdi-scenario',
    'sdi-round',
    'sdi-evidence',
  ]);
  for (const name of _internals.SDI_SKILLS) {
    assert.ok(
      fs.existsSync(path.join(PLUGIN_ROOT, 'skills', name, 'SKILL.md')),
      `skills/${name}/SKILL.md must exist on disk`,
    );
  }
  const manifest = JSON.parse(
    fs.readFileSync(path.join(PLUGIN_ROOT, '.claude-plugin/plugin.json'), 'utf8'),
  );
  const names = (manifest.skillsList || []).map((s) => s.name);
  for (const name of _internals.SDI_SKILLS) {
    assert.ok(
      names.includes(name),
      `plugin.json#skillsList must include "${name}"`,
    );
  }
});

test('shared module: findWorkspaceRoot locates the Cargo workspace from plugin/', () => {
  delete require.cache[require.resolve(SHARED)];
  const { _internals } = require(SHARED);
  const found = _internals.findWorkspaceRoot(PLUGIN_ROOT);
  assert.equal(found, WORKSPACE_ROOT);
});

test('shared module: resolveSdiBin finds the workspace target/debug/sdi binary', () => {
  delete require.cache[require.resolve(SHARED)];
  const { _internals } = require(SHARED);
  const res = _internals.resolveSdiBin(PLUGIN_ROOT);
  assert.ok(res, 'expected sdi binary to be resolvable');
  assert.ok(['target-debug', 'target-release', 'plugin-bin', 'path'].includes(res.kind));
  assert.ok(fs.existsSync(res.bin));
});

test('shared module: SDI_BIN env override wins over workspace targets', () => {
  // Use the sdid binary as a stand-in to prove the env override is honored.
  const fakeSdi = path.join(WORKSPACE_ROOT, 'target/debug/sdid');
  assert.ok(fs.existsSync(fakeSdi));
  delete require.cache[require.resolve(SHARED)];
  process.env.SDI_BIN = fakeSdi;
  try {
    const { _internals } = require(SHARED);
    const res = _internals.resolveSdiBin(PLUGIN_ROOT);
    assert.equal(res.kind, 'env');
    assert.equal(res.bin, fakeSdi);
  } finally {
    delete process.env.SDI_BIN;
  }
});

test('shared module: resolveWebDist three states (ready / buildable / absent)', () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'sdi-web-'));
  try {
    delete require.cache[require.resolve(SHARED)];
    const { _internals } = require(SHARED);

    // Stub a plugin root with no web/ tree → absent.
    const rootAbsent = path.join(tmp, 'absent');
    fs.mkdirSync(rootAbsent, { recursive: true });
    assert.equal(_internals.resolveWebDist(rootAbsent).state, 'absent');

    // Stub a plugin root with web/package.json but no dist → buildable.
    const rootBuildable = path.join(tmp, 'buildable');
    fs.mkdirSync(path.join(rootBuildable, 'web'), { recursive: true });
    fs.writeFileSync(path.join(rootBuildable, 'web', 'package.json'), '{}');
    const buildable = _internals.resolveWebDist(rootBuildable);
    assert.equal(buildable.state, 'buildable');
    assert.equal(buildable.source, path.join(rootBuildable, 'web'));

    // Stub a plugin root with web/dist/index.html → ready.
    const rootReady = path.join(tmp, 'ready');
    fs.mkdirSync(path.join(rootReady, 'web', 'dist'), { recursive: true });
    fs.writeFileSync(path.join(rootReady, 'web', 'dist', 'index.html'), '<html/>');
    const ready = _internals.resolveWebDist(rootReady);
    assert.equal(ready.state, 'ready');
    assert.equal(ready.dist, path.join(rootReady, 'web', 'dist'));

    // SDI_WEB_DIST env override wins over plugin-root lookup.
    const override = path.join(tmp, 'override');
    fs.mkdirSync(override, { recursive: true });
    fs.writeFileSync(path.join(override, 'index.html'), '<html/>');
    process.env.SDI_WEB_DIST = override;
    try {
      delete require.cache[require.resolve(SHARED)];
      const { _internals: i2 } = require(SHARED);
      const r = i2.resolveWebDist(rootAbsent);
      assert.equal(r.state, 'ready');
      assert.equal(r.dist, override);
    } finally {
      delete process.env.SDI_WEB_DIST;
    }
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

// ────────────────────────────────────────────────────────────────────────────
// Shim error-safety: every shim exits 0 even if the shared module throws.

test('shim wraps: PreToolUse exits 0 with no active task (deny path, not crash)', async () => {
  const home = mkTempHome('sdi-pretool');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '' });
    // D21: main session can't call Edit at all — simulate a registered
    // sub-agent so the active-task gate is what's under test here.
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Edit',
        agent_id: '00000000-0000-0000-0000-000000000099',
        agent_type: 'impl-coder',
      }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    // Should print the deny JSON payload on stdout.
    assert.match(r.stdout, /permissionDecision.*deny/);
    assert.match(r.stdout, /no active task/);
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

// ────────────────────────────────────────────────────────────────────────────
// D21 — Mandatory delegation gate (PRD §2 D21, §5 Layer 1.5).

test('D21: PreToolUse blocks main session Edit (delegation gate)', async () => {
  const home = mkTempHome('sdi-d21-main-edit');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_DELEGATION_BYPASS: '' });
    // No agent_id → main session.
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({ cwd: PROJECT_CWD, tool_name: 'Edit' }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    assert.match(r.stdout, /permissionDecision.*deny/);
    assert.match(r.stdout, /D21 delegation gate/);
    // Audit log records the block reason.
    const log = path.join(home, '.local/state/sdi/hook.log');
    const entries = fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => JSON.parse(l));
    assert.ok(
      entries.some((e) => e.event === 'pre_tool_use_blocked' && e.reason === 'delegation-gate'),
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('D21: PreToolUse blocks main mutating Bash (rm)', async () => {
  const home = mkTempHome('sdi-d21-mut-bash');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_DELEGATION_BYPASS: '' });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Bash',
        tool_input: { command: 'rm -rf /tmp/foo' },
      }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    assert.match(r.stdout, /permissionDecision.*deny/);
    assert.match(r.stdout, /D21 delegation gate/);
    assert.match(r.stdout, /mutating Bash/);
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('D21: PreToolUse allows main read-only Bash (git status) through delegation gate', async () => {
  const home = mkTempHome('sdi-d21-ro-bash');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_DELEGATION_BYPASS: '' });
    // Main + read-only bash → passes D21, falls through to active-task gate.
    // Bash is bootstrap-capable (Change B), so the active-task gate no longer
    // fires for Bash — assert the read-only allow audit row instead.
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Bash',
        tool_input: { command: 'git status' },
      }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    assert.doesNotMatch(r.stdout, /D21 delegation gate/);
    // Audit log records the read-only allow.
    const log = path.join(home, '.local/state/sdi/hook.log');
    const entries = fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => JSON.parse(l));
    assert.ok(
      entries.some(
        (e) => e.event === 'pre_tool_use_delegation_allow' && e.reason === 'read-only-bash',
      ),
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('D21: PreToolUse rejects unregistered sub-agent (rogue-specialist)', async () => {
  const home = mkTempHome('sdi-d21-rogue');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_ACTIVE_TASK: 'TASK-X' });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Edit',
        agent_id: '00000000-0000-0000-0000-000000000001',
        agent_type: 'never-registered-agent',
      }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    assert.match(r.stdout, /permissionDecision.*deny/);
    assert.match(r.stdout, /rogue-specialist/);
    const log = path.join(home, '.local/state/sdi/hook.log');
    const entries = fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => JSON.parse(l));
    assert.ok(
      entries.some(
        (e) => e.event === 'pre_tool_use_blocked' && e.reason === 'rogue-specialist',
      ),
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('D21: PreToolUse passes registered sub-agent through delegation gate', async () => {
  const home = mkTempHome('sdi-d21-pass');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '' });
    // Registered sub-agent, no active task → delegation passes, active-task fails.
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Edit',
        agent_id: '00000000-0000-0000-0000-000000000002',
        agent_type: 'impl-coder',
      }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    assert.match(r.stdout, /permissionDecision.*deny/);
    assert.match(r.stdout, /no active task/);
    assert.doesNotMatch(r.stdout, /D21 delegation gate/);
    assert.doesNotMatch(r.stdout, /rogue-specialist/);
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('D21: SDI_DELEGATION_BYPASS=1 unblocks main + audits the bypass', async () => {
  const home = mkTempHome('sdi-d21-bypass');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_DELEGATION_BYPASS: '1' });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({ cwd: PROJECT_CWD, tool_name: 'Edit' }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    // Delegation bypassed — falls through to active-task gate (no active task here).
    assert.doesNotMatch(r.stdout, /D21 delegation gate/);
    assert.match(r.stdout, /no active task/);
    // stderr surfaces the bypass warning.
    assert.match(r.stderr, /SDI_DELEGATION_BYPASS=1/);
    const log = path.join(home, '.local/state/sdi/hook.log');
    const entries = fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => JSON.parse(l));
    assert.ok(entries.some((e) => e.event === 'pre_tool_use_delegation_bypass'));
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('D21: PreToolUse does NOT trigger delegation gate on orchestration tools (Agent)', async () => {
  const home = mkTempHome('sdi-d21-agent');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_ACTIVE_TASK: 'TASK-AGENT' });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({ cwd: PROJECT_CWD, tool_name: 'Agent' }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    // Agent is orchestration — main is allowed to spawn specialists.
    assert.doesNotMatch(r.stdout, /D21 delegation gate/);
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('shim wraps: PreToolUse allows when SDI_BYPASS_HOOKS=1', () => {
  const home = mkTempHome('sdi-bypass');
  try {
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '1' });
    const r = runShim('pre-tool-use.cjs', env, JSON.stringify({ tool_name: 'Edit' }));
    assert.equal(r.status, 0);
    // No deny payload emitted.
    assert.doesNotMatch(r.stdout, /permissionDecision.*deny/);
  } finally {
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('shim wraps: PreToolUse ignores tools outside the matcher (e.g. Read)', () => {
  const home = mkTempHome('sdi-read');
  try {
    const env = shimEnv(home, {});
    const r = runShim('pre-tool-use.cjs', env, JSON.stringify({ tool_name: 'Read' }));
    assert.equal(r.status, 0);
    assert.equal(r.stdout, '');
  } finally {
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('shim wraps: PostToolUse records to audit log when active task set', () => {
  const home = mkTempHome('sdi-post');
  try {
    const env = shimEnv(home, { SDI_ACTIVE_TASK: 'TASK-DEADBEEF' });
    const r = runShim(
      'post-tool-use.cjs',
      env,
      JSON.stringify({ tool_name: 'Edit', tool_input: { file_path: '/tmp/foo.rs' } }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    const log = path.join(home, '.local/state/sdi/hook.log');
    assert.ok(fs.existsSync(log));
    const lines = fs.readFileSync(log, 'utf8').trim().split('\n');
    const entry = JSON.parse(lines[lines.length - 1]);
    assert.equal(entry.event, 'post_tool_use');
    assert.equal(entry.task_id, 'TASK-DEADBEEF');
    assert.equal(entry.file, '/tmp/foo.rs');
    assert.equal(entry.tool, 'Edit');
  } finally {
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('shim wraps: SubagentStart / Stop append to the audit log', () => {
  const home = mkTempHome('sdi-sub');
  try {
    const env = shimEnv(home, { SDI_ACTIVE_TASK: 'TASK-AAA' });
    const start = runShim(
      'subagent-start.cjs',
      env,
      JSON.stringify({ subagent_type: 'Plan' }),
    );
    assert.equal(start.status, 0, `stderr=${start.stderr}`);
    const stop = runShim(
      'subagent-stop.cjs',
      env,
      JSON.stringify({ subagent_type: 'Plan', result: 'ok' }),
    );
    assert.equal(stop.status, 0, `stderr=${stop.stderr}`);
    const log = path.join(home, '.local/state/sdi/hook.log');
    const entries = fs
      .readFileSync(log, 'utf8')
      .trim()
      .split('\n')
      .map((l) => JSON.parse(l));
    assert.ok(entries.some((e) => e.event === 'subagent_start' && e.agent === 'Plan'));
    assert.ok(entries.some((e) => e.event === 'subagent_stop' && e.result === 'ok'));
  } finally {
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('shim wraps: UserPromptSubmit exits 0 silently when no daemon up', () => {
  const home = mkTempHome('sdi-prompt-nodaemon');
  try {
    const env = shimEnv(home, {});
    const r = runShim('user-prompt-submit.cjs', env, JSON.stringify({}));
    // No daemon, no project, no panic — just exit 0 with nothing on stdout.
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    assert.equal(r.stdout, '');
  } finally {
    fs.rmSync(home, { recursive: true, force: true });
  }
});

// ────────────────────────────────────────────────────────────────────────────
// End-to-end: SessionStart runs ensureInstalled with the workspace binary,
// daemon spins up, dashboard banner is emitted, daemon is stopped at the end.

// ────────────────────────────────────────────────────────────────────────────
// v0.5 meta-specialist agent definitions (D22 / D28).
//
// pattern-orchestrator, pattern-critic, reversal-runner each carry the
// minimal frontmatter Claude Code's agent loader requires: name +
// description + tools + model. Without these the AgentSpec registry won't
// pick them up and the D26 / D28 negotiation surface collapses.

test('v0.5 agents: pattern-orchestrator, pattern-critic, reversal-runner exist with proper frontmatter', () => {
  const required = ['pattern-orchestrator', 'pattern-critic', 'reversal-runner'];
  for (const name of required) {
    const file = path.join(PLUGIN_ROOT, 'agents', `${name}.md`);
    assert.ok(fs.existsSync(file), `agents/${name}.md must exist`);
    const raw = fs.readFileSync(file, 'utf8');
    assert.match(raw, /^---\n[\s\S]+?\n---\n/, `agents/${name}.md missing frontmatter block`);
    const fm = raw.slice(4, raw.indexOf('\n---\n', 4));
    assert.match(fm, new RegExp(`name:\\s*${name}\\b`), `agents/${name}.md frontmatter must declare name: ${name}`);
    assert.match(fm, /description:\s*\S/, `agents/${name}.md frontmatter must include description`);
    assert.match(fm, /tools:\s*\[?\s*\S/, `agents/${name}.md frontmatter must declare tools`);
    assert.match(fm, /model:\s*\S/, `agents/${name}.md frontmatter must declare model`);
  }
});

// ────────────────────────────────────────────────────────────────────────────
// D29 — Resource claim overlap gate (PRD §5 Layer 2.8).
//
// PreToolUse blocks Edit/Write/NotebookEdit when another scenario holds an
// active claim on the target path. Daemon unreachable → proceed (don't lock
// the editor when the daemon is down). SDI_HOOK_V05_DISABLE=1 bypasses the
// gate (one-shot escape; audit log records every use).
//
// We stand up a tiny mock HTTP server returning the daemon's
// /scenarios/active-claims shape and point SDI_HOME at a temp dir whose
// .cache/sdi/sdid.port references it. Same mechanism the e2e test uses,
// minus a real daemon — D29 logic is the unit under test.

test('D29: PreToolUse blocks Edit when another scenario claims the target path', async () => {
  const home = mkTempHome('sdi-d29-overlap');
  const { server, port } = await startMockSdiDaemon({
    scenarios: [
      {
        id: 'SCN-OTHER',
        claimed_resources_json: JSON.stringify(['crates/db/src/migrations/*.sql']),
      },
    ],
  });
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, {
      SDI_BYPASS_HOOKS: '',
      SDI_ACTIVE_TASK: 'TASK-D29',
      SDI_ACTIVE_SCENARIO: 'SCN-MINE',
    });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Edit',
        tool_input: { file_path: 'crates/db/src/migrations/008_x.sql' },
        agent_id: '00000000-0000-0000-0000-0000000000d2',
        agent_type: 'impl-coder',
      }),
    );
    assert.equal(r.status, 2, `expected exit 2 from claim block, got ${r.status} stderr=${r.stderr}`);
    const out = r.stdout;
    assert.match(out, /sdi_claim_overlap/);
    assert.match(out, /SCN-OTHER/);
    assert.match(out, /crates\/db\/src\/migrations\/008_x\.sql/);
    const log = path.join(home, '.local/state/sdi/hook.log');
    const entries = fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => JSON.parse(l));
    assert.ok(
      entries.some((e) => e.event === 'pre_tool_use_blocked' && e.reason === 'claim-overlap'),
      `audit log missing claim-overlap entry:\n${JSON.stringify(entries, null, 2)}`,
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('D29: PreToolUse proceeds when the claims ledger is empty', async () => {
  const home = mkTempHome('sdi-d29-empty');
  const { server, port } = await startMockSdiDaemon({ scenarios: [] });
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, {
      SDI_BYPASS_HOOKS: '',
      SDI_ACTIVE_TASK: 'TASK-D29',
      SDI_ACTIVE_SCENARIO: 'SCN-MINE',
    });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Edit',
        tool_input: { file_path: 'crates/db/src/migrations/008_x.sql' },
        agent_id: '00000000-0000-0000-0000-0000000000d3',
        agent_type: 'impl-coder',
      }),
    );
    assert.equal(r.status, 0, `expected exit 0, got ${r.status} stderr=${r.stderr}`);
    assert.doesNotMatch(r.stdout, /sdi_claim_overlap/);
    // Mock server was reachable, so no "unreachable" advisory should fire.
    assert.doesNotMatch(r.stderr, /daemon unreachable/, `unexpected unreachable warning: ${r.stderr}`);
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('D29: PreToolUse proceeds (with warning) when the claims endpoint is unreachable', async () => {
  // Project lookup must succeed (project-scope gate fires first); the claims
  // endpoint specifically returns 503 → claim gate emits the "daemon
  // unreachable" advisory and proceeds without blocking.
  const home = mkTempHome('sdi-d29-unreachable');
  const { server, port } = await startMockSdiDaemon({ claimsStatus: 503 });
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, {
      SDI_BYPASS_HOOKS: '',
      SDI_ACTIVE_TASK: 'TASK-D29',
      SDI_ACTIVE_SCENARIO: 'SCN-MINE',
    });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Edit',
        tool_input: { file_path: 'crates/db/src/migrations/008_x.sql' },
        agent_id: '00000000-0000-0000-0000-0000000000d4',
        agent_type: 'impl-coder',
      }),
    );
    assert.equal(r.status, 0, `expected exit 0, got ${r.status} stderr=${r.stderr}`);
    assert.doesNotMatch(r.stdout, /sdi_claim_overlap/);
    // stderr surfaces the "daemon unreachable" advisory.
    assert.match(r.stderr, /D29 advisory: daemon unreachable/);
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('D29: SDI_HOOK_V05_DISABLE=1 bypasses the claim gate even when overlap exists', async () => {
  const home = mkTempHome('sdi-d29-disable');
  const { server, port } = await startMockSdiDaemon({
    scenarios: [
      {
        id: 'SCN-OTHER',
        claimed_resources_json: JSON.stringify(['crates/db/src/migrations/*.sql']),
      },
    ],
  });
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, {
      SDI_BYPASS_HOOKS: '',
      SDI_HOOK_V05_DISABLE: '1',
      SDI_ACTIVE_TASK: 'TASK-D29',
      SDI_ACTIVE_SCENARIO: 'SCN-MINE',
    });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Edit',
        tool_input: { file_path: 'crates/db/src/migrations/008_x.sql' },
        agent_id: '00000000-0000-0000-0000-0000000000d5',
        agent_type: 'impl-coder',
      }),
    );
    assert.equal(r.status, 0, `expected exit 0 under bypass, got ${r.status} stderr=${r.stderr}`);
    assert.doesNotMatch(r.stdout, /sdi_claim_overlap/);
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('SessionStart runs ensureInstalled against workspace binaries and spawns the daemon', { timeout: 20000 }, async () => {
  const home = mkTempHome('sdi-session');
  let pid = null;
  try {
    const env = shimEnv(home, {});
    const r = runShim('session-start.cjs', env, JSON.stringify({ cwd: home }));
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    // Banner has minimal session info.
    assert.match(r.stdout, /SessionStart/);
    assert.match(r.stdout, /No SDI project registered/);
    // Daemon should have written a pid file under the temp SDI_HOME.
    const pidFile = path.join(home, '.cache/sdi/sdid.pid');
    const portFile = path.join(home, '.cache/sdi/sdid.port');
    assert.ok(fs.existsSync(pidFile), 'expected daemon pid file');
    assert.ok(fs.existsSync(portFile), 'expected daemon port file');
    pid = parseInt(fs.readFileSync(pidFile, 'utf8').trim(), 10);
  } finally {
    // Tear down the daemon we spawned.
    if (pid && Number.isFinite(pid)) {
      try {
        process.kill(pid, 'SIGTERM');
      } catch {}
    }
    fs.rmSync(home, { recursive: true, force: true });
  }
});
