// The docs site's unit tests and the coverage floor: 70% of lines (docs/adr/0018-test-coverage-threshold.md).
// The browser test of the playground is vitest.e2e.config.ts. Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3.
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['test/**/*.test.ts'],
    exclude: ['test/e2e/**'],
    testTimeout: 30_000,
    coverage: {
      provider: 'v8',
      include: ['src/**/*.ts', 'web/**/*.ts'],
      reporter: ['text', 'lcov'],
      thresholds: { lines: 70 },
    },
  },
});
