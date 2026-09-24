// Tests and the coverage floor of rulebearing/vitest: 70% of lines (docs/adr/0018-test-coverage-threshold.md).
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14 (2H).
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['test/**/*.test.ts'],
    // The end-to-end test starts vitest and the binary in child processes.
    testTimeout: 60_000,
    coverage: {
      provider: 'v8',
      include: ['src/**/*.ts'],
      reporter: ['text', 'lcov'],
      thresholds: { lines: 70 },
    },
  },
});
