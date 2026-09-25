// Tests and the coverage floor of the npm wrapper: 70% of lines (docs/adr/0018-test-coverage-threshold.md).
// Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 19.
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['test/**/*.test.ts'],
    coverage: {
      provider: 'v8',
      include: ['src/**/*.ts'],
      reporter: ['text', 'lcov'],
      thresholds: { lines: 70 },
    },
  },
});
