// eslint-plugin-rulebearing: one rule, `rulebearing/boundaries`, over the gate's `can-import`.
//
// Source: docs/artifacts/design.md#two-front-ends-that-will-matter-more-than-the-mcp-server.
// Decision: docs/adr/0021-agent-surface-cli-first.md (decision 2 lists the plugin in wave 2).
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 13. Requirement: FR-DIST-04.
//
// Usage, flat config:
//   import rulebearing from 'eslint-plugin-rulebearing';
//   export default [{ plugins: { rulebearing }, rules: { 'rulebearing/boundaries': 'error' } }];

import { readFileSync } from 'node:fs';
import type { ESLint } from 'eslint';
import { boundaries } from './boundaries.js';

/** This package's version, which ESLint's cache keys on. */
export function ownVersion(): string {
  const manifest: unknown = JSON.parse(
    readFileSync(new URL('../package.json', import.meta.url), 'utf8'),
  );
  if (typeof manifest === 'object' && manifest !== null && 'version' in manifest) {
    const { version } = manifest;
    if (typeof version === 'string') {
      return version;
    }
  }
  return '0.0.0';
}

const plugin: ESLint.Plugin = {
  meta: { name: 'eslint-plugin-rulebearing', version: ownVersion() },
  rules: { boundaries },
};

export default plugin;
