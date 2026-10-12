// rb-node's tests: the programmatic API of the `rulebearing` package (wrappers/npm/src/api) over
// the addon built from this crate, held to the 70% line floor (docs/adr/0018-test-coverage-threshold.md).
// Plan: docs/plans/implemented/0003-wave-3-operations-surface-inner-loop.md, Step 22.
// Forked workers, not threads: the tests change the working directory, as a script calling
// dependency-cruiser's `cruise()` from its repository does. coverage.sh runs these files in one
// forked worker instead (--maxWorkers=1 --no-isolate), so each test file that changes the working
// directory restores it in afterAll.
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['__tests__/**/*.test.ts'],
    globalSetup: ['__tests__/global-setup.ts'],
    pool: 'forks',
    testTimeout: 60_000,
    hookTimeout: 600_000,
    coverage: {
      provider: 'v8',
      allowExternal: true,
      include: ['**/wrappers/npm/src/api/**/*.ts'],
      reporter: ['text', 'lcov'],
      thresholds: { lines: 70 },
    },
  },
});
