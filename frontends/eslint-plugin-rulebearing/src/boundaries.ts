// `rulebearing/boundaries`: each import asks the cached graph `can-import`, and a `no` is reported
// at the import with the rule's name, its `fix` and the violation id.
//
// Source: docs/artifacts/design.md#two-front-ends-that-will-matter-more-than-the-mcp-server
// ("one rule (`rulebearing/boundaries`) that asks the cached graph `can-import` for each import
// statement and reports inline"). Decision: docs/adr/0021-agent-surface-cli-first.md.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 13. Requirement: FR-DIST-04.
//
// Import forms: `import`, `export * from`, `export { } from`, `import()` and `require()` with a
// string. The target is resolved off the graph's `modules[]` (src/graph.ts); an import the graph
// cannot name is left to the gate, which re-extracts. Each file's answers are memoised for the
// file's lint, so a target imported twice is asked once. Options: `config` (the rules file),
// `graph` (a graph document instead of the worktree-aware cache) and `severity` (the lowest rule
// severity reported: `error`, the default, reports what fails the gate; `warn` and `info` add the
// rules that only warn or inform).

import { join, resolve } from 'node:path';
import type { Rule } from 'eslint';
import type { Answer, Finding, Invocation } from './cli.js';
import { CliError, canImport, locateBinary, warmCache } from './cli.js';
import type { Graph } from './graph.js';
import { GraphError, findCachedGraph, graphPath, loadGraph, resolveSpecifier } from './graph.js';
import type { Position } from './message.js';
import { message } from './message.js';

/** The rule's options. */
export interface BoundariesOptions {
  readonly config?: string;
  readonly graph?: string;
  readonly severity?: 'error' | 'warn' | 'info';
}

const RANK: Readonly<Record<string, number>> = { error: 3, warn: 2, info: 1 };

/** The findings of an answer at or above `severity`, deciding rules first. */
export function reported(answer: Answer, severity: BoundariesOptions['severity']): Finding[] {
  const floor = RANK[severity ?? 'error'] ?? 3;
  return [...answer.violations, ...answer.warnings].filter((f) => (RANK[f.severity] ?? 0) >= floor);
}

type Outcome =
  | { readonly kind: 'answer'; readonly answer: Answer }
  | { readonly kind: 'error'; readonly message: string };

function reason(error: unknown): string {
  if (error instanceof CliError || error instanceof GraphError) {
    return error.message;
  }
  throw error;
}

/** The graph resolution reads: `graph` when given, else the cache entry, written on a miss. */
function graphFor(invocation: Invocation, from: string): Graph {
  if (invocation.graph !== undefined) {
    return loadGraph(resolve(invocation.cwd, invocation.graph));
  }
  let path = findCachedGraph(invocation.cwd);
  if (path === undefined) {
    warmCache(invocation, from);
    path = findCachedGraph(invocation.cwd);
  }
  if (path === undefined) {
    throw new GraphError(
      `no graph under ${join(invocation.cwd, '.graph', 'cache')} after extracting; pass the graph option`,
    );
  }
  return loadGraph(path);
}

/** A string literal source, or a template literal without expressions. */
function literal(node: unknown): string | undefined {
  if (typeof node !== 'object' || node === null || !('type' in node)) {
    return undefined;
  }
  if (node.type === 'Literal' && 'value' in node && typeof node.value === 'string') {
    return node.value;
  }
  if (
    node.type === 'TemplateLiteral' &&
    'expressions' in node &&
    Array.isArray(node.expressions) &&
    node.expressions.length === 0 &&
    'quasis' in node &&
    Array.isArray(node.quasis)
  ) {
    const [quasi] = node.quasis as unknown[];
    if (
      typeof quasi === 'object' &&
      quasi !== null &&
      'value' in quasi &&
      typeof quasi.value === 'object' &&
      quasi.value !== null &&
      'cooked' in quasi.value &&
      typeof quasi.value.cooked === 'string'
    ) {
      return quasi.value.cooked;
    }
  }
  return undefined;
}

export const boundaries: Rule.RuleModule = {
  meta: {
    type: 'problem',
    docs: {
      description:
        'Report an import the rulebearing gate would fail, with the rule, its fix and the violation id',
      url: 'https://github.com/benbahrenburg/rulebearing/tree/main/frontends/eslint-plugin-rulebearing#readme',
    },
    schema: [
      {
        type: 'object',
        properties: {
          config: { type: 'string' },
          graph: { type: 'string' },
          severity: { enum: ['error', 'warn', 'info'] },
        },
        additionalProperties: false,
      },
    ],
    messages: { violation: '{{text}}', unanswered: 'rulebearing could not answer: {{text}}' },
  },
  create(context) {
    const options = (context.options[0] ?? {}) as BoundariesOptions;
    const cwd = context.cwd;
    const from = graphPath(cwd, context.filename);
    const answers = new Map<string, Outcome>();
    let setup: { invocation: Invocation; graph: Graph } | { error: string } | undefined;

    const prepare = (): { invocation: Invocation; graph: Graph } | { error: string } => {
      if (setup === undefined) {
        try {
          const invocation: Invocation = {
            binary: locateBinary(),
            cwd,
            config: options.config,
            graph: options.graph,
          };
          setup = { invocation, graph: graphFor(invocation, from) };
        } catch (error) {
          setup = { error: reason(error) };
        }
      }
      return setup;
    };

    const ask = (invocation: Invocation, to: string): Outcome => {
      const known = answers.get(to);
      if (known !== undefined) {
        return known;
      }
      let outcome: Outcome;
      try {
        outcome = { kind: 'answer', answer: canImport(invocation, from, to) };
      } catch (error) {
        outcome = { kind: 'error', message: reason(error) };
      }
      answers.set(to, outcome);
      return outcome;
    };

    const check = (node: Rule.Node, source: unknown): void => {
      const specifier = literal(source);
      if (specifier === undefined) {
        return;
      }
      const ready = prepare();
      if ('error' in ready) {
        context.report({ node, messageId: 'unanswered', data: { text: ready.error } });
        return;
      }
      const to = resolveSpecifier(ready.graph, from, specifier);
      if (to === undefined) {
        return;
      }
      const outcome = ask(ready.invocation, to);
      if (outcome.kind === 'error') {
        context.report({ node, messageId: 'unanswered', data: { text: outcome.message } });
        return;
      }
      const at: Position = {
        line: node.loc?.start.line ?? 1,
        column: (node.loc?.start.column ?? 0) + 1,
      };
      for (const finding of reported(outcome.answer, options.severity)) {
        context.report({
          node,
          messageId: 'violation',
          data: { text: message(finding, outcome.answer.from, outcome.answer.to, at) },
        });
      }
    };

    return {
      ImportDeclaration(node) {
        check(node, node.source);
      },
      ExportAllDeclaration(node) {
        check(node, node.source);
      },
      ExportNamedDeclaration(node) {
        if (node.source !== null && node.source !== undefined) {
          check(node, node.source);
        }
      },
      ImportExpression(node) {
        check(node, node.source);
      },
      CallExpression(node) {
        if (
          node.callee.type === 'Identifier' &&
          node.callee.name === 'require' &&
          node.arguments.length === 1
        ) {
          check(node, node.arguments[0]);
        }
      },
    };
  },
};
