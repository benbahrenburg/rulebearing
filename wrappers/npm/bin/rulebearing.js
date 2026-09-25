#!/usr/bin/env node
// The `rulebearing` command installed by npm. The launcher (src/launcher.ts, compiled to dist/)
// finds the platform binary and runs it with every argument unchanged.
// Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 19.
import { launch } from '../dist/launcher.js';

await launch();
