// `defineArchitectureTests()`: one vitest `test` per rule of `rulebearing cruise`.
//
// Contract: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 1.5 (the adapters'
// failure message is the junit message text).
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
// Decisions: docs/adr/0007-vacuous-rules-fail-by-default.md; docs/adr/0010 rule 4.
// Requirement: FR-DIST-03.
//
// The binary runs once, while vitest collects the test file; each rule becomes a test that throws
// an ArchitectureRuleError whose message is exactly the junit message for that rule. The rule's
// family, name, severity, message and non-failing output ride along as task metadata, which
// RulebearingReporter reads. Nothing here evaluates a rule.

import { describe as vitestDescribe, test as vitestTest } from 'vitest';
import type { TestOptions } from 'vitest';
import { cases, failed, message } from './cases.js';
import type { Case } from './cases.js';
import { CruiseError, cruise } from './run.js';
import type { RunOptions } from './run.js';

/** What a rule test carries for reporters, under `task.meta.rulebearing`. */
export interface RuleMeta {
  readonly rule: string;
  readonly family: string;
  readonly severity: string;
  /** The failure message; empty when the rule passes. */
  readonly message: string;
  /** Warn, info and known findings, which do not fail the rule. */
  readonly output: readonly string[];
}

declare module 'vitest' {
  interface TaskMeta {
    /** Set on every test `defineArchitectureTests` registers. */
    rulebearing?: RuleMeta;
  }
}

/** A rule failed; the message is the junit message text, with no stack of its own. */
export class ArchitectureRuleError extends Error {
  override readonly name = 'ArchitectureRuleError';

  constructor(text: string) {
    super(text);
    // The rule, not this adapter's frames, is what failed: print the message alone.
    this.stack = `${this.name}: ${text}`;
  }
}

/** Options of `defineArchitectureTests`. */
export interface ArchitectureTestOptions extends RunOptions {
  /** The name of the suite the rule tests are registered in; `rulebearing` by default. */
  readonly suite?: string;
}

/** The two vitest functions the helper registers with, replaceable in tests. */
export interface Registrar {
  readonly describe: (name: string, body: () => void) => void;
  readonly test: (name: string, options: TestOptions, body: () => void) => void;
}

const vitest: Registrar = {
  describe: (name, body) => {
    vitestDescribe(name, body);
  },
  test: (name, options, body) => {
    vitestTest(name, options, body);
  },
};

/** The metadata a rule's test carries. */
export function ruleMeta(found: Case): RuleMeta {
  return {
    rule: found.rule.name,
    family: found.rule.family,
    severity: found.rule.severity,
    message: failed(found) ? message(found) : '',
    output: [...found.output],
  };
}

/** The body of a rule's test: throws the rule's message when it failed. */
export function ruleBody(found: Case): () => void {
  return () => {
    if (failed(found)) {
      throw new ArchitectureRuleError(message(found));
    }
  };
}

/**
 * Runs `rulebearing cruise --output-type json` and registers one test per rule, inside a
 * `describe` named `options.suite`. A run that writes no result registers one failing test,
 * `cruise`, whose message is the command, its exit code and its stderr.
 *
 * ```ts
 * // architecture.test.ts
 * import { defineArchitectureTests } from 'rulebearing/vitest';
 * defineArchitectureTests({ config: 'rulebearing.yaml' });
 * ```
 *
 * @returns the cases the tests were registered for; empty when the run failed.
 */
export function defineArchitectureTests(
  options: ArchitectureTestOptions = {},
  registrar: Registrar = vitest,
  env: NodeJS.ProcessEnv = process.env,
): Case[] {
  let found: Case[];
  try {
    found = cases(cruise(options, env));
  } catch (error) {
    if (!(error instanceof CruiseError)) {
      throw error;
    }
    registrar.describe(options.suite ?? 'rulebearing', () => {
      registrar.test('cruise', {}, () => {
        throw new ArchitectureRuleError(error.message);
      });
    });
    return [];
  }
  registrar.describe(options.suite ?? 'rulebearing', () => {
    for (const item of found) {
      registrar.test(item.rule.id, { meta: { rulebearing: ruleMeta(item) } }, ruleBody(item));
    }
  });
  return found;
}
