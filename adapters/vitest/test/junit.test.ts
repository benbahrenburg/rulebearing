// The proof the plan asks for: every rule test's failure message is the junit message for that
// rule, on the shared fixture (adapters/fixture), with the locally built binary.
// Contract: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 1.5; Step 14 (2H);
// docs/adr/0007-vacuous-rules-fail-by-default.md for the vacuous rule.
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import type { TestOptions } from 'vitest';
import { ArchitectureRuleError, defineArchitectureTests } from '../src/define.js';
import { copyFixture, junitMessages, localBinary } from './helpers.js';

let cleanup: (() => void) | undefined;
afterEach(() => {
  cleanup?.();
  cleanup = undefined;
});

interface Registered {
  readonly name: string;
  readonly options: TestOptions;
  readonly body: () => void;
}

function register(options: Parameters<typeof defineArchitectureTests>[0]): {
  suites: string[];
  tests: Registered[];
} {
  const suites: string[] = [];
  const tests: Registered[] = [];
  defineArchitectureTests(options, {
    describe: (name, body) => {
      suites.push(name);
      body();
    },
    test: (name, testOptions, body) => {
      tests.push({ name, options: testOptions, body });
    },
  });
  return { suites, tests };
}

function outcome(body: () => void): string | undefined {
  try {
    body();
    return undefined;
  } catch (error) {
    expect(error).toBeInstanceOf(ArchitectureRuleError);
    return (error as Error).message;
  }
}

describe('rule tests against rulebearing cruise -T junit', () => {
  it('fail with exactly the junit message, and pass where junit has none', () => {
    const binary = localBinary();
    const fixture = copyFixture();
    cleanup = fixture.remove;
    const expected = junitMessages(binary, fixture.dir);
    expect([...expected.keys()]).toEqual([
      'handlers-not-to-util',
      'api-not-to-util',
      'nothing-matches',
      'util-is-a-leaf',
      'classes-are-pascal-case',
    ]);

    const { suites, tests } = register({ binary, cwd: fixture.dir });
    expect(suites).toEqual(['rulebearing']);
    expect(tests.map((t) => t.name)).toEqual([...expected.keys()]);
    for (const test of tests) {
      const text = expected.get(test.name) ?? '';
      expect(outcome(test.body), test.name).toBe(text === '' ? undefined : text);
      expect(test.options.meta?.rulebearing?.message, test.name).toBe(text);
    }

    const byName = new Map(tests.map((t) => [t.name, t]));
    expect(outcome(byName.get('nothing-matches')?.body ?? (() => undefined))).toBe(
      'rule `nothing-matches` is vacuous: its from side matched nothing, so it checks nothing (ADR-0007)',
    );
    expect(outcome(byName.get('handlers-not-to-util')?.body ?? (() => undefined))).toMatch(
      /^Call app\.service instead of importing app\.util\.\n(RB-[0-9a-f]{8} py\/app\/h\d\.py -> py\/app\/util\.py \(line 1, column 17\)\n){5}\.\.\. and 2 more$/,
    );
    expect(byName.get('api-not-to-util')?.options.meta?.rulebearing?.output).toEqual([
      expect.stringMatching(/^warn: RB-[0-9a-f]{8} py\/api\/view\.py -> py\/app\/util\.py/),
    ]);
  });

  it('keep anonymous rules apart and never follow options.outputTo (unnamed.yaml)', () => {
    const binary = localBinary();
    const fixture = copyFixture();
    cleanup = fixture.remove;
    const expected = junitMessages(binary, fixture.dir, ['--config', 'unnamed.yaml']);
    expect([...expected.keys()]).toEqual(['unnamed', 'unnamed#2', 'unnamed#3', 'unnamed#4']);
    const { tests } = register({ binary, cwd: fixture.dir, config: 'unnamed.yaml' });
    expect(tests.map((t) => t.name)).toEqual([...expected.keys()]);
    for (const test of tests) {
      const text = expected.get(test.name) ?? '';
      expect(outcome(test.body), test.name).toBe(text === '' ? undefined : text);
    }
    expect(expected.get('unnamed')).toMatch(/\n\.\.\. and 2 more$/);
    expect(expected.get('unnamed#2')).toBe('');
    expect(expected.get('unnamed#4')).toMatch(/^Rename the class in PascalCase\./);
    expect(existsSync(join(fixture.dir, 'must-not-be-written.txt'))).toBe(false);
  });

  it('are the same on two runs over the same inputs', () => {
    const binary = localBinary();
    const fixture = copyFixture();
    cleanup = fixture.remove;
    const first = register({ binary, cwd: fixture.dir }).tests.map((t) => [t.name, t.options]);
    const second = register({ binary, cwd: fixture.dir }).tests.map((t) => [t.name, t.options]);
    expect(second).toEqual(first);
  });
});
