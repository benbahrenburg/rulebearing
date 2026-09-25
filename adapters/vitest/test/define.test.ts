// defineArchitectureTests: the suite and tests it registers, the metadata they carry, and the one
// failing test for a run that wrote no result.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14 (2H).
import { describe, expect, it } from 'vitest';
import type { TestOptions } from 'vitest';
import type { Case } from '../src/cases.js';
import {
  ArchitectureRuleError,
  defineArchitectureTests,
  ruleBody,
  ruleMeta,
} from '../src/define.js';
import type { Registrar } from '../src/define.js';

function recorder(): {
  registrar: Registrar;
  suites: string[];
  tests: [string, TestOptions, () => void][];
} {
  const suites: string[] = [];
  const tests: [string, TestOptions, () => void][] = [];
  return {
    suites,
    tests,
    registrar: {
      describe: (name, body) => {
        suites.push(name);
        body();
      },
      test: (name, options, body) => {
        tests.push([name, options, body]);
      },
    },
  };
}

const FAILING: Case = {
  rule: { id: 'no-b', name: 'no-b', family: 'forbidden', severity: 'error' },
  failure: 'Move it.\nRB-1 a -> b',
  errors: [],
  output: ['warn: RB-2 c -> b'],
};

describe('ArchitectureRuleError', () => {
  it('carries the message alone, with no frames of its own', () => {
    const error = new ArchitectureRuleError('fix\nRB-1 a -> b');
    expect(error.name).toBe('ArchitectureRuleError');
    expect(error.message).toBe('fix\nRB-1 a -> b');
    expect(error.stack).toBe('ArchitectureRuleError: fix\nRB-1 a -> b');
  });
});

describe('ruleMeta and ruleBody', () => {
  it('describe a failing rule and throw its message', () => {
    expect(ruleMeta(FAILING)).toEqual({
      rule: 'no-b',
      family: 'forbidden',
      severity: 'error',
      message: 'Move it.\nRB-1 a -> b',
      output: ['warn: RB-2 c -> b'],
    });
    expect(ruleBody(FAILING)).toThrow(new ArchitectureRuleError('Move it.\nRB-1 a -> b'));
  });

  it('describe a passing rule with an empty message and do not throw', () => {
    const passing: Case = { ...FAILING, failure: undefined, output: [] };
    expect(ruleMeta(passing).message).toBe('');
    expect(ruleBody(passing)).not.toThrow();
  });
});

describe('defineArchitectureTests', () => {
  it('registers one failing test, cruise, when the run writes no result', () => {
    const { registrar, suites, tests } = recorder();
    const found = defineArchitectureTests(
      { binary: '/no/such/rulebearing', suite: 'architecture' },
      registrar,
    );
    expect(found).toEqual([]);
    expect(suites).toEqual(['architecture']);
    expect(tests.map(([name]) => name)).toEqual(['cruise']);
    const body = tests[0]?.[2] ?? (() => undefined);
    expect(body).toThrow(ArchitectureRuleError);
    expect(body).toThrow(/could not start/);
  });

  it('lets an error that is not a failed run propagate', () => {
    const { registrar } = recorder();
    const env = new Proxy(
      {},
      {
        get: () => {
          throw new TypeError('environment unreadable');
        },
      },
    );
    expect(() => defineArchitectureTests({}, registrar, env)).toThrow(TypeError);
  });

  it('registers with vitest by default', () => {
    // Registering inside a running test is refused by vitest; that refusal proves the default
    // registrar is vitest's own `describe`.
    expect(() => defineArchitectureTests({ binary: '/no/such/rulebearing' })).toThrow();
  });
});
