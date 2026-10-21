#!/usr/bin/env node
require('../shared/sdi-hooks.cjs').runUserPromptSubmit().catch((err) => {
  process.stderr.write(`[sdi] UserPromptSubmit failed: ${err && err.message ? err.message : err}\n`);
  process.exit(0);
});
