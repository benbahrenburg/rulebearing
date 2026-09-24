// RulebearingReporter: the architecture rules of a vitest run, as one block an agent can act on.
//
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
// Source: design § Hooks, test runners, an MCP server, an LSP (docs/artifacts/design.md): "an agent
// already knows how to read a failing test". Requirement: FR-DIST-03.
//
// Add it beside vitest's own reporter. At the end of the run it prints, for the tests that
// defineArchitectureTests registered, how many rules ran and failed, then each failing rule's
// message (the fix first) and each passing rule's warn findings, without stack traces. It reads
// the message the test carried; it evaluates nothing.
//
//   // vitest.config.ts
//   import { RulebearingReporter } from 'rulebearing/vitest';
//   export default defineConfig({ test: { reporters: ['default', new RulebearingReporter()] } });

import type { Reporter, TestCase } from 'vitest/node';
import type { RuleMeta } from './define.js';

/** One rule test's outcome. */
export interface RuleOutcome extends RuleMeta {
  /** vitest's state for the test: `passed`, `failed`, `skipped` or `pending`. */
  readonly state: string;
}

/** Options of the reporter. */
export interface RulebearingReporterOptions {
  /** Where the block goes; standard error by default, so standard output stays vitest's. */
  readonly write?: (text: string) => void;
}

function indent(text: string): string {
  return text
    .split('\n')
    .map((line) => `    ${line}`)
    .join('\n');
}

/** The block the reporter prints for a run's rule tests; empty when there were none. */
export function summarise(outcomes: readonly RuleOutcome[]): string {
  if (outcomes.length === 0) {
    return '';
  }
  const failing = outcomes.filter((o) => o.state === 'failed');
  const skipped = outcomes.filter((o) => o.state === 'skipped').length;
  const lines = [
    `rulebearing: ${String(outcomes.length)} rules, ${String(failing.length)} failed${skipped > 0 ? `, ${String(skipped)} skipped` : ''}`,
  ];
  for (const outcome of failing) {
    lines.push(`  FAIL ${outcome.family} ${outcome.rule}`, indent(outcome.message));
  }
  for (const outcome of outcomes) {
    if (outcome.state !== 'failed' && outcome.output.length > 0) {
      lines.push(`  WARN ${outcome.family} ${outcome.rule}`, indent(outcome.output.join('\n')));
    }
  }
  return `${lines.join('\n')}\n`;
}

/** A vitest reporter that summarises the rule tests of `defineArchitectureTests`. */
export class RulebearingReporter implements Reporter {
  private readonly outcomes: RuleOutcome[] = [];
  private readonly write: (text: string) => void;

  constructor(options: RulebearingReporterOptions = {}) {
    this.write =
      options.write ??
      ((text) => {
        process.stderr.write(text);
      });
  }

  /** A new run starts: forget the last one's outcomes (watch mode reruns). */
  onTestRunStart(): void {
    this.outcomes.length = 0;
  }

  /** Records a rule test's outcome; other tests are not this reporter's. */
  onTestCaseResult(testCase: TestCase): void {
    const meta = testCase.meta().rulebearing;
    if (meta === undefined) {
      return;
    }
    this.outcomes.push({ ...meta, state: testCase.result().state });
  }

  /** Prints the block, in the order the rules ran. */
  onTestRunEnd(): void {
    const text = summarise(this.outcomes);
    if (text !== '') {
      this.write(text);
    }
  }
}

export default RulebearingReporter;
