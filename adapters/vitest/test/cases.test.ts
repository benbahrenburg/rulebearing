// The cases read from the JSON, against the fixed vectors of crates/rb-report (junit.rs, catalog.rs).
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14 (2H).
import { describe, expect, it } from 'vitest';
import {
  SHOWN,
  cases,
  describe as describeViolation,
  failed,
  jsNumber,
  message,
  ruleIndex,
  ruleName,
  rules,
  severity,
  text,
} from '../src/cases.js';
import type { Case } from '../src/cases.js';

// The vector of crates/rb-report/src/junit.rs, a_rule_per_case_with_failures_errors_and_output.
const JUNIT_VECTOR = {
  modules: [{ source: 'a.ts', dependencies: [{ resolved: 'b.ts', line: 2, column: 1 }] }],
  summary: {
    violations: [
      {
        type: 'dependency',
        from: 'a.ts',
        to: 'b.ts',
        rule: { name: 'no-b', severity: 'error' },
        id: 'RB-1',
      },
      {
        type: 'dependency',
        from: 'c.ts',
        to: 'b.ts',
        rule: { name: 'no-b', severity: 'ignore' },
        id: 'RB-2',
      },
      {
        type: 'element',
        from: 's.cs',
        to: 'S<T>',
        rule: { name: 'sealed', severity: 'warn' },
        id: 'RB-3',
      },
    ],
    ruleSetUsed: {
      forbidden: [
        { name: 'no-b', severity: 'error', fix: 'Use "the" index & go.' },
        { name: 'dead' },
      ],
      elements: [{ name: 'sealed', severity: 'warn' }],
    },
    vacuousRules: [{ name: 'dead', side: 'from' }],
    expired: [{ name: 'RB-9', expires: '2026-01-01', kind: 'knownViolation' }],
  },
};

function violation(name: string, index: number, level = 'error', extra: object = {}): object {
  return {
    type: 'dependency',
    from: `src/m${String(index)}.ts`,
    to: 'src/b.ts',
    rule: { name, severity: level },
    id: `RB-${String(index)}`,
    ...extra,
  };
}

describe('cases', () => {
  it('reads the junit vector case for case', () => {
    const found = cases(JUNIT_VECTOR);
    expect(found.map((c) => [c.rule.name, c.rule.family])).toEqual([
      ['no-b', 'forbidden'],
      ['dead', 'forbidden'],
      ['sealed', 'elements'],
      ['RB-9', 'knownViolations'],
    ]);
    const [noB, dead, sealed, expired] = found as [Case, Case, Case, Case];
    expect(noB.failure).toBe('Use "the" index & go.\nRB-1 a.ts -> b.ts (line 2, column 1)');
    expect(noB.output).toEqual(['ignore: RB-2 c.ts -> b.ts [known]']);
    expect(dead.errors).toEqual([
      [
        'vacuous',
        'rule `dead` is vacuous: its from side matched nothing, so it checks nothing (ADR-0007)',
      ],
    ]);
    expect(failed(sealed)).toBe(false);
    expect(sealed.output).toEqual(['warn: RB-3 s.cs -> S<T>']);
    expect(expired.errors).toEqual([
      [
        'expired',
        'knownViolation `RB-9` expired on 2026-01-01; it no longer applies and the run fails',
      ],
    ]);
  });

  it('lists five errors, then counts the rest', () => {
    const result = {
      summary: {
        violations: Array.from({ length: 8 }, (_, i) => violation('many', i + 1)),
        ruleSetUsed: { forbidden: [{ name: 'many', severity: 'error' }] },
      },
    };
    const lines = (cases(result)[0]?.failure ?? '').split('\n');
    expect(lines[0]).toBe('8 violation(s) of `many`');
    expect(lines.slice(1, 6)).toEqual(
      [1, 2, 3, 4, 5].map((i) => `RB-${String(i)} src/m${String(i)}.ts -> src/b.ts`),
    );
    expect(lines[6]).toBe('... and 3 more');
    expect(lines).toHaveLength(SHOWN + 2);
  });

  it("takes a violation's fix over the rule's, even an empty one", () => {
    const result = {
      summary: {
        violations: [violation('r', 1, 'error', { fix: '' })],
        ruleSetUsed: { forbidden: [{ name: 'r', severity: 'error', fix: 'rule fix' }] },
      },
    };
    expect(cases(result)[0]?.failure).toBe('\nRB-1 src/m1.ts -> src/b.ts');
  });

  it('makes the allowed list one rule at its severity', () => {
    const result = {
      summary: {
        violations: [violation('not-in-allowed', 1)],
        ruleSetUsed: {
          allowed: [{ comment: 'c', fix: 'stay inside' }, {}],
          allowedSeverity: 'error',
        },
      },
    };
    expect(rules(result)).toEqual([
      {
        id: 'not-in-allowed',
        name: 'not-in-allowed',
        family: 'allowed',
        severity: 'error',
        comment: 'c',
        fix: 'stay inside',
      },
    ]);
    expect(cases(result)[0]?.failure).toBe('stay inside\nRB-1 src/m1.ts -> src/b.ts');
    expect(rules({ summary: { ruleSetUsed: { allowed: [{}] } } })[0]?.severity).toBe('warn');
  });

  it('orders families, then unlisted rules by name, then ratchets, then stray vacuous entries', () => {
    const result = {
      summary: {
        violations: [
          violation('zeta', 1, 'error', { comment: 'z', fix: 'fz' }),
          violation('alpha', 2, 'warn'),
          violation('req', 3),
        ],
        ruleSetUsed: {
          diagrams: [{ name: 'd' }],
          slices: [{ name: 's' }],
          elements: [{ name: 'e', severity: 'info' }],
          required: [{ name: 'req' }],
          forbidden: [{ name: 'f' }],
        },
        ratchets: [{ name: 'budget', status: 'within', count: 1, ceiling: 2 }],
        vacuousRules: [
          { name: 'f', side: 'from' },
          { name: 'ghost', side: 'to' },
        ],
      },
    };
    expect(rules(result).map((r) => [r.name, r.family, r.severity])).toEqual([
      ['f', 'forbidden', 'warn'],
      ['req', 'required', 'warn'],
      ['e', 'elements', 'info'],
      ['s', 'slices', 'warn'],
      ['d', 'diagrams', 'warn'],
      ['alpha', 'rules', 'warn'],
      ['zeta', 'rules', 'error'],
      ['budget', 'ratchets', 'error'],
      ['ghost', 'rules', 'error'],
    ]);
    const zeta = rules(result).find((r) => r.name === 'zeta');
    expect([zeta?.comment, zeta?.fix]).toEqual(['z', 'fz']);
  });

  it('reports ratchets exceeded, without a budget, within, and unread', () => {
    const result = {
      summary: {
        ratchets: [
          { name: 'up', status: 'exceeded', count: 12, ceiling: 10.0, budget: 'b.json' },
          { name: 'lost', status: 'no-budget', count: 3, budget: 'gone.json' },
          { name: 'ok', status: 'within', count: 1.5, ceiling: 2, budget: 'b.json' },
          { name: 'unread' },
        ],
      },
    };
    const [up, lost, ok, unread] = cases(result) as [Case, Case, Case, Case];
    expect(up.failure).toBe('ratchet `up`: 12 edges exceed the ceiling of 10 in b.json');
    expect(lost.errors).toEqual([
      [
        'no-budget',
        'ratchet `lost`: the budget gone.json cannot be read, so the count 3 is checked against nothing',
      ],
    ]);
    expect(ok.output).toEqual(['1.5 edges, within the ceiling of 2 in b.json']);
    expect(unread.output).toEqual([
      'undefined edges, within the ceiling of undefined in undefined',
    ]);
  });

  it('makes a warn vacuous rule output and an expired rule an error', () => {
    const result = {
      summary: {
        ruleSetUsed: { forbidden: [{ name: 'stale' }, { name: 'old' }] },
        vacuousRules: [{ name: 'stale', side: 'from', severity: 'warn' }],
        expired: [{ name: 'old', kind: 'rule', expires: '2026-02-03' }],
      },
    };
    const [stale, old] = cases(result) as [Case, Case];
    expect(failed(stale)).toBe(false);
    expect(stale.output).toEqual([
      'warning: rule `stale` is vacuous: its from side matched nothing, so it checks nothing (ADR-0007)',
    ]);
    expect(old.errors).toEqual([
      ['expired', 'rule `old` expired on 2026-02-03; it no longer applies and the run fails'],
    ]);
    expect(cases(result)).toHaveLength(2);
  });
});

describe('describe', () => {
  it('places an element violation at its type declaration', () => {
    const result = {
      code: {
        types: [
          { fullName: 'app.Other', line: 9 },
          { fullName: 'app.shapes.shape', line: 4, column: 7 },
          { fullName: 'app.NoColumn', line: 2 },
          { fullName: 'app.NoLine' },
        ],
      },
    };
    const element = { type: 'element', from: 'py/app/shapes.py', rule: { severity: 'error' } };
    expect(describeViolation(result, { ...element, to: 'app.shapes.shape' })).toBe(
      'py/app/shapes.py -> app.shapes.shape (line 4, column 7)',
    );
    expect(describeViolation(result, { ...element, to: 'app.NoColumn' })).toMatch(
      /\(line 2, column 1\)$/,
    );
    expect(describeViolation(result, { ...element, to: 'app.NoLine' })).toBe(
      'py/app/shapes.py -> app.NoLine',
    );
    expect(describeViolation({}, { ...element, to: 'x' })).toBe('py/app/shapes.py -> x');
  });

  it('places a dependency at its first matching edge', () => {
    const result = {
      modules: [
        { source: 'a', dependencies: [{ resolved: 'b', line: 1 }] },
        { source: 'a', dependencies: [{ resolved: 'b', line: 5, column: 2 }] },
        { source: 'c', dependencies: [{ resolved: 'd', line: true, column: 1 }] },
        { source: 'e', dependencies: [{ resolved: 'f', line: 3.5, column: 1 }] },
      ],
    };
    const dependency = { rule: { severity: 'error' } };
    expect(describeViolation(result, { ...dependency, from: 'a', to: 'b' })).toBe('a -> b');
    expect(describeViolation(result, { ...dependency, from: 'c', to: 'd' })).toBe('c -> d');
    expect(describeViolation(result, { ...dependency, from: 'e', to: 'f' })).toBe('e -> f');
    expect(describeViolation(result, { ...dependency, from: 'x', to: 'b' })).toBe('x -> b');
    expect(describeViolation(result, { from: 'a', to: 'zz' })).toBe('a -> zz');
  });

  it('prints missing fields as JavaScript would', () => {
    expect(describeViolation({}, {})).toBe('undefined -> undefined');
    expect(severity({})).toBe('');
    expect(severity({ rule: null })).toBe('undefined');
    expect(ruleName({ rule: { name: 3 } })).toBe('');
    expect(text({ n: 7 }, 'n')).toBe('7');
    expect(text('not an object', 'n')).toBe('undefined');
    expect(cases(null)).toEqual([]);
    expect(cases({ summary: { violations: 'not a list' } })).toEqual([]);
  });
});

describe('jsNumber', () => {
  it.each([
    [undefined, false, 'undefined'],
    [null, true, 'null'],
    ['s', true, 's'],
    [true, true, 'true'],
    [false, true, 'false'],
    [42, true, '42'],
    [-3, true, '-3'],
    [5.0, true, '5'],
    [1.5, true, '1.5'],
    [1e21, true, '1e+21'],
    [1.5e-7, true, '1.5e-7'],
    [1.5e-6, true, '1.5e-6'],
    [-3.25e-6, true, '-3.25e-6'],
    [1.5e-5, true, '0.000015'],
    [-2e-5, true, '-0.00002'],
    [0.25, true, '0.25'],
    [['py'], true, '["py"]'],
    [{ a: 'é' }, true, '{"a":"é"}'],
  ])('prints %j as serde_json then JavaScript would', (value, present, printed) => {
    expect(jsNumber(value, present)).toBe(printed);
  });
});

describe('message', () => {
  it('is the failure, then each error, one per line; empty for a passing case', () => {
    const found: Case = {
      rule: { id: 'r', name: 'r', family: 'forbidden', severity: 'error' },
      failure: 'fix\nRB-1 a -> b',
      errors: [
        ['expired', 'rule `r` expired on 2026-01-01; it no longer applies and the run fails'],
      ],
      output: [],
    };
    expect(message(found)).toBe(
      'fix\nRB-1 a -> b\nrule `r` expired on 2026-01-01; it no longer applies and the run fails',
    );
    expect(message({ ...found, failure: undefined, errors: [] })).toBe('');
  });

  it('is deterministic', () => {
    expect(cases(JUNIT_VECTOR).map(message)).toEqual(cases(JUNIT_VECTOR).map(message));
  });
});

// The fixed vectors of crates/rb-report/src/catalog.rs,
// rules_sharing_a_name_keep_distinct_identities_and_their_own_violations.
describe('rules sharing a name', () => {
  const violation = (type: string, from: string, to: string, level: string): object => ({
    type,
    from,
    to,
    rule: { name: 'unnamed', severity: level },
  });

  it('keep distinct identities and their own violations', () => {
    const result = {
      summary: {
        violations: [
          violation('dependency', 'a', 'b', 'error'),
          violation('dependency', 'c', 'd', 'warn'),
          violation('element', 'e.cs', 'E', 'error'),
          violation('dependency', 'f', 'g', 'ignore'),
        ],
        ruleSetUsed: {
          forbidden: [
            { name: 'unnamed', severity: 'error' },
            { name: 'unnamed#2' },
            { severity: 'warn', name: 'unnamed' },
          ],
          elements: [{ name: 'unnamed', severity: 'error' }],
        },
        vacuousRules: [{ name: 'unnamed', side: 'from' }],
      },
    };
    const catalogue = rules(result);
    expect(catalogue.map((r) => [r.id, r.family])).toEqual([
      ['unnamed', 'forbidden'],
      ['unnamed#2', 'forbidden'],
      ['unnamed#3', 'forbidden'],
      ['unnamed#4', 'elements'],
    ]);
    expect(result.summary.violations.map((v) => ruleIndex(catalogue, v))).toEqual([0, 2, 3, 0]);
    expect(ruleIndex(catalogue, { rule: { name: 'none' } })).toBeUndefined();
    const found = cases(result);
    expect(found.map((c) => c.failure)).toEqual([
      '1 violation(s) of `unnamed`\na -> b',
      undefined,
      undefined,
      '1 violation(s) of `unnamed`\ne.cs -> E',
    ]);
    expect(found[0]?.output).toEqual(['ignore: f -> g [known]']);
    expect(found[2]?.output).toEqual(['warn: c -> d']);
    expect(found[0]?.errors).toHaveLength(1);
    expect(found.slice(1).every((c) => c.errors.length === 0)).toBe(true);
  });

  it('an element violation named like a dependency rule is a rule of its own', () => {
    const result = {
      summary: {
        violations: [
          violation('dependency', 'a', 'b', 'error'),
          violation('element', 'e.cs', 'E', 'error'),
        ],
        ruleSetUsed: { forbidden: [{ name: 'unnamed', severity: 'error' }] },
      },
    };
    const catalogue = rules(result);
    expect(catalogue.map((r) => [r.id, r.family])).toEqual([
      ['unnamed', 'forbidden'],
      ['unnamed#2', 'rules'],
    ]);
    expect(result.summary.violations.map((v) => ruleIndex(catalogue, v))).toEqual([0, 1]);
  });
});
