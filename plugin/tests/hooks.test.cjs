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
  const inFlight = o.inFlight || []; // in_progress tasks for the active plan (#9)
  const chores = o.chores || []; // in_progress chores for the project (#18)
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
    if (u.pathname === `/projects/${project.id}/handoff`) {
      return send(200, {
        active_plan: plan,
        scenarios: o.handoffScenarios || [],
        in_flight_tasks: inFlight,
        backlog_tasks: o.backlog || [],
        recent_decisions: o.decisions || [],
        recent_activity: o.activity || [],
      });
    }
    if (u.pathname === `/projects/${project.id}/next`) {
      return send(200, {
        project,
        active_plan: plan,
        command: o.nextCommand || 'sdi round complete <ROUND>',
        reason: o.nextReason || 'all verified',
        provisional_decisions: o.provisional || [],
      });
    }
    if (plan && u.pathname === `/plans/${plan.id}/tasks/in-flight`) {
      return send(200, { tasks: inFlight });
    }
    if (u.pathname === `/projects/${project.id}/chores`) {
      return send(200, { tasks: chores });
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
  // Seven `sdi-` prefixed skills, all self-contained. Lock-step contract
  // enforced across three sources: SDI_SKILLS array, skills/<name>/SKILL.md
  // files, plugin.json#skillsList. `sdi-init` (cold-start) precedes the
  // `sdi-converge` it hands off to.
  assert.deepEqual(_internals.SDI_SKILLS, [
    'sdi-overview',
    'sdi-scenario',
    'sdi-round',
    'sdi-evidence',
    'sdi-init',
    'sdi-converge',
    'sdi-impl-loop',
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
  const codexManifest = JSON.parse(
    fs.readFileSync(path.join(PLUGIN_ROOT, '.codex-plugin/plugin.json'), 'utf8'),
  );
  const names = (manifest.skillsList || []).map((s) => s.name);
  for (const name of _internals.SDI_SKILLS) {
    assert.ok(
      names.includes(name),
      `plugin.json#skillsList must include "${name}"`,
    );
  }
  assert.equal(codexManifest.skills, './skills/');
  assert.equal(codexManifest.version, manifest.version);
});

test('shared module: pluginRoot prefers PLUGIN_ROOT and falls back to CLAUDE_PLUGIN_ROOT', () => {
  delete require.cache[require.resolve(SHARED)];
  const { pluginRoot } = require(SHARED);
  const prevPluginRoot = process.env.PLUGIN_ROOT;
  const prevClaudePluginRoot = process.env.CLAUDE_PLUGIN_ROOT;
  try {
    process.env.PLUGIN_ROOT = '/tmp/sdi-codex-root';
    process.env.CLAUDE_PLUGIN_ROOT = '/tmp/sdi-claude-root';
    assert.equal(pluginRoot(), '/tmp/sdi-codex-root');

    delete process.env.PLUGIN_ROOT;
    assert.equal(pluginRoot(), '/tmp/sdi-claude-root');
  } finally {
    if (prevPluginRoot === undefined) delete process.env.PLUGIN_ROOT;
    else process.env.PLUGIN_ROOT = prevPluginRoot;
    if (prevClaudePluginRoot === undefined) delete process.env.CLAUDE_PLUGIN_ROOT;
    else process.env.CLAUDE_PLUGIN_ROOT = prevClaudePluginRoot;
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
// #18 — chore lane satisfies the active-task gate.

test('#18: inFlightChores + hasActiveTaskContext read the chore lane', async () => {
  const home = mkTempHome('sdi-chore-unit');
  const chore = { id: 'TASK-chore1', kind: 'chore', status: 'in_progress' };
  const { server, port } = await startMockSdiDaemon({ plan: null, chores: [chore] });
  const prevHome = process.env.SDI_HOME;
  try {
    pinDaemonPort(home, port);
    process.env.SDI_HOME = home;
    delete require.cache[require.resolve(SHARED)];
    const { _internals } = require(SHARED);
    const project = { id: 'PROJ-test' };

    const chores = await _internals.inFlightChores(project.id);
    assert.equal(chores.length, 1);
    assert.equal(chores[0].id, 'TASK-chore1');

    // No active plan, but a chore is in flight → context is satisfied (#18).
    const has = await _internals.hasActiveTaskContext(project);
    assert.equal(has, true);
  } finally {
    if (prevHome === undefined) delete process.env.SDI_HOME;
    else process.env.SDI_HOME = prevHome;
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('#18: hasActiveTaskContext is false with no plan and no chores', async () => {
  const home = mkTempHome('sdi-chore-empty');
  const { server, port } = await startMockSdiDaemon({ plan: null, chores: [] });
  const prevHome = process.env.SDI_HOME;
  try {
    pinDaemonPort(home, port);
    process.env.SDI_HOME = home;
    delete require.cache[require.resolve(SHARED)];
    const { _internals } = require(SHARED);
    const has = await _internals.hasActiveTaskContext({ id: 'PROJ-test' });
    assert.equal(has, false);
  } finally {
    if (prevHome === undefined) delete process.env.SDI_HOME;
    else process.env.SDI_HOME = prevHome;
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('#18: PreToolUse allows a sub-agent Edit when a chore is in flight (no active plan)', async () => {
  const home = mkTempHome('sdi-chore-pretool');
  const chore = { id: 'TASK-chore1', kind: 'chore', status: 'in_progress' };
  const { server, port } = await startMockSdiDaemon({ plan: null, chores: [chore] });
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '' });
    // Registered sub-agent (agent_id present) so the D21 delegation gate passes
    // and the active-task gate is what's under test. The chore lane satisfies it.
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
    // Active-task gate satisfied → no "no active task" deny payload.
    assert.doesNotMatch(r.stdout, /no active task/);
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

// #11: Monitor runs an arbitrary shell command, so it is gated exactly like
// Bash — a mutating Monitor command is denied (it used to be a silent hole).
test('D21: PreToolUse gates main Monitor like Bash (#11 paradox)', async () => {
  const home = mkTempHome('sdi-d21-monitor');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_DELEGATION_BYPASS: '' });
    const mut = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Monitor',
        tool_input: { command: 'while true; do rm -rf /tmp/x; done' },
      }),
    );
    assert.equal(mut.status, 0, `stderr=${mut.stderr}`);
    assert.match(mut.stdout, /permissionDecision.*deny/);
    assert.match(mut.stdout, /D21 delegation gate/);
    // A read-only Monitor (polling) passes the same gate.
    const ro = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Monitor',
        tool_input: { command: 'gh run list 2>/dev/null' },
      }),
    );
    assert.equal(ro.status, 0, `stderr=${ro.stderr}`);
    assert.doesNotMatch(ro.stdout, /D21 delegation gate/);
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

// isReadOnlyBash is quote-aware: metacharacters inside quoted arguments are
// string content, not shell operators. The sdi CLI takes natural-language
// arguments (GWT clauses, `--reason` text) by design, so without this the
// gate blocks its own escape hatch (`sdi bypass arm --reason "(…)"`) and
// ordinary scenario authoring — the structural defect behind GH issue #3.
test('D21: isReadOnlyBash — quote-aware scan + chain handling', () => {
  const { isReadOnlyBash } = require(SHARED)._internals;

  // sdi CLI with metacharacters inside quotes (the original failure mode).
  assert.equal(isReadOnlyBash('sdi scenario create --given "a user (admin) exists" --when "x" --then "y"'), true);
  assert.equal(isReadOnlyBash("sdi bypass arm --reason '(claim overlap) manual fix'"), true);
  assert.equal(isReadOnlyBash('sdi bypass arm --reason "fix D21 gate (hooks)"'), true);

  // fd duplication is not a file redirect.
  assert.equal(isReadOnlyBash('sdi daemon status 2>&1'), true);

  // Read-only chains: every segment whitelisted → allowed.
  assert.equal(isReadOnlyBash('ls plugin && grep -rn isReadOnlyBash plugin'), true);
  assert.equal(isReadOnlyBash('grep -c foo file.txt | wc -l'), true);
  assert.equal(isReadOnlyBash('git status; git log'), true);
  assert.equal(isReadOnlyBash('sdi plan list && sdi task list'), true);

  // Mixed chains: any non-whitelisted segment poisons the whole command.
  assert.equal(isReadOnlyBash('sdi plan list && rm -rf /tmp/x'), false);
  assert.equal(isReadOnlyBash('ls | xargs rm'), false);
  assert.equal(isReadOnlyBash('git status; cargo build'), false);

  // Substitution / redirection / subshell outside quotes still disqualify.
  assert.equal(isReadOnlyBash('echo $(rm -rf /)'), false);
  assert.equal(isReadOnlyBash('sdi bypass arm --reason "uses $(date)"'), false); // $ live in double quotes
  assert.equal(isReadOnlyBash("sdi bypass arm --reason '$(date) is inert here'"), true); // inert in single quotes
  assert.equal(isReadOnlyBash('echo `whoami`'), false);
  assert.equal(isReadOnlyBash('cat foo > bar'), false);
  assert.equal(isReadOnlyBash('grep x < input'), false);
  assert.equal(isReadOnlyBash('(ls)'), false);

  // Background execution and unbalanced quotes disqualify.
  assert.equal(isReadOnlyBash('ls &'), false);
  assert.equal(isReadOnlyBash('sdi scenario create --given "unbalanced'), false);

  // Pre-existing verb rules unchanged.
  assert.equal(isReadOnlyBash('git status'), true);
  assert.equal(isReadOnlyBash('git push'), false);
  assert.equal(isReadOnlyBash('cargo check'), true);
  assert.equal(isReadOnlyBash('cargo build'), false);
  assert.equal(isReadOnlyBash('find . -name "*.rs"'), true);
  assert.equal(isReadOnlyBash('find . -name "*.rs" -delete'), false);
  assert.equal(isReadOnlyBash('find . -name "-delete"'), false); // find acts on args regardless of quoting
  assert.equal(isReadOnlyBash('rm -rf /'), false);
});

// #10 + #4: /dev/null redirects, the cd/export PATH idiom, sdi subcommand
// split (plan/scenario/round/decide = main; task mutation = delegate),
// absolute-path sdi, and read-only gh.
test('D21: isReadOnlyBash — /dev/null, PATH idiom, sdi split, gh (#4/#10)', () => {
  const { isReadOnlyBash } = require(SHARED)._internals;

  // #10 — discarding redirects touch no file.
  assert.equal(isReadOnlyBash('sdi plan list ID 2>/dev/null'), true);
  assert.equal(isReadOnlyBash('sdi plan list ID 2>/dev/null || sdi plan list'), true);
  assert.equal(isReadOnlyBash('cat foo.json >/dev/null 2>&1'), true);
  assert.equal(isReadOnlyBash('ls dirA dirB'), true); // multi-arg
  assert.equal(isReadOnlyBash('cat x.json; ls y/'), true);

  // #4 — cd / export / VAR= prefixes + the PATH-setup idiom (bare $VAR is fine).
  assert.equal(isReadOnlyBash('cd /repo'), true);
  assert.equal(isReadOnlyBash('export PATH="$P/bin:$PATH"'), true);
  assert.equal(isReadOnlyBash('cd /repo; export PATH="$P/bin:$PATH"; sdi plan create P SC-1'), true);
  assert.equal(isReadOnlyBash('FOO=bar sdi plan list'), true); // env prefix stripped, real verb judged
  assert.equal(isReadOnlyBash('FOO=bar rm -rf /'), false); // prefix must not whitelist the payload
  assert.equal(isReadOnlyBash('FOO=bar'), true); // bare assignment segment

  // #4 — sdi subcommand split: orchestration authoring allowed for main,
  // task lifecycle mutation delegated.
  assert.equal(isReadOnlyBash('sdi plan create P "title"'), true);
  assert.equal(isReadOnlyBash('sdi scenario create P SC-1 --given g --when w --then t'), true);
  assert.equal(isReadOnlyBash('sdi round activate ROUND-1'), true);
  assert.equal(isReadOnlyBash('sdi decide create P --title t'), true);
  assert.equal(isReadOnlyBash('sdi task list ROUND-1'), true); // read-only task
  assert.equal(isReadOnlyBash('sdi task create ROUND-1 T-1 desc'), false); // task mutation → delegate
  assert.equal(isReadOnlyBash('sdi task complete TASK-1 --evidence x'), false);
  assert.equal(isReadOnlyBash('sdi bypass arm --reason "x"'), true);

  // #4 — absolute/relative path to the bundled binary.
  assert.equal(isReadOnlyBash('/home/u/.claude/plugins/cache/sdi/bin/sdi plan active P'), true);
  assert.equal(isReadOnlyBash('/home/u/.claude/plugins/cache/sdi/bin/sdi task create R T d'), false);

  // #4c — read-only gh; mutations delegate.
  assert.equal(isReadOnlyBash('gh repo list scenario-driven --limit 10'), true);
  assert.equal(isReadOnlyBash('gh issue view 12'), true);
  assert.equal(isReadOnlyBash('gh pr checks 5'), true);
  assert.equal(isReadOnlyBash('gh auth status'), true);
  assert.equal(isReadOnlyBash('gh api repos/o/r/issues'), true); // default GET
  assert.equal(isReadOnlyBash('gh api repos/o/r/issues -X POST'), false);
  assert.equal(isReadOnlyBash('gh issue create --title x'), false);
  assert.equal(isReadOnlyBash('gh pr merge 5'), false);
});

test('D21 (#18): git global flags (-C / -c / --no-pager) reach the real subcommand', () => {
  delete require.cache[require.resolve(SHARED)];
  const { isReadOnlyBash } = require(SHARED)._internals;
  // Global options precede the subcommand — judge the subcommand, not the flag.
  assert.equal(isReadOnlyBash('git -C /repo remote -v'), true);
  assert.equal(isReadOnlyBash('git -C /repo status'), true);
  assert.equal(isReadOnlyBash('git --no-pager log --oneline -5'), true);
  assert.equal(isReadOnlyBash('git -c color.ui=always diff'), true);
  assert.equal(isReadOnlyBash('git -C /a -c x=y rev-parse HEAD'), true);
  // A mutating subcommand after global flags is still blocked.
  assert.equal(isReadOnlyBash('git -C /repo push'), false);
  assert.equal(isReadOnlyBash('git -C /repo checkout main -- file'), false);
  // Bare git mutations remain blocked (regression anchor).
  assert.equal(isReadOnlyBash('git commit -m x'), false);
  assert.equal(isReadOnlyBash('git status'), true);
});

test('install gate (#17): pluginVersion reads the host plugin manifest', () => {
  delete require.cache[require.resolve(SHARED)];
  const { pluginVersion, readManifestVersion } = require(SHARED)._internals;
  // Against this repo's own plugin/ root, returns the manifest version string.
  const v = pluginVersion(PLUGIN_ROOT);
  assert.match(v, /^\d+\.\d+\.\d+$/, `expected semver, got ${v}`);
  assert.equal(v, readManifestVersion(PLUGIN_ROOT, '.codex-plugin/plugin.json'));
  // Unknown root → null (graceful, never throws).
  assert.equal(pluginVersion('/nonexistent/path'), null);
});

test('D13: parseRoundDecomposeIntent — round activate / task create seams', () => {
  delete require.cache[require.resolve(SHARED)];
  const { _internals } = require(SHARED);
  const parse = _internals.parseRoundDecomposeIntent;

  // round activate → kind=activate, positional round id.
  assert.deepEqual(parse('sdi round activate ROUND-123'), {
    kind: 'activate',
    roundId: 'ROUND-123',
    hasPatternFlag: false,
  });

  // task create → kind=create, round id is positional arg 1 (before the
  // quoted description and the --scenario flag).
  assert.deepEqual(
    parse('sdi task create ROUND-9 T-1 "wire the thing" --scenario SCN-1'),
    { kind: 'create', roundId: 'ROUND-9', hasPatternFlag: false },
  );

  // A create already carrying the binding flag is detected (advisory suppresses).
  assert.equal(
    parse('sdi task create R-1 T-2 "x" --produced-via-pattern PAT-7').hasPatternFlag,
    true,
  );
  assert.equal(parse('sdi task create R-1 T-2 "x" --pattern PAT-7').hasPatternFlag, true);

  // Absolute-path bundled binary (delegated sub-agents invoke this form).
  assert.deepEqual(parse('/plugins/sdi/bin/sdi round activate RR'), {
    kind: 'activate',
    roundId: 'RR',
    hasPatternFlag: false,
  });

  // Survives a `cd … && export … &&` prefix chain (#4 idiom).
  assert.equal(
    parse('cd /repo && export PATH=$P && sdi task create R5 T9 "go"').roundId,
    'R5',
  );

  // Unrelated sdi reads / other verbs do not match.
  assert.equal(parse('sdi task list --round R-1'), null);
  assert.equal(parse('sdi round list'), null);
  assert.equal(parse('git status'), null);
});

test('D21: PreToolUse allows sdi CLI with quoted metachars through delegation gate', async () => {
  const home = mkTempHome('sdi-d21-quoted');
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
        tool_input: { command: 'sdi bypass arm --reason "fix D21 gate (hooks)"' },
      }),
    );
    assert.equal(r.status, 0);
    assert.doesNotMatch(r.stdout, /D21 delegation gate/, 'quoted metachars must not read as operators');
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

// #11: an unregistered agent_type is NO LONGER hard-blocked. It acts at L3 —
// read-only + execution work allowed (consensus autonomy stays structurally
// out of reach because it needs a registered (name, stance) tuple). The
// previous deny was a deadlock with no escape hatch; the new behaviour emits a
// one-line L3 advisory and proceeds.
test('D21: PreToolUse lets unregistered sub-agent act at L3 (advisory, not deny)', async () => {
  const home = mkTempHome('sdi-d21-l3');
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
    // Not blocked: with an active task and a sub-agent id, Edit proceeds.
    assert.doesNotMatch(r.stdout, /permissionDecision.*deny/);
    assert.doesNotMatch(r.stdout, /rogue-specialist/);
    // L3 advisory surfaces on stderr + audit log.
    assert.match(r.stderr, /unregistered.*L3/i);
    const log = path.join(home, '.local/state/sdi/hook.log');
    const entries = fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => JSON.parse(l));
    assert.ok(
      entries.some(
        (e) => e.event === 'pre_tool_use_unregistered_agent' && e.reason === 'l3-autonomy-cap',
      ),
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

// #9: the active-task gate is satisfied by DAEMON STATE (an in_progress task
// in the active plan), not the unsatisfiable SDI_ACTIVE_TASK env. A specialist
// that ran `sdi task start` can now Edit without setting any env.
test('D21: active-task gate passes on daemon in_progress task, no env (#9)', async () => {
  const home = mkTempHome('sdi-active-daemon');
  const { server, port } = await startMockSdiDaemon({
    plan: { id: 'PLAN-x', title: 'p', status: 'active' },
    inFlight: [{ id: 'TASK-x', status: 'in_progress' }],
  });
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_ACTIVE_TASK: '' });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Edit',
        agent_id: '00000000-0000-0000-0000-0000000000aa',
        agent_type: 'impl-coder',
      }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    assert.doesNotMatch(r.stdout, /no active task/);
    assert.doesNotMatch(r.stdout, /permissionDecision.*deny/);
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

// Conversely, an active plan with NO in_progress task still denies (the gate
// is real — it just reads daemon state instead of env).
test('D21: active-task gate still denies when no in_progress task exists (#9)', async () => {
  const home = mkTempHome('sdi-active-none');
  const { server, port } = await startMockSdiDaemon({
    plan: { id: 'PLAN-x', title: 'p', status: 'active' },
    inFlight: [],
  });
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_ACTIVE_TASK: '' });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Write',
        agent_id: '00000000-0000-0000-0000-0000000000ab',
        agent_type: 'impl-coder',
      }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    assert.match(r.stdout, /no active task/);
    assert.match(r.stdout, /sdi task start/);
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

// Regression — Claude Code dispatches namespaced agent types ("sdi:impl-coder").
// The rogue-specialist guard must compare against the bare AgentSpec name from
// frontmatter, not the raw namespaced string. Before the normalization fix the
// guard wrongly blocked every legitimate plugin specialist (every Edit call
// from `sdi:impl-coder` denied with rogue-specialist), so this case stays as a
// permanent regression anchor.
test('D21: PreToolUse accepts namespace-prefixed sub-agent type (sdi:impl-coder)', async () => {
  const home = mkTempHome('sdi-d21-namespaced');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '' });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Edit',
        agent_id: '00000000-0000-0000-0000-000000000003',
        agent_type: 'sdi:impl-coder',
      }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    assert.doesNotMatch(r.stdout, /rogue-specialist/);
    assert.doesNotMatch(r.stdout, /D21 delegation gate/);
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
    // SDI_DELEGATION_BYPASS=1 + bypass marker now unlock every mutating gate
    // (D21 delegation, active-task, D29) — one consciously armed emergency
    // bypass clears every block in one shot. The active-task gate that used
    // to fire next is also bypassed; only "allow" remains.
    assert.doesNotMatch(r.stdout, /D21 delegation gate/);
    assert.doesNotMatch(r.stdout, /no active task/);
    // stderr surfaces the bypass warning.
    assert.match(r.stderr, /SDI_DELEGATION_BYPASS=1/);
    const log = path.join(home, '.local/state/sdi/hook.log');
    const entries = fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => JSON.parse(l));
    assert.ok(
      entries.some(
        (e) => e.event === 'pre_tool_use_delegation_bypass' && e.source === 'env',
      ),
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

// Real-environment simulation. Claude Code spawns PreToolUse before user
// shell expands the inline `VAR=1 cmd` prefix, so SDI_DELEGATION_BYPASS never
// reaches the hook. The marker file at ~/.cache/sdi/bypass-once exists for
// this case — substrate both sides own, naturally one-shot via auto-delete.
test('D21: bypass-once marker file unblocks main + is consumed (one-shot)', async () => {
  const home = mkTempHome('sdi-d21-marker');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    const marker = path.join(home, '.cache/sdi/bypass-once');
    fs.mkdirSync(path.dirname(marker), { recursive: true });
    fs.writeFileSync(marker, 'investigating D21 env propagation\n');
    // Explicit empty env for SDI_DELEGATION_BYPASS — simulates Claude Code
    // not propagating shell env to the hook spawn.
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_DELEGATION_BYPASS: '' });

    const r1 = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({ cwd: PROJECT_CWD, tool_name: 'Edit' }),
    );
    assert.equal(r1.status, 0, `stderr=${r1.stderr}`);
    // One armed marker unlocks every mutating gate (D21 + active-task + D29)
    // for one invocation — no blocking JSON should be emitted.
    assert.doesNotMatch(r1.stdout, /D21 delegation gate/);
    assert.doesNotMatch(r1.stdout, /no active task/);
    assert.match(r1.stderr, /D21 bypass via marker/);
    assert.match(r1.stderr, /investigating D21 env propagation/);
    // Marker auto-consumed — file must be gone.
    assert.equal(fs.existsSync(marker), false, 'marker should be deleted after one hit');

    const log = path.join(home, '.local/state/sdi/hook.log');
    const entries = fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => JSON.parse(l));
    const bypassEntry = entries.find((e) => e.event === 'pre_tool_use_delegation_bypass');
    assert.ok(bypassEntry, 'audit entry missing');
    assert.equal(bypassEntry.source, 'marker');
    assert.equal(bypassEntry.reason, 'investigating D21 env propagation');

    // Second invocation — marker already consumed, so D21 must re-engage.
    const r2 = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({ cwd: PROJECT_CWD, tool_name: 'Edit' }),
    );
    assert.match(r2.stdout, /D21 delegation gate/, 'second call should be blocked again');
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

// D21 deny message must point users at the recommended bypass surface —
// `sdi bypass arm`, the daemon-friendly CLI verb. `sdi` is on the D21
// read-only Bash whitelist so the main session can call it directly,
// breaking the self-deadlock where the only way to clear the gate was a
// `touch` (mutating Bash, itself blocked by D21). Regression anchor for
// "deny message must surface the CLI verb, not just a marker path users
// can't reach without delegation."
test('D21: deny message advertises `sdi bypass arm` override surface', async () => {
  const home = mkTempHome('sdi-d21-hint');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_DELEGATION_BYPASS: '' });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({ cwd: PROJECT_CWD, tool_name: 'Edit' }),
    );
    assert.match(r.stdout, /D21 delegation gate/);
    assert.match(r.stdout, /sdi bypass arm/, 'deny message must surface the `sdi bypass arm` CLI verb');
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

test('Codex agents: Claude agent specs generate valid Codex TOML without Claude model leakage', () => {
  delete require.cache[require.resolve(SHARED)];
  const { _internals } = require(SHARED);
  const file = path.join(PLUGIN_ROOT, 'agents', 'pattern-orchestrator.md');
  const agent = _internals.parseClaudeAgentMarkdown(fs.readFileSync(file, 'utf8'), file);
  assert.equal(agent.name, 'pattern-orchestrator');
  assert.equal(agent.model, 'sonnet');

  const toml = _internals.buildCodexAgentToml(agent);
  assert.match(toml, /^# Generated by SDI Codex adapter\./);
  assert.match(toml, /^name = "pattern-orchestrator"$/m);
  assert.match(toml, /^description = ".+"/m);
  assert.match(toml, /^developer_instructions = ".+"/m);
  assert.doesNotMatch(toml, /^model\s*=/m, 'Claude model names must not be copied into Codex');
  assert.match(toml, /Source Claude model "sonnet" is inherited in Codex/);
});

test('Codex agents: install generated TOML and preserve existing user files', () => {
  const home = mkTempHome('sdi-codex-agents');
  const oldCodexHome = process.env.CODEX_HOME;
  try {
    process.env.CODEX_HOME = home;
    delete require.cache[require.resolve(SHARED)];
    const { _internals } = require(SHARED);
    const targetDir = path.join(home, 'agents');
    fs.mkdirSync(targetDir, { recursive: true });
    const userFile = path.join(targetDir, 'impl-coder.toml');
    const userBody = 'name = "impl-coder"\ndescription = "user-owned"\ndeveloper_instructions = "keep"\n';
    fs.writeFileSync(userFile, userBody, 'utf8');

    const result = _internals.installCodexAgents(PLUGIN_ROOT);
    assert.ok(result.total >= 3, 'expected SDI built-in agents to be discovered');
    assert.ok(result.written >= 2, 'expected generated Codex agents to be written');
    assert.ok(result.skipped.some((s) => s.name === 'impl-coder'), 'user-owned impl-coder must be skipped');
    assert.equal(fs.readFileSync(userFile, 'utf8'), userBody, 'user-owned agent file must not be overwritten');

    const generated = fs.readFileSync(path.join(targetDir, 'pattern-orchestrator.toml'), 'utf8');
    assert.match(generated, /^# Generated by SDI Codex adapter\./);
    assert.match(generated, /^name = "pattern-orchestrator"$/m);
    assert.doesNotMatch(generated, /^model\s*=/m);

    const names = _internals.loadAgentNamesFromDir(targetDir);
    assert.ok(names.has('pattern-orchestrator'), 'registry must read Codex TOML agent names');
    assert.ok(
      _internals.isRegisteredAgent('pattern-orchestrator', PROJECT_CWD),
      'D21 registry must recognise CODEX_HOME/agents custom agents',
    );
  } finally {
    if (oldCodexHome === undefined) delete process.env.CODEX_HOME;
    else process.env.CODEX_HOME = oldCodexHome;
    fs.rmSync(home, { recursive: true, force: true });
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

// ────────────────────────────────────────────────────────────────────────────
// Unified bypass marker — JSON+TTL shape (current `sdi bypass arm` surface).
//
// `sdi bypass arm` writes a JSON body with `{reason, armed_at, expires_at,
// ttl_seconds}`. The hook reads + parses it, treats `expires_at <= now` as
// expired (cleanup-only, does NOT open the gate), and applies the same
// marker to every mutating gate (D21 / active-task / D29) within a single
// invocation. Plain-text bodies (legacy `touch`) stay backward-compatible.

function writeJsonMarker(home, { reason, ttlSeconds }) {
  const marker = path.join(home, '.cache/sdi/bypass-once');
  fs.mkdirSync(path.dirname(marker), { recursive: true });
  const now = Date.now();
  const body = {
    reason: reason || null,
    armed_at: new Date(now).toISOString(),
    expires_at: new Date(now + ttlSeconds * 1000).toISOString(),
    ttl_seconds: ttlSeconds,
  };
  fs.writeFileSync(marker, JSON.stringify(body, null, 2) + '\n');
  return marker;
}

test('bypass marker (JSON, valid TTL): unlocks D21 + auto-consumes', async () => {
  const home = mkTempHome('sdi-bypass-json-valid');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    const marker = writeJsonMarker(home, { reason: 'emergency hotfix', ttlSeconds: 60 });
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_DELEGATION_BYPASS: '' });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({ cwd: PROJECT_CWD, tool_name: 'Edit' }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    assert.doesNotMatch(r.stdout, /D21 delegation gate/);
    assert.match(r.stderr, /D21 bypass via marker/);
    assert.match(r.stderr, /emergency hotfix/);
    assert.equal(fs.existsSync(marker), false, 'marker should be consumed');
    const log = path.join(home, '.local/state/sdi/hook.log');
    const entries = fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => JSON.parse(l));
    const bypassEntry = entries.find((e) => e.event === 'pre_tool_use_delegation_bypass');
    assert.ok(bypassEntry, 'audit row missing');
    assert.equal(bypassEntry.reason, 'emergency hotfix');
    assert.equal(bypassEntry.source, 'marker');
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('bypass marker (JSON, expired): does NOT unlock; marker is cleaned up', async () => {
  const home = mkTempHome('sdi-bypass-json-expired');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    const marker = path.join(home, '.cache/sdi/bypass-once');
    fs.mkdirSync(path.dirname(marker), { recursive: true });
    // Negative TTL → expires_at in the past.
    const now = Date.now();
    fs.writeFileSync(
      marker,
      JSON.stringify({
        reason: 'stale',
        armed_at: new Date(now - 120000).toISOString(),
        expires_at: new Date(now - 60000).toISOString(),
        ttl_seconds: 60,
      }) + '\n',
    );
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_DELEGATION_BYPASS: '' });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({ cwd: PROJECT_CWD, tool_name: 'Edit' }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    // Expired marker does not open the gate.
    assert.match(r.stdout, /D21 delegation gate/);
    // But it IS cleaned up so it doesn't linger.
    assert.equal(fs.existsSync(marker), false, 'expired marker should be deleted');
    const log = path.join(home, '.local/state/sdi/hook.log');
    const entries = fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => JSON.parse(l));
    assert.ok(
      entries.some((e) => e.event === 'bypass_marker_expired'),
      'expected bypass_marker_expired audit row',
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('bypass marker: same marker unlocks the active-task gate (registered sub-agent, no SDI_ACTIVE_TASK)', async () => {
  const home = mkTempHome('sdi-bypass-active-task');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    writeJsonMarker(home, { reason: 'fixing CI', ttlSeconds: 60 });
    // Registered sub-agent → passes D21. No SDI_ACTIVE_TASK → would normally
    // block on active-task gate. Marker must lift it.
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '' });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Edit',
        agent_id: '00000000-0000-0000-0000-0000000000a1',
        agent_type: 'impl-coder',
      }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    assert.doesNotMatch(r.stdout, /no active task/);
    assert.match(r.stderr, /active-task bypass via marker/);
    const log = path.join(home, '.local/state/sdi/hook.log');
    const entries = fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => JSON.parse(l));
    assert.ok(
      entries.some((e) => e.event === 'pre_tool_use_active_task_bypass' && e.reason === 'fixing CI'),
      `audit log missing active-task bypass entry:\n${JSON.stringify(entries, null, 2)}`,
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('bypass marker: same marker unlocks the D29 claim-overlap gate', async () => {
  const home = mkTempHome('sdi-bypass-claim');
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
    writeJsonMarker(home, { reason: 'cross-scenario coordination', ttlSeconds: 60 });
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
        agent_id: '00000000-0000-0000-0000-0000000000c1',
        agent_type: 'impl-coder',
      }),
    );
    assert.equal(r.status, 0, `expected exit 0 under marker bypass, got ${r.status} stderr=${r.stderr}`);
    assert.doesNotMatch(r.stdout, /sdi_claim_overlap/);
    assert.match(r.stderr, /D29 bypass via marker/);
    const log = path.join(home, '.local/state/sdi/hook.log');
    const entries = fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => JSON.parse(l));
    assert.ok(
      entries.some(
        (e) =>
          e.event === 'pre_tool_use_claim_bypass' &&
          e.reason === 'cross-scenario coordination',
      ),
      `audit log missing claim bypass entry:\n${JSON.stringify(entries, null, 2)}`,
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('bypass marker (plain text, backward compat): treated as armed-forever and consumed', async () => {
  const home = mkTempHome('sdi-bypass-plain');
  const { server, port } = await startMockSdiDaemon();
  try {
    pinDaemonPort(home, port);
    const marker = path.join(home, '.cache/sdi/bypass-once');
    fs.mkdirSync(path.dirname(marker), { recursive: true });
    // Legacy v0.1.4 shape — what `touch ~/.cache/sdi/bypass-once` produced.
    fs.writeFileSync(marker, 'legacy plain-text reason\n');
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_DELEGATION_BYPASS: '' });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({ cwd: PROJECT_CWD, tool_name: 'Edit' }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    assert.doesNotMatch(r.stdout, /D21 delegation gate/);
    assert.match(r.stderr, /legacy plain-text reason/);
    assert.equal(fs.existsSync(marker), false, 'plain-text marker should be consumed too');
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

// ────────────────────────────────────────────────────────────────────────────
// Project soft-disable (v0.3) — `project.enabled === false` collapses every
// downstream mutating gate to the same skip path the unregistered-cwd case
// uses. Mirrors Clawket's `isProjectDisabled` allow-on-disable pattern so the
// user can run `sdi project disable` to step SDI governance aside on a repo.

test('project enabled=false: PreToolUse skips every mutating gate + audits project-disabled', async () => {
  const home = mkTempHome('sdi-project-disabled');
  const { server, port } = await startMockSdiDaemon({
    project: {
      id: 'PROJ-disabled',
      key: 'DIS',
      name: 'Disabled',
      cwd: PROJECT_CWD,
      enabled: false,
    },
  });
  try {
    pinDaemonPort(home, port);
    // Main session calling Edit would normally trip D21 first. With the
    // project disabled, the project-scope gate short-circuits before D21
    // even runs, so no deny payload is emitted.
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_DELEGATION_BYPASS: '' });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({ cwd: PROJECT_CWD, tool_name: 'Edit' }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    assert.doesNotMatch(r.stdout, /D21 delegation gate/);
    assert.doesNotMatch(r.stdout, /no active task/);
    assert.equal(r.stdout, '', 'no deny payload should be emitted for disabled project');
    const log = path.join(home, '.local/state/sdi/hook.log');
    const entries = fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => JSON.parse(l));
    assert.ok(
      entries.some(
        (e) =>
          e.event === 'pre_tool_use_skip' &&
          e.reason === 'project-disabled' &&
          e.project_id === 'PROJ-disabled',
      ),
      `audit log missing project-disabled skip:\n${JSON.stringify(entries, null, 2)}`,
    );
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('project enabled=0 (legacy integer shape): same skip path', async () => {
  const home = mkTempHome('sdi-project-disabled-int');
  const { server, port } = await startMockSdiDaemon({
    project: {
      id: 'PROJ-disabled-int',
      key: 'DI2',
      name: 'Disabled (int)',
      cwd: PROJECT_CWD,
      enabled: 0,
    },
  });
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '' });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({
        cwd: PROJECT_CWD,
        tool_name: 'Edit',
        agent_id: '00000000-0000-0000-0000-0000000000aa',
        agent_type: 'impl-coder',
      }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    assert.equal(r.stdout, '', 'no deny payload should be emitted');
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test('project enabled=true: normal D21 enforcement remains (regression anchor)', async () => {
  const home = mkTempHome('sdi-project-enabled-d21');
  const { server, port } = await startMockSdiDaemon({
    project: {
      id: 'PROJ-enabled',
      key: 'EN1',
      name: 'Enabled',
      cwd: PROJECT_CWD,
      enabled: true,
    },
  });
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, { SDI_BYPASS_HOOKS: '', SDI_DELEGATION_BYPASS: '' });
    const r = await runShimAsync(
      'pre-tool-use.cjs',
      env,
      JSON.stringify({ cwd: PROJECT_CWD, tool_name: 'Edit' }),
    );
    assert.match(r.stdout, /D21 delegation gate/, 'enabled project must keep D21 active');
  } finally {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

// #20 — soft-disable must silence context injection, not just the mutating
// gate. SessionStart/UserPromptSubmit previously ignored `enabled === false`.
// UserPromptSubmit runs no ensureInstalled, so the mock daemon exercises the
// guard directly; SessionStart shares the identical `projectDisabled` guard.
test('project enabled=false: UserPromptSubmit injects no context (#20)', async () => {
  const home = mkTempHome('sdi-ups-disabled');
  const { server, port } = await startMockSdiDaemon({
    project: {
      id: 'PROJ-disabled-ups',
      key: 'DUP',
      name: 'Disabled',
      cwd: PROJECT_CWD,
      enabled: false,
    },
  });
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, {});
    const r = await runShimAsync(
      'user-prompt-submit.cjs',
      env,
      JSON.stringify({ cwd: PROJECT_CWD }),
    );
    assert.equal(r.status, 0, `stderr=${r.stderr}`);
    assert.equal(r.stdout, '', 'disabled project must inject no SDI context');
    const log = path.join(home, '.local/state/sdi/hook.log');
    const entries = fs.readFileSync(log, 'utf8').trim().split('\n').map((l) => JSON.parse(l));
    assert.ok(
      entries.some(
        (e) => e.event === 'user_prompt_submit_skip' && e.reason === 'project-disabled',
      ),
      `audit log missing user_prompt_submit skip:\n${JSON.stringify(entries, null, 2)}`,
    );
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
    // No registered project for this cwd → the register hint is model-only
    // (additionalContext); it must NOT be surfaced as a visible terminal
    // banner (systemMessage) in unrelated directories.
    assert.doesNotMatch(r.stdout, /systemMessage/);
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

// SessionStart work summary (Clawket-style banner): plan + scenario counts +
// tasks + decisions + the daemon-computed next step (#15) + recent activity.
test('SessionStart: buildSessionSummary renders a rich work banner', async () => {
  const home = mkTempHome('sdi-sess-summary');
  const { server, port } = await startMockSdiDaemon({
    plan: { id: 'PLAN-x', short_code: 'P-1', title: 'demo plan', status: 'active' },
    handoffScenarios: [
      { id: 'S1', status: 'confirmed' },
      { id: 'S2', status: 'confirmed' },
      { id: 'S3', status: 'draft' },
      { id: 'S4', status: 'confirmed', retired_at: '2026-06-16T00:00:00Z' },
    ],
    inFlight: [{ short_code: 'T-1', description: 'wire the thing', status: 'in_progress' }],
    decisions: [{ short_code: 'DEC-1', supersede_when: 'team disagrees' }],
    activity: [{ kind: 'task.updated', summary: 'started T-1' }],
    nextCommand: 'sdi task brief TASK-1',
    nextReason: 'a task is in progress',
    provisional: [{ short_code: 'DEC-1', supersede_when: 'team disagrees' }],
  });
  try {
    pinDaemonPort(home, port);
    const env = shimEnv(home, {});
    process.env.SDI_HOME = home; // daemonBase reads the pinned port under SDI_HOME
    const mod = require(SHARED);
    const project = { id: 'PROJ-test', name: 'Demo', key: 'TEST' };
    // Plain mode (model-facing additionalContext) — no ANSI escapes, stable text.
    const banner = await mod._internals.buildSessionSummary(project);
    assert.match(banner, /SDI · Demo \(TEST\)/);
    assert.match(banner, /plan: P-1 · demo plan/);
    assert.match(banner, /scenarios: 2 confirmed · 1 draft · 1 retired/);
    assert.match(banner, /tasks: 1 in-flight · 0 backlog/);
    assert.match(banner, /▸ T-1 wire the thing/);
    assert.match(banner, /decisions: 1 · 1 provisional/);
    assert.match(banner, /↳ next: sdi task brief TASK-1/);
    assert.match(banner, /a task is in progress/);
    assert.match(banner, /revisit DEC-1 when: team disagrees/);
    assert.match(banner, /recent:/);
    // Plain mode must NOT contain ANSI escape codes (would pollute model context).
    assert.doesNotMatch(banner, /\x1b\[/, 'plain banner must have no ANSI codes');

    // Coloured mode (terminal systemMessage) — same content, wrapped in ANSI.
    const coloured = await mod._internals.buildSessionSummary(project, { ansi: true });
    assert.match(coloured, /\x1b\[/, 'coloured banner must contain ANSI codes');
    assert.match(coloured, /2 confirmed/);
    assert.match(coloured, /sdi task brief TASK-1/);
    // Stripping ANSI from the coloured banner yields the same text as plain.
    // eslint-disable-next-line no-control-regex
    assert.equal(coloured.replace(/\x1b\[[0-9;]*m/g, ''), banner);
    assert.ok(env); // keep env referenced
  } finally {
    delete process.env.SDI_HOME;
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  }
});

// The fix for "SDI summary never shows in the terminal": the work banner must
// go out as `systemMessage` (Claude Code renders that as the visible
// "SessionStart … says:" line), not only as model-context `additionalContext`.
test('SessionStart: payload carries plain additionalContext + optional coloured systemMessage', () => {
  delete require.cache[require.resolve(SHARED)];
  const { _internals } = require(SHARED);
  const make = _internals.sessionStartPayload;

  // Registered project → plain text goes to the model (additionalContext) while
  // the COLOURED banner is shown to the user (systemMessage). They differ.
  const withProject = make('SDI · Demo (TEST)\nplan: …\n', '\x1b[36mSDI\x1b[0m · Demo\n');
  assert.equal(withProject.hookSpecificOutput.additionalContext, 'SDI · Demo (TEST)\nplan: …\n');
  assert.equal(withProject.systemMessage, '\x1b[36mSDI\x1b[0m · Demo\n');

  // No project → model-only hint, NO visible banner (SDI's hook runs in every
  // cwd; a banner in unrelated dirs would be noise).
  const noProject = make('# SDI session\nNo SDI project registered\n', null);
  assert.equal(noProject.hookSpecificOutput.additionalContext, '# SDI session\nNo SDI project registered\n');
  assert.ok(!('systemMessage' in noProject), 'no-project payload must omit systemMessage');
});
