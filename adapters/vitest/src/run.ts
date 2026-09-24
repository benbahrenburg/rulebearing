// Runs `rulebearing cruise --output-type json` and reads what it wrote.
//
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md § 2.14, Step 14 (2H).
// Decisions: docs/adr/0008-exit-code-contract.md (exit 1 and 2 still write the JSON);
// docs/adr/0010-crate-layout-and-extractor-boundary.md rule 4 (the binary evaluates; the adapter
// reports). Requirement: FR-DIST-03.
//
// The binary is the one the `rulebearing` package runs: this module ships inside that package
// (wrappers/npm, as `rulebearing/vitest`), so it starts the package's own launcher,
// bin/rulebearing.js, which finds the platform binary. RULEBEARING_BINARY, or the `binary` option,
// names a binary to run instead, as it does for the launcher.

import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';

/** Environment variable naming a binary to run instead of the package's. */
export const BINARY_OVERRIDE = 'RULEBEARING_BINARY';

/** How to run the binary. */
export interface RunOptions {
  /** The binary; unset, `RULEBEARING_BINARY`, then the `rulebearing` package's launcher. */
  readonly binary?: string;
  /** `--config`; unset, the binary finds the configuration. */
  readonly config?: string;
  /** `--graph`: read a graph document instead of extracting. */
  readonly graph?: string;
  /** Further arguments, such as the files or directories to cruise. */
  readonly args?: readonly string[];
  /** The directory to run in; the process's working directory when unset. */
  readonly cwd?: string;
}

/** The run wrote no result to read; the message names the command, exit code and stderr. */
export class CruiseError extends Error {
  override readonly name = 'CruiseError';
}

/** A command: the program and its leading arguments. */
export type Command = readonly [string, ...string[]];

/** Resolves `<name>/package.json` from this module's location, as Node would for an import. */
export function resolvePackageJson(name: string): string {
  return createRequire(import.meta.url).resolve(`${name}/package.json`);
}

/**
 * The program to start: the option, else `RULEBEARING_BINARY`, else Node running the
 * `rulebearing` package's launcher.
 */
export function resolveCommand(
  binary: string | undefined,
  env: NodeJS.ProcessEnv = process.env,
  resolve: (name: string) => string = resolvePackageJson,
): Command {
  if (binary !== undefined && binary !== '') {
    return [binary];
  }
  const override = env[BINARY_OVERRIDE];
  if (override !== undefined && override !== '') {
    return [override];
  }
  let manifest: string;
  try {
    manifest = resolve('rulebearing');
  } catch {
    throw new CruiseError(
      `rulebearing/vitest: the rulebearing package cannot be resolved from ${import.meta.url}; install rulebearing, or set ${BINARY_OVERRIDE} to a rulebearing binary.`,
    );
  }
  return [process.execPath, join(dirname(manifest), 'bin', 'rulebearing.js')];
}

/** The arguments after the program: `cruise --output-type json --no-progress ...`. */
export function cruiseArguments(options: RunOptions): string[] {
  return [
    'cruise',
    '--output-type',
    'json',
    '--no-progress',
    ...(options.config === undefined ? [] : ['--config', options.config]),
    ...(options.graph === undefined ? [] : ['--graph', options.graph]),
    ...(options.args ?? []),
  ];
}

/** Enough for the JSON of a large monorepo; spawnSync's default is 1 MiB. */
const MAX_BUFFER = 1024 * 1024 * 1024;

/**
 * Runs the binary and parses its JSON. A run that finds violations (exit 1) or cannot be trusted
 * (exit 2, a vacuous rule) still writes its result; only a run that wrote none is an error.
 */
export function cruise(options: RunOptions = {}, env: NodeJS.ProcessEnv = process.env): unknown {
  const [program, ...leading] = resolveCommand(options.binary, env);
  const args = [...leading, ...cruiseArguments(options)];
  const shown = [program, ...args].join(' ');
  const completed = spawnSync(program, args, {
    cwd: options.cwd ?? process.cwd(),
    env,
    encoding: 'utf8',
    maxBuffer: MAX_BUFFER,
    windowsHide: true,
  });
  if (completed.error !== undefined) {
    throw new CruiseError(`\`${shown}\` could not start: ${completed.error.message}`);
  }
  let result: unknown;
  try {
    result = JSON.parse(completed.stdout);
  } catch {
    result = undefined;
  }
  if (typeof result !== 'object' || result === null || !('summary' in result)) {
    const status =
      completed.status === null ? `signal ${String(completed.signal)}` : String(completed.status);
    throw new CruiseError(
      `\`${shown}\` exited ${status} without a result:\n${completed.stderr.trim()}`,
    );
  }
  return result;
}
