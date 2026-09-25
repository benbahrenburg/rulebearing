#!/usr/bin/env node
// Stages the npm packages of a release from the release archives; the logic is src/stage.ts.
// Usage: node scripts/stage.mjs <dist-dir> <version> <out-dir> [--partial]
// Needs `npm run build` first. Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 19.
import { stageCli } from '../dist/stage.js';

process.exitCode = stageCli(process.argv.slice(2));
