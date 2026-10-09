// Tests and the coverage floor of eslint-plugin-rulebearing: 70% of lines (docs/adr/0018-test-coverage-threshold.md).
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 13.
// `rulebearing/launcher` is the npm wrapper's launcher source in this repository, as the published package depends on it.
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  resolve: {
    alias: {
      'rulebearing/launcher': fileURLToPath(
        new URL('../../wrappers/npm/src/launcher.ts', import.meta.url),
      ),
    },
  },
  test: {
    include: ['test/**/*.test.ts'],
    globalSetup: ['test/global-setup.ts'],
    testTimeout: 60_000,
    hookTimeout: 600_000,
    coverage: {
      provider: 'v8',
      include: ['src/**/*.ts'],
      reporter: ['text', 'lcov'],
      thresholds: { lines: 70 },
    },
  },
});
