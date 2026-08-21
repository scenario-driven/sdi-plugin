#!/usr/bin/env node
require('../shared/sdi-hooks.cjs').runSessionStart().catch((err) => {
  process.stderr.write(`[sdi] SessionStart failed: ${err && err.message ? err.message : err}\n`);
  process.exit(0);
});
