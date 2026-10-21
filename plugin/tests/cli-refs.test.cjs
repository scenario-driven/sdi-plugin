// Every `sdi …` command quoted in the plugin's skills, agents, slash commands
// and hook messages must exist in the CLI. The command tree comes from the
// clap definitions (crates/cli/src/cli.rs) via plugin/scripts/cli-tree.cjs;
// the shipped JSON copy must match a fresh parse of that source.
//
// Run: `node --test plugin/tests/cli-refs.test.cjs`

'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const PLUGIN_ROOT = path.resolve(__dirname, '..');
const cliTree = require(path.join(PLUGIN_ROOT, 'scripts/cli-tree.cjs'));

function walk(dir, ext) {
  if (!fs.existsSync(dir)) return [];
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(p, ext));
    else if (entry.name.endsWith(ext)) out.push(p);
  }
  return out;
}

// Code spans and fenced blocks of a Markdown file — prose mentions of "sdi"
// are not commands.
function markdownCode(raw) {
  const chunks = [];
  const withoutFences = raw.replace(/```[^\n]*\n([\s\S]*?)```/g, (_, body) => {
    chunks.push(body);
    return '';
  });
  for (const m of withoutFences.matchAll(/`([^`\n]+)`/g)) chunks.push(m[1]);
  return chunks;
}

// Backtick-quoted spans in JS sources that start with the command: template-
// literal hints (\`sdi …\`) and comment references (`sdi …`). Whole template
// literals are not spans — their text is prose around interpolations.
function sourceCode(raw) {
  return [...raw.matchAll(/\\?`((?:[^`\s]*\/)?sdi\s[^`\n]*?)\\?`/g)].map((m) => m[1]);
}

const SDI_INVOCATION = /(?:^|[\s/;&|(])sdi\s+([a-z][a-z0-9-]*)(?:\s+([a-z][a-z0-9-]*))?/g;

function invocations(chunks) {
  const out = [];
  for (const chunk of chunks) {
    for (const m of chunk.matchAll(SDI_INVOCATION)) out.push([m[1], m[2]].filter(Boolean));
  }
  return out;
}

test('cli tree: shipped sdi-cli-tree.json matches crates/cli/src/cli.rs', { skip: !fs.existsSync(cliTree.CLI_RS) && 'CLI source not present' }, () => {
  const fresh = cliTree.renderTree(fs.readFileSync(cliTree.CLI_RS, 'utf8'));
  const shipped = cliTree.loadTree();
  assert.ok(shipped, 'adapters/shared/sdi-cli-tree.json is missing — run node plugin/scripts/cli-tree.cjs');
  assert.deepEqual(shipped, fresh, 'sdi-cli-tree.json is stale — run node plugin/scripts/cli-tree.cjs');
});

test('cli tree: parser resolves nested and aliased commands', () => {
  const tree = cliTree.loadTree();
  assert.ok(cliTree.commandExists(tree, ['task', 'complete']));
  assert.ok(cliTree.commandExists(tree, ['task', 'brief']));
  assert.ok(cliTree.commandExists(tree, ['requirement', 'list']), 'alias of `req`');
  assert.ok(cliTree.commandExists(tree, ['chore', 'done']), 'subcommand nested in an Args struct');
  assert.ok(cliTree.commandExists(tree, ['doctor', 'anything']), 'words after a leaf are arguments');
  assert.ok(cliTree.commandExists(tree, ['plan', 'help']), 'clap adds help to every group');
  assert.equal(cliTree.commandExists(tree, ['task', 'update']), false);
  assert.equal(cliTree.commandExists(tree, ['task', 'done']), false);
  assert.equal(cliTree.commandExists(tree, ['disruption']), false);
});

test('cli refs: every `sdi <sub> [<sub>]` quoted in skills, agents, commands and hooks exists', () => {
  const tree = cliTree.loadTree();
  const sources = [
    ...walk(path.join(PLUGIN_ROOT, 'skills'), '.md').map((f) => [f, markdownCode]),
    ...walk(path.join(PLUGIN_ROOT, 'agents'), '.md').map((f) => [f, markdownCode]),
    ...walk(path.join(PLUGIN_ROOT, 'commands'), '.md').map((f) => [f, markdownCode]),
    ...walk(path.join(PLUGIN_ROOT, 'adapters'), '.cjs').map((f) => [f, sourceCode]),
  ];
  const stale = [];
  let checked = 0;
  for (const [file, extract] of sources) {
    for (const words of invocations(extract(fs.readFileSync(file, 'utf8')))) {
      checked += 1;
      if (!cliTree.commandExists(tree, words)) stale.push(`${path.relative(PLUGIN_ROOT, file)}: sdi ${words.join(' ')}`);
    }
  }
  assert.ok(checked > 50, `expected to check many references, found ${checked}`);
  assert.deepEqual(stale, []);
});
