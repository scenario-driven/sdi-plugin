#!/usr/bin/env node
// Replays the Bash commands the D21 delegation gate denied (hook audit log)
// through the previous and the current read-only classifier, and summarises
// how the verdicts change. The log is only read.
//
//   node plugin/scripts/replay-d21-denials.cjs [--log <hook.log>] [--old <sdi-hooks.cjs>] [--json]
//
// --log  defaults to $SDI_HOME/.local/state/sdi/hook.log (SDI_HOME → $HOME).
// --old  a previous sdi-hooks.cjs whose `_internals.isReadOnlyBash` is the
//        baseline; defaults to the committed HEAD version (`git show`).
//
// The audit log stores at most 200 characters per command, so a truncated
// entry can end inside a quote; those are counted separately because the
// new classifier rejects them for the truncation, not the command.

'use strict';

const fs = require('fs');
const os = require('os');
const path = require('path');
const readline = require('readline');
const { execFileSync } = require('child_process');
const { classifyBash } = require('../adapters/shared/bash-classifier.cjs');

const LOG_CMD_LIMIT = 200;

function arg(name) {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : null;
}

function loadOldClassifier(explicit) {
  if (explicit) return { fn: require(path.resolve(explicit))._internals.isReadOnlyBash, cleanup: () => {} };
  const repo = path.resolve(__dirname, '..', '..');
  const src = execFileSync('git', ['-C', repo, 'show', 'HEAD:plugin/adapters/shared/sdi-hooks.cjs'], { encoding: 'utf8' });
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'sdi-replay-'));
  const file = path.join(dir, 'sdi-hooks.cjs');
  fs.writeFileSync(file, src);
  const cleanup = () => fs.rmSync(dir, { recursive: true, force: true });
  try {
    return { fn: require(file)._internals.isReadOnlyBash, cleanup };
  } catch (e) {
    cleanup();
    throw e;
  }
}

function headWord(cmd) {
  const t = cmd.trim().split(/\s+/);
  const w = t[0] || '';
  return ['sdi', 'git', 'gh', 'cargo', 'clawket', 'cmux', 'sed', 'command', 'cd'].includes(path.basename(w)) ? `${path.basename(w)} ${t[1] || ''}`.trim() : path.basename(w);
}

function bump(map, key, example) {
  const e = map.get(key) || { count: 0, example };
  e.count += 1;
  map.set(key, e);
}

function top(map, n) {
  return [...map.entries()].sort((a, b) => b[1].count - a[1].count).slice(0, n).map(([key, v]) => ({ key, count: v.count, example: v.example }));
}

async function main() {
  const home = process.env.SDI_HOME || os.homedir();
  const logPath = arg('--log') || path.join(home, '.local', 'state', 'sdi', 'hook.log');
  const old = loadOldClassifier(arg('--old'));
  const stats = { denials: 0, unique: 0, truncated: 0, oldReadOnly: 0, nowReadOnly: 0, stillBlocked: 0 };
  const allowed = new Map();
  const blockedByReason = new Map();
  const blockedByHead = new Map();
  const seen = new Set();
  try {
    const rl = readline.createInterface({ input: fs.createReadStream(logPath) });
    for await (const line of rl) {
      if (!line.includes('"delegation-gate"')) continue;
      let e;
      try {
        e = JSON.parse(line);
      } catch {
        continue;
      }
      if (e.event !== 'pre_tool_use_blocked' || e.reason !== 'delegation-gate' || typeof e.bash_cmd !== 'string') continue;
      stats.denials += 1;
      const cmd = e.bash_cmd;
      if (!seen.has(cmd)) {
        seen.add(cmd);
        stats.unique += 1;
      }
      if (old.fn(cmd)) stats.oldReadOnly += 1;
      const v = classifyBash(cmd);
      if (v.readOnly) {
        stats.nowReadOnly += 1;
        bump(allowed, headWord(cmd), cmd.slice(0, 120));
      } else if (cmd.length >= LOG_CMD_LIMIT) {
        stats.truncated += 1;
      } else {
        stats.stillBlocked += 1;
        bump(blockedByReason, v.reason.replace(/`[^`]*`/g, '`…`'), cmd.slice(0, 120));
        bump(blockedByHead, headWord(v.failingSegment || cmd), cmd.slice(0, 120));
      }
    }
  } finally {
    old.cleanup();
  }
  const report = {
    log: logPath,
    ...stats,
    newlyAllowedByCommand: top(allowed, 25),
    stillBlockedByCommand: top(blockedByHead, 25),
    stillBlockedByReason: top(blockedByReason, 15),
  };
  if (process.argv.includes('--json')) {
    process.stdout.write(JSON.stringify(report, null, 2) + '\n');
    return;
  }
  const pct = (n) => (stats.denials ? ((100 * n) / stats.denials).toFixed(1) : '0.0');
  const out = [];
  out.push(`D21 denials replayed: ${stats.denials} (${stats.unique} unique) from ${logPath}`);
  out.push(`  old classifier read-only (should be 0 — they were denied): ${stats.oldReadOnly}`);
  out.push(`  new classifier read-only: ${stats.nowReadOnly} (${pct(stats.nowReadOnly)}%)`);
  out.push(`  still blocked: ${stats.stillBlocked} (${pct(stats.stillBlocked)}%)`);
  out.push(`  truncated in the log (≥${LOG_CMD_LIMIT} chars, not judged): ${stats.truncated} (${pct(stats.truncated)}%)`);
  const table = (title, rows) => {
    out.push('', title);
    for (const r of rows) out.push(`  ${String(r.count).padStart(6)}  ${r.key}  | ${r.example.replace(/\s+/g, ' ')}`);
  };
  table('Newly allowed, by command:', report.newlyAllowedByCommand);
  table('Still blocked, by failing command:', report.stillBlockedByCommand);
  table('Still blocked, by reason:', report.stillBlockedByReason);
  process.stdout.write(out.join('\n') + '\n');
}

main().catch((e) => {
  process.stderr.write(`${e.stack || e}\n`);
  process.exit(1);
});
