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
const { spawnSync } = require('node:child_process');

const PLUGIN_ROOT = path.resolve(__dirname, '..');
const WORKSPACE_ROOT = path.resolve(PLUGIN_ROOT, '..');
const SHARED = path.join(PLUGIN_ROOT, 'adapters/shared/sdi-hooks.cjs');
const SHIM_DIR = path.join(PLUGIN_ROOT, 'adapters/claude');

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

// ────────────────────────────────────────────────────────────────────────────
// Shim error-safety: every shim exits 0 even if the shared module throws.

test('shim wraps: PreToolUse exits 0 with no active task (deny path, not crash)', () => {
  const home = mkTempHome('sdi-pretool');
  try {
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '' });
    // Don't set SDI_ACTIVE_TASK / CLAUDE_ACTIVE_TASK.
    const r = runShim('pre-tool-use.cjs', env, JSON.stringify({ tool_name: 'Edit' }));
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    // Should print the deny JSON payload on stdout.
    assert.match(r.stdout, /permissionDecision.*deny/);
    assert.match(r.stdout, /no active task/);
  } finally {
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
