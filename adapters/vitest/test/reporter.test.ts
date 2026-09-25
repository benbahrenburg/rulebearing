// RulebearingReporter: the block it prints for the rule tests of a run, and nothing for others.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14 (2H).
import { describe, expect, it } from 'vitest';
import type { TestCase } from 'vitest/node';
import type { RuleMeta } from '../src/define.js';
import { RulebearingReporter, summarise } from '../src/reporter.js';
import type { RuleOutcome } from '../src/reporter.js';

function testCase(meta: RuleMeta | undefined, state: string): TestCase {
  return {
    meta: () => (meta === undefined ? {} : { rulebearing: meta }),
    result: () => ({ state }),
  } as unknown as TestCase;
}

const NO_B: RuleMeta = {
  rule: 'no-b',
  family: 'forbidden',
  severity: 'error',
  message: 'Move it.\nRB-1 a -> b',
  output: [],
};
const API: RuleMeta = {
  rule: 'api',
  family: 'forbidden',
  severity: 'warn',
  message: '',
  output: ['warn: RB-2 c -> b', 'warn: RB-3 d -> b'],
};

describe('summarise', () => {
  it('is empty when no rule test ran', () => {
    expect(summarise([])).toBe('');
  });

  it('counts the rules, then prints each failure and each warning, indented', () => {
    const outcomes: RuleOutcome[] = [
      { ...NO_B, state: 'failed' },
      { ...API, state: 'passed' },
      { ...API, rule: 'skipped-one', output: [], state: 'skipped' },
    ];
    expect(summarise(outcomes)).toBe(
      [
        'rulebearing: 3 rules, 1 failed, 1 skipped',
        '  FAIL forbidden no-b',
        '    Move it.',
        '    RB-1 a -> b',
        '  WARN forbidden api',
        '    warn: RB-2 c -> b',
        '    warn: RB-3 d -> b',
        '',
      ].join('\n'),
    );
  });

  it('leaves the skipped count out when nothing was skipped', () => {
    expect(summarise([{ ...API, output: [], state: 'passed' }])).toBe(
      'rulebearing: 1 rules, 0 failed\n',
    );
  });
});

describe('RulebearingReporter', () => {
  it('prints the rule tests of a run and ignores other tests', () => {
    const written: string[] = [];
    const reporter = new RulebearingReporter({ write: (text) => written.push(text) });
    reporter.onTestRunStart();
    reporter.onTestCaseResult(testCase(undefined, 'failed'));
    reporter.onTestCaseResult(testCase(NO_B, 'failed'));
    reporter.onTestCaseResult(testCase(API, 'passed'));
    reporter.onTestRunEnd();
    expect(written).toEqual([
      summarise([
        { ...NO_B, state: 'failed' },
        { ...API, state: 'passed' },
      ]),
    ]);
  });

  it('prints nothing for a run without rule tests, and starts afresh on a rerun', () => {
    const written: string[] = [];
    const reporter = new RulebearingReporter({ write: (text) => written.push(text) });
    reporter.onTestCaseResult(testCase(NO_B, 'failed'));
    reporter.onTestRunStart();
    reporter.onTestCaseResult(testCase(undefined, 'passed'));
    reporter.onTestRunEnd();
    expect(written).toEqual([]);
  });

  it('writes to standard error by default', () => {
    const reporter = new RulebearingReporter();
    const original = process.stderr.write.bind(process.stderr);
    const captured: string[] = [];
    process.stderr.write = (chunk: string) => {
      captured.push(chunk);
      return true;
    };
    try {
      reporter.onTestCaseResult(testCase(NO_B, 'failed'));
      reporter.onTestRunEnd();
    } finally {
      process.stderr.write = original;
    }
    expect(captured.join('')).toContain('FAIL forbidden no-b');
  });
});
