// The playground's browser test: the built site (site/build.sh) served locally and the playground
// page driven in headless Chromium by Playwright. Plan: docs/plans/pending/0004-wave-4-reach.md,
// Step 3 and § 1.8 ("Playwright test in the docs build").
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['test/e2e/**/*.test.ts'],
    testTimeout: 120_000,
    hookTimeout: 120_000,
    // The test prints the measured time; keep it in the log.
    silent: false,
  },
});
