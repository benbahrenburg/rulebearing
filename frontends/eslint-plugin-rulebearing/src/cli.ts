// The binary, asked `can-import <from> <to> --json`: the gate's own answer for one edge.
//
// Source: docs/artifacts/design.md#two-front-ends-that-will-matter-more-than-the-mcp-server.
// Decisions: docs/adr/0021-agent-surface-cli-first.md (the CLI is the agent surface),
// docs/adr/0015-stable-violation-id.md (the id), docs/adr/0008-exit-code-contract.md (0 yes, 1 no,
// 2 untrustworthy, 3 invalid configuration).
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 13. Requirement: FR-DIST-04.
//
// The binary is located through the npm wrapper (wrappers/npm), so the plugin runs the one
// `npx rulebearing` runs, and `RULEBEARING_BINARY` points both at a local build. The plugin never
// evaluates a rule itself.

import { spawnSync } from 'node:child_process';
import { detectHost, resolveBinary } from 'rulebearing';

/** How the binary is asked: where, with which configuration and which graph. */
export interface Invocation {
  /** The binary's path. */
  readonly binary: string;
  /** The folder the binary runs in; graph paths are relative to it. */
  readonly cwd: string;
  /** `--config FILE`, else the binary looks for its configuration in `cwd`. */
  readonly config?: string | undefined;
  /** `--graph FILE`, else the worktree-aware cache. */
  readonly graph?: string | undefined;
}

/** One rule the edge matches, as `can-import --json` reports it. */
export interface Finding {
  readonly name: string;
  readonly severity: string;
  readonly id: string;
  readonly comment?: string;
  readonly fix?: string;
}

/** The answer for one edge. */
export interface Answer {
  readonly verdict: 'yes' | 'no';
  readonly from: string;
  readonly to: string;
  readonly violations: readonly Finding[];
  readonly warnings: readonly Finding[];
}

/** The binary could not be found, started, or trusted; the message says why. */
export class CliError extends Error {
  override readonly name = 'CliError';
}

/** The binary the npm wrapper would run on this host. */
export function locateBinary(env: NodeJS.ProcessEnv = process.env): string {
  const found = resolveBinary(detectHost(), env);
  if (found.kind === 'missing') {
    throw new CliError(found.message);
  }
  return found.path;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function findings(value: unknown, field: string): Finding[] {
  if (!Array.isArray(value)) {
    throw new CliError(`can-import --json: \`${field}\` is not a list`);
  }
  return value.map((entry): Finding => {
    if (
      !isRecord(entry) ||
      typeof entry.name !== 'string' ||
      typeof entry.severity !== 'string' ||
      typeof entry.id !== 'string'
    ) {
      throw new CliError(`can-import --json: an entry of \`${field}\` has no name, severity or id`);
    }
    return {
      name: entry.name,
      severity: entry.severity,
      id: entry.id,
      ...(typeof entry.comment === 'string' ? { comment: entry.comment } : {}),
      ...(typeof entry.fix === 'string' ? { fix: entry.fix } : {}),
    };
  });
}

/** Reads the object `can-import --json` prints. */
export function parseAnswer(text: string): Answer {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new CliError(`can-import --json printed something that is not JSON: ${text.trim()}`);
  }
  if (
    !isRecord(value) ||
    (value.verdict !== 'yes' && value.verdict !== 'no') ||
    typeof value.from !== 'string' ||
    typeof value.to !== 'string'
  ) {
    throw new CliError('can-import --json: the answer has no verdict, from or to');
  }
  return {
    verdict: value.verdict,
    from: value.from,
    to: value.to,
    violations: findings(value.violations, 'violations'),
    warnings: findings(value.warnings, 'warnings'),
  };
}

function common(invocation: Invocation): string[] {
  return [
    ...(invocation.config === undefined ? [] : ['--config', invocation.config]),
    ...(invocation.graph === undefined ? [] : ['--graph', invocation.graph]),
  ];
}

function run(invocation: Invocation, args: readonly string[]): { code: number; stdout: string } {
  const result = spawnSync(invocation.binary, args, {
    cwd: invocation.cwd,
    encoding: 'utf8',
    windowsHide: true,
    maxBuffer: 16 * 1024 * 1024,
  });
  if (result.error !== undefined) {
    throw new CliError(`could not start ${invocation.binary}: ${result.error.message}`);
  }
  const code = result.status ?? 2;
  if (code !== 0 && code !== 1) {
    const reason = result.stderr.trim() || `exit ${String(code)}`;
    throw new CliError(reason);
  }
  return { code, stdout: result.stdout };
}

/** Asks whether `from` may import `to`. Exit 0 is yes and 1 is no; anything else is an error. */
export function canImport(invocation: Invocation, from: string, to: string): Answer {
  const { stdout } = run(invocation, ['can-import', from, to, '--json', ...common(invocation)]);
  return parseAnswer(stdout);
}

/**
 * Writes this worktree's cache entry when there is none, through a query command that reads the
 * cache and extracts on a miss (`impact`), so resolution has a graph to read.
 */
export function warmCache(invocation: Invocation, file: string): void {
  run(invocation, ['impact', file, '--json', ...common(invocation)]);
}
