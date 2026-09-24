// End to end: a real vitest run over a test file that calls defineArchitectureTests, with
// RulebearingReporter beside vitest's JSON reporter. Each rule is a vitest test whose failure
// message is the junit message; the reporter prints its block on standard error.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14 (2H).
import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';
import { REPOSITORY, copyFixture, junitMessages, localBinary } from './helpers.js';

let cleanup: (() => void) | undefined;
afterEach(() => {
  cleanup?.();
  cleanup = undefined;
});

interface JsonReport {
  readonly testResults: readonly {
    readonly assertionResults: readonly {
      readonly title: string;
      readonly status: string;
      readonly failureMessages: readonly string[];
      readonly meta: { readonly rulebearing?: { readonly family: string } };
    }[];
  }[];
}

describe('a vitest run with rulebearing/vitest', () => {
  it('reports one test per rule, failing with the junit message', () => {
    const binary = localBinary();
    const fixture = copyFixture();
    cleanup = fixture.remove;
    const source = (file: string): string =>
      pathToFileURL(join(REPOSITORY, 'adapters', 'vitest', 'src', file)).href;
    writeFileSync(
      join(fixture.dir, 'architecture.test.mjs'),
      `import { defineArchitectureTests } from ${JSON.stringify(source('index.ts'))};\n` +
        `defineArchitectureTests({ binary: ${JSON.stringify(binary)} });\n`,
    );
    writeFileSync(
      join(fixture.dir, 'vitest.config.mjs'),
      `import { RulebearingReporter } from ${JSON.stringify(source('index.ts'))};\n` +
        `export default { test: { include: ['architecture.test.mjs'], reporters: [new RulebearingReporter(), ['json', { outputFile: 'report.json' }]] } };\n`,
    );
    const run = spawnSync(
      process.execPath,
      [join(REPOSITORY, 'node_modules', 'vitest', 'vitest.mjs'), 'run', '--root', fixture.dir],
      { cwd: fixture.dir, encoding: 'utf8', env: { ...process.env, CI: '1' } },
    );
    expect(run.status, run.stderr).toBe(1);

    const report = JSON.parse(readFileSync(join(fixture.dir, 'report.json'), 'utf8')) as JsonReport;
    const results = report.testResults.flatMap((file) => file.assertionResults);
    const expected = junitMessages(binary, fixture.dir);
    expect(results.map((r) => r.title)).toEqual([...expected.keys()]);
    for (const result of results) {
      const text = expected.get(result.title) ?? '';
      expect(result.status, result.title).toBe(text === '' ? 'passed' : 'failed');
      if (text !== '') {
        expect(result.failureMessages[0], result.title).toBe(`ArchitectureRuleError: ${text}`);
      }
    }

    expect(run.stderr).toContain('rulebearing: 5 rules, 3 failed');
    expect(run.stderr).toContain(
      '  FAIL forbidden nothing-matches\n    rule `nothing-matches` is vacuous',
    );
    expect(run.stderr).toMatch(
      / {2}WARN forbidden api-not-to-util\n {4}warn: RB-[0-9a-f]{8} py\/api\/view\.py/,
    );
  });
});
