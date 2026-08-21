#!/usr/bin/env node
'use strict';

// Host-neutral MCP launcher. Claude Code and Codex both call this script from
// the installed plugin root; it resolves `sdi` the same way the install gate
// does, so local source checkouts and release bundles share one path policy.

const { spawn } = require('child_process');
const { pluginRoot, _internals } = require('../adapters/shared/sdi-hooks.cjs');

const root = pluginRoot();
const sdi = _internals.resolveSdiBin(root);
if (!sdi) {
  process.stderr.write(
    '[sdi] MCP launcher: `sdi` binary not found. ' +
      'Set SDI_BIN=/path/to/sdi or build the workspace with `cargo build`.\n',
  );
  process.exit(127);
}

const sdid = _internals.resolveSdidBin(root, sdi);
const env = { ...process.env };
if (sdid) env.SDI_DAEMON_BIN = sdid;

const child = spawn(sdi.bin, ['mcp'], {
  stdio: 'inherit',
  env,
});

child.on('error', (err) => {
  process.stderr.write(`[sdi] MCP launcher failed: ${err && err.message ? err.message : err}\n`);
  process.exit(127);
});

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code == null ? 1 : code);
});
