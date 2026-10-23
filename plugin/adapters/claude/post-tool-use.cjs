#!/usr/bin/env node
require('../shared/sdi-hooks.cjs').runPostToolUse().catch((err) => {
  process.stderr.write(`[sdi] PostToolUse failed: ${err && err.message ? err.message : err}\n`);
  process.exit(0);
});
