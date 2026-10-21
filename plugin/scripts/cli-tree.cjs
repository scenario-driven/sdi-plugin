#!/usr/bin/env node
// Derives the `sdi` command tree from the clap derive definitions in
// crates/cli/src/cli.rs and writes it to adapters/shared/sdi-cli-tree.json.
//
// The JSON ships with the plugin: the SessionStart hook injects it as the
// session's CLI reference (agents take signatures from there instead of
// re-running `--help`), and plugin/tests/cli-refs.test.cjs checks that every
// `sdi …` command quoted in skills, agents, commands and hooks exists in it.
//
//   node plugin/scripts/cli-tree.cjs          # regenerate the JSON
//   node plugin/scripts/cli-tree.cjs --check  # exit 1 when the JSON is stale
//
// The parser understands the subset of clap derive the CLI uses: enums with
// `#[derive(Subcommand)]`, unit / tuple / struct variants, `#[command(name =
// …, alias = …, aliases = […])]`, and `Args` structs that carry a nested
// `#[command(subcommand)]` field.

'use strict';

const fs = require('fs');
const path = require('path');

const PLUGIN_ROOT = path.resolve(__dirname, '..');
const CLI_RS = path.resolve(PLUGIN_ROOT, '..', 'crates', 'cli', 'src', 'cli.rs');
const TREE_JSON = path.join(PLUGIN_ROOT, 'adapters', 'shared', 'sdi-cli-tree.json');

function kebab(name) {
  return name.replace(/([a-z0-9])([A-Z])/g, '$1-$2').toLowerCase();
}

// Returns the text between the `{` at `open` and its matching `}`.
function braceBody(src, open) {
  let depth = 0;
  for (let i = open; i < src.length; i++) {
    if (src[i] === '{') depth += 1;
    else if (src[i] === '}') {
      depth -= 1;
      if (depth === 0) return src.slice(open + 1, i);
    }
  }
  throw new Error(`unbalanced braces at offset ${open}`);
}

function stripComments(src) {
  return src.replace(/\/\/[^\n]*/g, '');
}

function parseVariants(body) {
  const variants = [];
  let depth = 0;
  let attrs = [];
  for (const rawLine of body.split('\n')) {
    const line = rawLine.trim();
    if (!line) continue;
    if (depth === 0) {
      if (line.startsWith('#[')) {
        attrs.push(line);
      } else {
        const m = /^([A-Z][A-Za-z0-9]*)\s*(\(\s*([A-Za-z0-9_]+)\s*\))?/.exec(line);
        if (m) {
          const attr = attrs.join(' ');
          const nameM = /\bname\s*=\s*"([^"]+)"/.exec(attr);
          const aliases = [];
          for (const a of attr.matchAll(/\b(?:visible_)?alias\s*=\s*"([^"]+)"/g)) aliases.push(a[1]);
          for (const list of attr.matchAll(/\b(?:visible_)?aliases\s*=\s*\[([^\]]*)\]/g)) {
            for (const a of list[1].matchAll(/"([^"]+)"/g)) aliases.push(a[1]);
          }
          variants.push({ name: nameM ? nameM[1] : kebab(m[1]), aliases, payload: m[3] || null });
        }
        attrs = [];
      }
    }
    for (const ch of line) {
      if (ch === '{') depth += 1;
      else if (ch === '}') depth -= 1;
    }
  }
  return variants;
}

function parseCliRs(source) {
  const src = stripComments(source);
  const enums = new Map();
  for (const m of src.matchAll(/#\[derive\([^\]]*\bSubcommand\b[^\]]*\)\]\s*pub enum (\w+)\s*\{/g)) {
    enums.set(m[1], parseVariants(braceBody(src, m.index + m[0].length - 1)));
  }
  // Args structs whose nested subcommand enum hangs below the variant.
  const nested = new Map();
  for (const m of src.matchAll(/pub struct (\w+)\s*\{/g)) {
    const body = braceBody(src, m.index + m[0].length - 1);
    const sub = /#\[command\(subcommand\)\]\s*pub \w+\s*:\s*(?:Option<)?(\w+)/.exec(body);
    if (sub) nested.set(m[1], sub[1]);
  }
  const rootMatch = /#\[command\(subcommand\)\]\s*pub \w+\s*:\s*(\w+)/.exec(src.slice(src.indexOf('pub struct App')));
  if (!rootMatch || !enums.has(rootMatch[1])) throw new Error('root subcommand enum not found');

  const build = (enumName, seen) => {
    const node = {};
    for (const v of enums.get(enumName)) {
      let childEnum = null;
      if (v.payload && enums.has(v.payload)) childEnum = v.payload;
      else if (v.payload && nested.has(v.payload)) childEnum = nested.get(v.payload);
      const children = childEnum && !seen.has(childEnum) ? build(childEnum, new Set([...seen, childEnum])) : null;
      node[v.name] = { aliases: v.aliases, subcommands: children };
    }
    return node;
  };
  return build(rootMatch[1], new Set([rootMatch[1]]));
}

function renderTree(source) {
  return {
    source: 'crates/cli/src/cli.rs',
    commands: parseCliRs(source),
  };
}

// True when `sdi <words…>` names a real command path. Words after a leaf
// command are its arguments; clap adds `help` to every command group.
function commandExists(tree, words) {
  let level = tree.commands;
  for (const w of words) {
    if (!level) return true;
    const hit = Object.entries(level).find(([name, spec]) => name === w || spec.aliases.includes(w));
    if (!hit) return w === 'help';
    level = hit[1].subcommands;
  }
  return true;
}

// One line per command group, for the SessionStart context.
function summarizeTree(tree) {
  const lines = [];
  for (const [name, spec] of Object.entries(tree.commands)) {
    const subs = spec.subcommands ? Object.keys(spec.subcommands) : [];
    lines.push(subs.length ? `sdi ${name} {${subs.join('|')}}` : `sdi ${name}`);
  }
  return lines.join('\n');
}

function loadTree() {
  try {
    return JSON.parse(fs.readFileSync(TREE_JSON, 'utf8'));
  } catch {
    return null;
  }
}

module.exports = { parseCliRs, renderTree, commandExists, summarizeTree, loadTree, CLI_RS, TREE_JSON };

if (require.main === module) {
  const source = fs.readFileSync(CLI_RS, 'utf8');
  const json = JSON.stringify(renderTree(source), null, 2) + '\n';
  if (process.argv.includes('--check')) {
    const current = fs.existsSync(TREE_JSON) ? fs.readFileSync(TREE_JSON, 'utf8') : '';
    if (current !== json) {
      process.stderr.write(`${path.relative(process.cwd(), TREE_JSON)} is stale — run node plugin/scripts/cli-tree.cjs\n`);
      process.exit(1);
    }
  } else {
    fs.writeFileSync(TREE_JSON, json);
  }
}
