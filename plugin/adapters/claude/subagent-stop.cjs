#!/usr/bin/env node
require('../shared/sdi-hooks.cjs').runSubagentStop().catch((err) => {
  process.stderr.write(`[sdi] SubagentStop failed: ${err && err.message ? err.message : err}\n`);
  process.exit(0);
});
