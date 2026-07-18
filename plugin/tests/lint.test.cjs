// Structural lint for the plugin shell.
//
// Asserts that the moving parts a Claude Code install needs are wired
// consistently to each other:
//   - hooks/hooks.json command paths resolve to existing .cjs files
//   - every .cjs in adapters/ + scripts/ parses with `node --check`
//   - every commands/*.md has a YAML frontmatter block with a description
//   - every skill in Claude plugin.json#skillsList exists on disk AND in
//     adapters/shared/sdi-hooks.cjs `SDI_SKILLS`
//   - the Codex plugin manifest points at the shared skills/ tree and inline
//     MCP config
//   - .mcp.json invokes the shared MCP launcher (the install gate resolver is
//     the single trust boundary for finding `sdi`)
//
// Run: `node --test plugin/tests/lint.test.cjs`

'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const PLUGIN_ROOT = path.resolve(__dirname, '..');
const REPO_ROOT = path.resolve(PLUGIN_ROOT, '..');
const HOOKS_JSON = path.join(PLUGIN_ROOT, 'hooks/hooks.json');
const CLAUDE_MANIFEST = path.join(PLUGIN_ROOT, '.claude-plugin/plugin.json');
const CODEX_MANIFEST = path.join(PLUGIN_ROOT, '.codex-plugin/plugin.json');
const MARKETPLACE_JSON = path.join(REPO_ROOT, '.agents/plugins/marketplace.json');
const MCP_JSON = path.join(PLUGIN_ROOT, '.mcp.json');
const MCP_LAUNCHER = path.join(PLUGIN_ROOT, 'scripts/sdi-mcp.cjs');
const SHARED = path.join(PLUGIN_ROOT, 'adapters/shared/sdi-hooks.cjs');
const COMMANDS_DIR = path.join(PLUGIN_ROOT, 'commands');
const SKILLS_DIR = path.join(PLUGIN_ROOT, 'skills');

function listFiles(dir, ext) {
  if (!fs.existsSync(dir)) return [];
  return fs.readdirSync(dir).filter((f) => f.endsWith(ext)).map((f) => path.join(dir, f));
}

function readJson(p) {
  return JSON.parse(fs.readFileSync(p, 'utf8'));
}

// ────────────────────────────────────────────────────────────────────────────
// hooks.json wiring

test('lint: hooks.json references resolve to existing .cjs shims', () => {
  const manifest = readJson(HOOKS_JSON);
  assert.ok(manifest.hooks && typeof manifest.hooks === 'object');
  const events = Object.keys(manifest.hooks);
  assert.ok(events.length >= 6, `expected ≥6 wired events, got ${events.length}`);
  for (const evt of events) {
    const groups = manifest.hooks[evt];
    assert.ok(Array.isArray(groups) && groups.length > 0, `event ${evt} has no groups`);
    for (const grp of groups) {
      for (const h of grp.hooks || []) {
        assert.equal(h.type, 'command');
        const cmd = h.command || '';
        assert.match(cmd, /^\$\{CLAUDE_PLUGIN_ROOT\}/, `command must start with \${CLAUDE_PLUGIN_ROOT}: ${cmd}`);
        const rel = cmd.replace('${CLAUDE_PLUGIN_ROOT}/', '');
        const abs = path.join(PLUGIN_ROOT, rel);
        assert.ok(fs.existsSync(abs), `hook target missing: ${abs}`);
        assert.match(abs, /\.cjs$/);
      }
    }
  }
});

test('lint: hooks.json wires all six expected events', () => {
  const manifest = readJson(HOOKS_JSON);
  const expected = [
    'SessionStart',
    'UserPromptSubmit',
    'PreToolUse',
    'PostToolUse',
    'SubagentStart',
    'SubagentStop',
  ];
  for (const evt of expected) {
    assert.ok(manifest.hooks[evt], `missing event: ${evt}`);
  }
});

test('lint: SessionStart matcher covers Codex start sources', () => {
  const manifest = readJson(HOOKS_JSON);
  const matcher = manifest.hooks.SessionStart?.[0]?.matcher || '';
  for (const source of ['startup', 'resume', 'clear', 'compact']) {
    assert.match(source, new RegExp(matcher), `SessionStart must match ${source}`);
  }
});

// ────────────────────────────────────────────────────────────────────────────
// node --check on every .cjs

test('lint: every adapters/**/*.cjs parses with `node --check`', () => {
  const files = [
    ...listFiles(path.join(PLUGIN_ROOT, 'adapters/claude'), '.cjs'),
    ...listFiles(path.join(PLUGIN_ROOT, 'adapters/shared'), '.cjs'),
    ...listFiles(path.join(PLUGIN_ROOT, 'scripts'), '.cjs'),
  ];
  assert.ok(files.length > 0, 'expected at least one .cjs file');
  for (const file of files) {
    const r = spawnSync('node', ['--check', file], { encoding: 'utf8' });
    assert.equal(r.status, 0, `${file} failed --check:\n${r.stderr}`);
  }
});

test('lint: every adapters/claude/*.cjs is a thin shim (≤ 8 lines)', () => {
  const shims = listFiles(path.join(PLUGIN_ROOT, 'adapters/claude'), '.cjs');
  assert.equal(shims.length, 6, `expected 6 shims, got ${shims.length}`);
  for (const file of shims) {
    const lines = fs.readFileSync(file, 'utf8').split('\n').filter((l) => l.trim().length > 0);
    assert.ok(
      lines.length <= 8,
      `${path.basename(file)} has ${lines.length} non-blank lines; shim must stay thin`,
    );
    // Must wrap the shared call with .catch() or try/catch.
    const body = fs.readFileSync(file, 'utf8');
    assert.ok(
      /\.catch\s*\(/.test(body) || /try\s*\{[\s\S]*\}\s*catch/.test(body),
      `${path.basename(file)} must wrap with .catch() or try/catch — hook crash safety`,
    );
    // Must exit 0 on failure, never propagate non-zero.
    assert.match(body, /process\.exit\(0\)/, `${path.basename(file)} must call process.exit(0)`);
    // Must NOT contain business logic — only the require + dispatch.
    assert.doesNotMatch(
      body,
      /\b(spawn|http\.request|fs\.write|setInterval|new Server|new Daemon)\b/,
      `${path.basename(file)} contains business logic; move it into adapters/shared/sdi-hooks.cjs`,
    );
  }
});

// ────────────────────────────────────────────────────────────────────────────
// commands/*.md frontmatter

test('lint: every commands/*.md has a YAML frontmatter block with description', () => {
  const cmds = listFiles(COMMANDS_DIR, '.md');
  assert.equal(cmds.length, 10, `expected 10 command files, got ${cmds.length}`);
  for (const file of cmds) {
    const raw = fs.readFileSync(file, 'utf8');
    assert.match(raw, /^---\n[\s\S]+?\n---\n/, `${path.basename(file)} missing frontmatter block`);
    const fm = raw.slice(4, raw.indexOf('\n---\n', 4));
    assert.match(fm, /description:\s*\S/, `${path.basename(file)} frontmatter must include description`);
  }
});

test('lint: command file names match the slash-command vocabulary', () => {
  const cmds = listFiles(COMMANDS_DIR, '.md').map((f) => path.basename(f, '.md'));
  const expected = [
    'scenario', 'round', 'plan', 'req', 'decide', 'sdi-status',
    'autonomy', 'agent-note', 'consensus', 'pattern',
  ];
  for (const name of expected) assert.ok(cmds.includes(name), `missing command: /${name}`);
});

// ────────────────────────────────────────────────────────────────────────────
// SKILL.md frontmatter + three-way sync

test('lint: every skills/<name>/SKILL.md has frontmatter with name and description', () => {
  if (!fs.existsSync(SKILLS_DIR)) return;
  const dirs = fs.readdirSync(SKILLS_DIR).filter((d) =>
    fs.statSync(path.join(SKILLS_DIR, d)).isDirectory(),
  );
  for (const d of dirs) {
    const skill = path.join(SKILLS_DIR, d, 'SKILL.md');
    assert.ok(fs.existsSync(skill), `${skill} missing`);
    const raw = fs.readFileSync(skill, 'utf8');
    assert.match(raw, /^---\n[\s\S]+?\n---\n/, `${d}/SKILL.md missing frontmatter`);
    const fm = raw.slice(4, raw.indexOf('\n---\n', 4));
    assert.match(fm, /name:\s*\S/, `${d}/SKILL.md frontmatter must include name`);
    assert.match(fm, /description:\s*\S/, `${d}/SKILL.md frontmatter must include description`);
  }
});

test('lint: SDI_SKILLS array, plugin.json#skillsList, and skills/ dirs are in lock-step', () => {
  delete require.cache[require.resolve(SHARED)];
  const { _internals } = require(SHARED);
  const sdiSkillsArr = _internals.SDI_SKILLS;

  const manifest = readJson(CLAUDE_MANIFEST);
  const manifestList = (manifest.skillsList || []).map((s) => s.name);

  const onDisk = fs.existsSync(SKILLS_DIR)
    ? fs.readdirSync(SKILLS_DIR).filter((d) =>
        fs.statSync(path.join(SKILLS_DIR, d)).isDirectory(),
      )
    : [];

  // All three sets must be equal.
  const sortedA = [...sdiSkillsArr].sort();
  const sortedB = [...manifestList].sort();
  const sortedC = [...onDisk].sort();
  assert.deepEqual(sortedA, sortedB, `SDI_SKILLS vs plugin.json#skillsList mismatch`);
  assert.deepEqual(sortedB, sortedC, `plugin.json#skillsList vs on-disk skills/ mismatch`);
});

test('lint: Codex plugin manifest points at shared skills and inline MCP config', () => {
  const claudeManifest = readJson(CLAUDE_MANIFEST);
  const manifest = readJson(CODEX_MANIFEST);
  assert.equal(manifest.name, 'sdi');
  assert.equal(manifest.version, claudeManifest.version);
  assert.equal(manifest.license, 'MIT');
  assert.equal(manifest.skills, './skills/');
  assert.ok(manifest.interface && typeof manifest.interface === 'object');
  assert.equal(manifest.interface.displayName, 'SDI');
  assert.ok(Array.isArray(manifest.interface.capabilities));

  assert.ok(manifest.mcpServers && manifest.mcpServers.sdi, 'Codex manifest must register `sdi` MCP server');
  const server = manifest.mcpServers.sdi;
  assert.equal(server.command, 'node');
  assert.deepEqual(server.args, ['./scripts/sdi-mcp.cjs']);
  assert.equal(server.cwd, '.');
});

test('lint: repo marketplace exposes the existing plugin/ root to Codex', () => {
  const marketplace = readJson(MARKETPLACE_JSON);
  assert.equal(marketplace.name, 'scenario-driven-sdi-plugin');
  const entry = (marketplace.plugins || []).find((p) => p && p.name === 'sdi');
  assert.ok(entry, 'marketplace must expose sdi');
  assert.deepEqual(entry.source, { source: 'local', path: './plugin' });
  assert.deepEqual(entry.policy, { installation: 'AVAILABLE', authentication: 'ON_INSTALL' });
  assert.equal(entry.category, 'Productivity');
});

// ────────────────────────────────────────────────────────────────────────────
// MCP wiring

test('lint: .mcp.json invokes the shared MCP launcher', () => {
  const cfg = readJson(MCP_JSON);
  assert.ok(cfg.mcpServers && cfg.mcpServers.sdi, '.mcp.json must register `sdi` server');
  const cmd = cfg.mcpServers.sdi.command || '';
  // Claude Code's MCP launcher does NOT inject the plugin bin onto the server
  // subprocess PATH (unlike the hook adapters, which resolve `sdi` specially),
  // so a bare `command: "sdi"` fails with `Executable not found in $PATH`. The
  // command MUST be an absolute, version-pinned path via ${CLAUDE_PLUGIN_ROOT}
  // and route through the shared resolver so source checkouts and release
  // bundles both work.
  assert.match(
    cmd,
    /^\$\{CLAUDE_PLUGIN_ROOT\}\/scripts\/sdi-mcp\.cjs$/,
    `MCP command must be \${CLAUDE_PLUGIN_ROOT}/scripts/sdi-mcp.cjs, got: ${cmd}`,
  );
  assert.equal(cfg.mcpServers.sdi.args, undefined);
});

test('lint: shared MCP launcher is executable', () => {
  const st = fs.statSync(MCP_LAUNCHER);
  assert.notEqual(st.mode & 0o111, 0, 'scripts/sdi-mcp.cjs must be directly executable');
});

// ────────────────────────────────────────────────────────────────────────────
// plugin.json sanity

test('lint: plugin.json has name, version, and license', () => {
  const manifest = readJson(CLAUDE_MANIFEST);
  assert.equal(manifest.name, 'sdi');
  assert.match(manifest.version, /^\d+\.\d+\.\d+/);
  assert.equal(manifest.license, 'MIT');
});
