'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const http = require('node:http');
const { spawn } = require('node:child_process');
const { evaluateCommandPolicy, loadCommandPolicy, resolvePolicyRoot } = require('../adapters/shared/command-policy.cjs');
const { classifyBash } = require('../adapters/shared/bash-classifier.cjs');

const PLUGIN = path.resolve(__dirname, '..');
const SHIM = path.join(PLUGIN, 'adapters/claude/pre-tool-use.cjs');

function fixture(t) {
  const home = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'sdi-policy-')));
  t.after(() => fs.rmSync(home, { recursive: true, force: true }));
  const root = path.join(home, 'project');
  fs.mkdirSync(root);
  const project = { id: 'policy-project', cwd: root, enabled: true };
  return { home, root, project, evaluate: (command, cwd = root) => evaluateCommandPolicy(command, { cwd, project }) };
}

function writePolicy(root, allow, local = false) {
  fs.mkdirSync(path.join(root, '.sdi'), { recursive: true });
  const file = path.join(root, '.sdi', local ? 'settings.local.json' : 'settings.json');
  fs.writeFileSync(file, JSON.stringify({ permissions: { allow } }));
  return file;
}

test('missing configuration leaves unknown writes denied and classifier unchanged', (t) => {
  const f = fixture(t);
  assert.equal(f.evaluate('fixture-tool send').allowedByPolicy, false);
  assert.deepEqual(f.evaluate('fixture-tool send').diagnostics, []);
  assert.equal(classifyBash('fixture-tool send').readOnly, false);
  assert.equal(classifyBash('git status').readOnly, true);
});

test('shared and personal grants merge, deduplicate, reload, and never reuse stale grants', (t) => {
  const f = fixture(t);
  writePolicy(f.root, ['fixture-tool shared', 'fixture-tool common']);
  const personal = writePolicy(f.root, ['fixture-tool personal', 'fixture-tool common'], true);
  assert.equal(f.evaluate('fixture-tool shared').allowedByPolicy, true);
  assert.equal(f.evaluate('fixture-tool personal').allowedByPolicy, true);
  const loaded = loadCommandPolicy({ cwd: f.root, project: f.project });
  assert.equal(loaded.rules.filter((entry) => entry.rule === 'fixture-tool common').length, 1);
  const next = `${personal}.next`;
  fs.writeFileSync(next, JSON.stringify({ permissions: { allow: ['fixture-tool replacement'] } }));
  fs.renameSync(next, personal);
  assert.equal(f.evaluate('fixture-tool personal').allowedByPolicy, false);
  assert.equal(f.evaluate('fixture-tool replacement').allowedByPolicy, true);
  fs.writeFileSync(personal, '{"permissions":');
  const invalid = f.evaluate('fixture-tool shared');
  assert.equal(invalid.allowedByPolicy, false);
  assert.ok(invalid.diagnostics.some((entry) => entry.source === personal && entry.reason));
  fs.unlinkSync(personal);
  assert.equal(f.evaluate('fixture-tool shared').allowedByPolicy, true);
});

for (const invalid of [null, [], { permissions: [] }, { permissions: { allow: 'fixture-tool *' } }, { permissions: { allow: [5] } }, { permissions: { allow: [''] } }, { permissions: { allow: ['fixture-tool * extra'] } }, { permissions: { allow: ['fixture-tool send; other-tool'] } }]) {
  test(`invalid present configuration invalidates both lists: ${JSON.stringify(invalid)}`, (t) => {
    const f = fixture(t);
    writePolicy(f.root, ['fixture-tool send']);
    const personal = writePolicy(f.root, [], true);
    fs.writeFileSync(personal, JSON.stringify(invalid));
    const result = f.evaluate('fixture-tool send');
    assert.equal(result.allowedByPolicy, false);
    assert.ok(result.diagnostics.some((entry) => entry.source === personal && entry.reason));
  });
}

test('an unreadable present configuration prevents grants from the other file', (t) => {
  const f = fixture(t);
  writePolicy(f.root, ['fixture-tool send']);
  const unreadable = path.join(f.root, '.sdi/settings.local.json');
  fs.mkdirSync(unreadable);
  const result = f.evaluate('fixture-tool send');
  assert.equal(result.allowedByPolicy, false);
  assert.ok(result.diagnostics.some((entry) => entry.source === unreadable && entry.reason));
});

test('every rule is validated before an earlier valid rule may grant permission', (t) => {
  const f = fixture(t);
  writePolicy(f.root, ['fixture-tool send', 'fixture-tool * unsupported']);
  const result = f.evaluate('fixture-tool send');
  assert.equal(result.allowedByPolicy, false);
  assert.ok(result.diagnostics.length > 0);
});

test('policy selects deepest registered real root and ignores invocation and ancestor files', (t) => {
  const f = fixture(t);
  const nested = path.join(f.root, 'nested');
  const child = path.join(nested, 'src');
  fs.mkdirSync(child, { recursive: true });
  writePolicy(f.home, ['fixture-tool ancestor']);
  writePolicy(f.root, ['fixture-tool outer']);
  writePolicy(nested, ['fixture-tool nested']);
  writePolicy(child, ['fixture-tool child']);
  const project = { ...f.project, cwds: [f.root, nested] };
  const context = { cwd: child, project };
  assert.equal(resolvePolicyRoot(child, project), nested);
  assert.equal(evaluateCommandPolicy('fixture-tool nested', context).allowedByPolicy, true);
  for (const name of ['ancestor', 'outer', 'child']) {
    assert.equal(evaluateCommandPolicy(`fixture-tool ${name}`, context).allowedByPolicy, false, name);
  }
  const alias = path.join(f.home, 'alias');
  fs.symlinkSync(nested, alias, 'dir');
  assert.equal(resolvePolicyRoot(path.join(alias, 'src'), project), nested);
  assert.equal(evaluateCommandPolicy('fixture-tool nested', { cwd: path.join(alias, 'src'), project }).allowedByPolicy, true);
  assert.equal(evaluateCommandPolicy('fixture-tool outer', { cwd: f.home, project }).allowedByPolicy, false);
});

test('host permission settings are never imported as command exceptions', (t) => {
  const f = fixture(t);
  fs.mkdirSync(path.join(f.root, '.claude'));
  fs.writeFileSync(path.join(f.root, '.claude/settings.json'), JSON.stringify({ permissions: { allow: ['Bash(*)'] } }));
  fs.mkdirSync(path.join(f.root, '.codex/rules'), { recursive: true });
  fs.writeFileSync(path.join(f.root, '.codex/rules/default.rules'), 'prefix_rule(pattern=["fixture-tool"], decision="allow")');
  assert.equal(f.evaluate('fixture-tool send').allowedByPolicy, false);
});

test('exact argument values match quoting but never absorb extra arguments or spoofed names', (t) => {
  const f = fixture(t);
  writePolicy(f.root, ['fixture-tool send "two words"', 'fixture-tool literal "*"']);
  for (const command of ["fixture-tool send 'two words'", 'fixture-tool send two\\ words', "fixture-tool literal '*'"]) {
    assert.equal(f.evaluate(command).allowedByPolicy, true, command);
  }
  for (const command of ['fixture-tool send two words', 'fixture-tool send "two words" extra', 'fixture-tool-other send "two words"', '/bin/fixture-tool send "two words"', 'fixture-tool literal arbitrary', 'fixture-tool send "$VALUE"']) {
    assert.equal(f.evaluate(command).allowedByPolicy, false, command);
  }
});

test('terminal wildcard allows zero or many trailing arguments and user-owned writes', (t) => {
  const f = fixture(t);
  writePolicy(f.root, ['fixture-tool send *', 'mkdir *']);
  for (const command of ['fixture-tool send', 'fixture-tool send --queue jobs --body "hello world"', 'mkdir output', 'mkdir -p deeply/nested']) {
    assert.equal(f.evaluate(command).allowedByPolicy, true, command);
    assert.equal(classifyBash(command).readOnly, false, command);
  }
  for (const command of ['fixture-tool sender data', 'fixture-tool-other send', 'env fixture-tool send', 'fixture-tool "$ACTION" body']) {
    assert.equal(f.evaluate(command).allowedByPolicy, false, command);
  }
});

test('policy cannot absorb chains, pipelines, redirects, or unsupported outer syntax', (t) => {
  const f = fixture(t);
  writePolicy(f.root, ['fixture-tool send *', 'fixture-tool receive *']);
  for (const command of ['fixture-tool send x && git status', 'fixture-tool send x | fixture-tool receive', 'fixture-tool send x; fixture-tool receive y']) {
    assert.equal(f.evaluate(command).allowedByPolicy, true, command);
  }
  for (const command of ['fixture-tool send x && rm payload', 'fixture-tool send x | other-tool', 'fixture-tool send x > output', 'fixture-tool send $(other-tool)', 'fixture-tool send `other-tool`', 'fixture-tool send x &', '(fixture-tool send x)', 'if true; then fixture-tool send x; fi', 'fixture-tool send x &&', 'fixture-tool send x |', 'fixture-tool send x && || git status']) {
    assert.equal(f.evaluate(command).allowedByPolicy, false, command);
  }
});

test('fixed arguments never match shell expansions whose runtime value is unknown', (t) => {
  const f = fixture(t);
  writePolicy(f.root, ['fixture-tool send literal']);
  for (const command of ['fixture-tool send $BODY', 'fixture-tool send ~', 'fixture-tool send {a,b}', "fixture-tool send $'literal'", 'fixture-tool send $"literal"']) {
    assert.equal(f.evaluate(command).allowedByPolicy, false, command);
  }
});

test('nested shell payload receives no implicit grant but its exact invocation can be allowed', (t) => {
  const f = fixture(t);
  writePolicy(f.root, ['fixture-tool send *']);
  assert.equal(f.evaluate("sh -c 'fixture-tool send body'").allowedByPolicy, false);
  writePolicy(f.root, ["sh -c 'fixture-tool send body'"]);
  assert.equal(f.evaluate('sh -c "fixture-tool send body"').allowedByPolicy, true);
  assert.equal(f.evaluate("sh -c 'fixture-tool send other'").allowedByPolicy, false);
});

test('directory changes never carry permission into an unrelated scope', (t) => {
  const f = fixture(t);
  const child = path.join(f.root, 'child');
  const outside = path.join(f.home, 'outside');
  fs.mkdirSync(child);
  fs.mkdirSync(outside);
  writePolicy(f.root, ['fixture-tool send *']);
  assert.equal(f.evaluate('cd child && fixture-tool send body').allowedByPolicy, true);
  assert.equal(f.evaluate('cd . && fixture-tool send body').allowedByPolicy, true);
  assert.equal(f.evaluate(`cd '${child}' && fixture-tool send body`).allowedByPolicy, true);
  assert.equal(f.evaluate(`cd '${outside}' && fixture-tool send body`).allowedByPolicy, false);
  assert.equal(f.evaluate('cd "$DESTINATION" && fixture-tool send body').allowedByPolicy, false);
});

async function hookFixture(t, host, mode = 'L5', active = true) {
  const f = fixture(t);
  const plan = { id: 'policy-plan', status: 'active' };
  const server = http.createServer((request, response) => {
    const pathname = new URL(request.url, 'http://localhost').pathname;
    let data;
    if (pathname === '/health') data = { ok: true };
    else if (pathname === '/projects/by-cwd') data = { project: f.project };
    else if (pathname.endsWith('/plans/active')) data = { plan };
    else if (pathname.endsWith('/tasks/in-flight')) data = { tasks: active ? [{ id: 'policy-task', status: 'in_progress' }] : [] };
    else if (pathname.endsWith('/chores')) data = { tasks: [] };
    else if (pathname === '/autonomy_policies/resolve') data = { policy: { mode } };
    else if (pathname === '/patterns/active') data = { patterns: [] };
    else if (pathname === '/scenarios/active-claims') data = { scenarios: [] };
    response.writeHead(data === undefined ? 404 : 200, { 'content-type': 'application/json' });
    response.end(JSON.stringify(data || {}));
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise((resolve) => server.close(resolve)));
  const cache = path.join(f.home, '.cache/sdi');
  fs.mkdirSync(cache, { recursive: true });
  fs.writeFileSync(path.join(cache, 'sdid.port'), String(server.address().port));
  const env = { ...process.env, SDI_HOME: f.home, CLAUDE_PLUGIN_ROOT: PLUGIN, PLUGIN_ROOT: host === 'codex' ? PLUGIN : '', CODEX_HOME: path.join(f.home, 'codex'), SDI_CODEX_AGENTS_DISABLE: '1', SDI_BYPASS_HOOKS: '', SDI_DELEGATION_BYPASS: '', SDI_HOOK_V05_DISABLE: '', SDI_ACTIVE_TASK: '', SDI_AGENT_TYPE: 'impl-coder', ORCA_WORKER_ID: 'external-worker' };
  f.run = (tool_name, tool_input, identity = {}) => new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [SHIM], { env });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (part) => { stdout += part; });
    child.stderr.on('data', (part) => { stderr += part; });
    child.on('error', reject);
    child.on('close', (status) => resolve({ status, stdout, stderr }));
    child.stdin.end(JSON.stringify({ cwd: f.root, session_id: 'policy-session', tool_name, tool_input, ...identity }));
  });
  f.log = () => fs.readFileSync(path.join(f.home, '.local/state/sdi/hook.log'), 'utf8');
  return f;
}

for (const host of ['claude', 'codex']) {
  test(`${host}: policy grants shell delegation only, preserves main edits and native identity behavior`, async (t) => {
    const f = await hookFixture(t, host);
    writePolicy(f.root, ['orca orchestration send *']);
    const shellTool = 'Bash';
    const allowed = await f.run(shellTool, { command: 'orca orchestration send --body runtime-secret-value' });
    assert.equal(allowed.status, 0, allowed.stderr);
    assert.doesNotMatch(allowed.stdout, /permissionDecision.*(?:deny|ask)/);
    assert.equal(classifyBash('orca orchestration send --body runtime-secret-value').readOnly, false);
    const denied = await f.run(shellTool, { command: 'orca orchestration worker_done' }, { agent_type: 'impl-coder', task_id: 'fabricated-task' });
    assert.match(denied.stdout, /delegation gate/);
    const edit = await f.run(host === 'codex' ? 'apply_patch' : 'Edit', { file_path: path.join(f.root, 'example.js'), patch: '*** Begin Patch\n*** End Patch\n' });
    assert.match(edit.stdout, /delegation gate/);
    const native = await f.run('Edit', { file_path: path.join(f.root, 'example.js') }, { agent_id: 'native-specialist', agent_type: 'impl-coder' });
    assert.doesNotMatch(native.stdout, /permissionDecision.*(?:deny|ask)/);
    const log = f.log();
    assert.match(log, /orca orchestration send \*/);
    assert.match(log, /settings\.json/);
    assert.doesNotMatch(log, /runtime-secret-value/);
  });

  test(`${host}: exception preserves autonomy confirmation for a mutating command`, async (t) => {
    const f = await hookFixture(t, host, 'L3');
    writePolicy(f.root, ['orca orchestration send *']);
    const result = await f.run('Bash', { command: 'orca orchestration send --body hello' });
    assert.match(result.stdout, /autonomy=L3/);
    const output = JSON.parse(result.stdout).hookSpecificOutput;
    assert.equal(output.permissionDecision, host === 'codex' ? 'deny' : 'ask');
  });

  test(`${host}: missing policies preserve main shell denial and task input cannot authorize edits`, async (t) => {
    const f = await hookFixture(t, host, 'L5', false);
    const result = await f.run('Bash', { command: 'orca skills get orchestration' });
    assert.match(result.stdout, /delegation gate/);
    const edit = await f.run('Edit', { file_path: path.join(f.root, 'example.js') }, { agent_id: 'native-specialist', agent_type: 'impl-coder', task_id: 'fabricated-task' });
    assert.match(edit.stdout, /no active task/);
  });
}
