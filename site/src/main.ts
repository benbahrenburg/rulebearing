// The entry point of the docs site's command line (cli.ts holds the commands).
//
// Decision: docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3.

import { main } from './cli.js';

const code = await main(process.argv.slice(2), {
  out: (text) => process.stdout.write(text),
  err: (text) => process.stderr.write(text),
});
process.exitCode = code;
