#!/usr/bin/env node
require('../shared/sdi-hooks.cjs').runSubagentStart().catch((err) => {
  process.stderr.write(`[sdi] SubagentStart failed: ${err && err.message ? err.message : err}\n`);
  process.exit(0);
});
