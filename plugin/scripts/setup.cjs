#!/usr/bin/env node
// Manual entry into the install gate. Mirrors Clawket's `scripts/setup.cjs`
// pattern: delegate to the same `ensureInstalled` used by SessionStart so there
// is exactly one install code path.
const path = require('path');
const { ensureInstalled } = require('../adapters/shared/sdi-hooks.cjs');

(async () => {
  const root = process.env.PLUGIN_ROOT || process.env.CLAUDE_PLUGIN_ROOT || path.resolve(__dirname, '..');
  const ok = await ensureInstalled(root).catch((err) => {
    process.stderr.write(`[sdi] setup failed: ${err && err.message ? err.message : err}\n`);
    return false;
  });
  if (!ok) {
    process.stderr.write(`[sdi] setup did not complete cleanly. See ~/.local/state/sdi/hook.log\n`);
    process.exit(1);
  }
  process.stdout.write(`[sdi] setup ok\n`);
})();
