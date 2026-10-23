#!/usr/bin/env node
try {
  require('../shared/sdi-hooks.cjs').runPreToolUse();
} catch (err) {
  process.stderr.write(`[sdi] PreToolUse failed: ${err && err.message ? err.message : err}\n`);
  process.exit(0);
}
