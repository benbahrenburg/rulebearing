// The `rulebearing` command on npm: find the platform binary and run it, nothing else.
//
// Architecture: docs/architecture.md#distribution. Decision: docs/adr/0020-single-name-across-registries.md.
// Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 19. Requirement: FR-DIST-01.
// Exit codes: docs/adr/0008-exit-code-contract.md (2 is the "untrustworthy run" code, used here when
// no binary can be found or started).
//
// There is no postinstall: npm installs only the platform package whose `os`, `cpu` and `libc`
// match, and this launcher resolves it at run time. It never re-implements a subcommand; every
// argument goes to the binary unchanged and the binary's exit code (or signal) is the result.

import { spawn } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { constants } from 'node:os';
import { dirname, join } from 'node:path';
import { PLATFORM_PACKAGES, platformPackageFor } from './platforms.js';
import type { Libc } from './platforms.js';

/** The exit code for a run that could not start the binary (ADR-0008). */
export const EXIT_UNTRUSTWORTHY = 2;

/** Environment variable naming a binary to run instead of the platform package's. */
export const BINARY_OVERRIDE = 'RULEBEARING_BINARY';

/** What the launcher knows about the machine it runs on. */
export interface Host {
  readonly platform: string;
  readonly arch: string;
  /** Linux only; `undefined` elsewhere. */
  readonly libc: Libc | undefined;
}

/** The binary to run, or the one-line reason there is none. */
export type Resolution =
  | { readonly kind: 'binary'; readonly path: string }
  | { readonly kind: 'missing'; readonly message: string };

/** How the binary ended: an exit code, or the signal that stopped it. */
export type Outcome = { readonly code: number } | { readonly signal: NodeJS.Signals };

/**
 * glibc or musl, from Node's diagnostic report: glibc builds of Node record
 * `header.glibcVersionRuntime`, musl builds do not.
 */
export function detectLibc(getReport: () => unknown = () => process.report.getReport()): Libc {
  const report = getReport();
  if (typeof report === 'object' && report !== null && 'header' in report) {
    const header = report.header;
    if (
      typeof header === 'object' &&
      header !== null &&
      'glibcVersionRuntime' in header &&
      typeof header.glibcVersionRuntime === 'string'
    ) {
      return 'glibc';
    }
  }
  return 'musl';
}

/** The running host. The C library is detected on Linux only. */
export function detectHost(
  platform: string = process.platform,
  arch: string = process.arch,
  libc: () => Libc = detectLibc,
): Host {
  return { platform, arch, libc: platform === 'linux' ? libc() : undefined };
}

/** `darwin arm64`, or `linux x64 musl`. */
export function describeHost(host: Host): string {
  return [host.platform, host.arch, host.libc].filter((part) => part !== undefined).join(' ');
}

/** This package's own version, which is also every platform package's version. */
export function ownVersion(): string {
  const text = readFileSync(new URL('../package.json', import.meta.url), 'utf8');
  const manifest: unknown = JSON.parse(text);
  if (typeof manifest === 'object' && manifest !== null && 'version' in manifest) {
    const { version } = manifest;
    if (typeof version === 'string') {
      return version;
    }
  }
  return 'latest';
}

/** Resolves `<name>/package.json` from this package's location, as Node would for an import. */
export function resolvePackageJson(name: string): string {
  return createRequire(import.meta.url).resolve(`${name}/package.json`);
}

/**
 * Finds the binary: the `RULEBEARING_BINARY` override when set, otherwise `bin/<binary>` inside
 * the platform package for `host`.
 */
export function resolveBinary(
  host: Host,
  env: NodeJS.ProcessEnv,
  resolve: (name: string) => string = resolvePackageJson,
  version: () => string = ownVersion,
): Resolution {
  const override = env[BINARY_OVERRIDE];
  if (override !== undefined && override !== '') {
    return existsSync(override)
      ? { kind: 'binary', path: override }
      : {
          kind: 'missing',
          message: `rulebearing: ${BINARY_OVERRIDE} is set to ${override}, which does not exist; unset it or point it at a rulebearing binary.`,
        };
  }
  const detected = describeHost(host);
  const pkg = platformPackageFor(host.platform, host.arch, host.libc);
  if (pkg === undefined) {
    const supported = PLATFORM_PACKAGES.map((p) => p.label).join(', ');
    return {
      kind: 'missing',
      message: `rulebearing: no prebuilt binary for ${detected}; the npm package covers ${supported}. Build from source (https://github.com/benbahrenburg/rulebearing) and set ${BINARY_OVERRIDE} to the binary.`,
    };
  }
  let manifest: string;
  try {
    manifest = resolve(pkg.name);
  } catch {
    return {
      kind: 'missing',
      message: `rulebearing: the platform package ${pkg.name} is not installed (detected ${detected}). Install it with \`npm install ${pkg.name}@${version()}\`, or reinstall rulebearing without --omit=optional or --no-optional.`,
    };
  }
  const path = join(dirname(manifest), 'bin', pkg.binary);
  if (!existsSync(path)) {
    return {
      kind: 'missing',
      message: `rulebearing: ${pkg.name} is installed but ${path} is missing (detected ${detected}); reinstall ${pkg.name}.`,
    };
  }
  return { kind: 'binary', path };
}

const FORWARDED: readonly NodeJS.Signals[] = ['SIGINT', 'SIGTERM', 'SIGHUP'];

/**
 * Runs the binary with inherited stdio and the arguments unchanged. Signals sent to the launcher
 * are forwarded while the binary runs. A binary that cannot be started is exit 2.
 */
export function runBinary(path: string, args: readonly string[]): Promise<Outcome> {
  return new Promise((resolve) => {
    const child = spawn(path, args, { stdio: 'inherit', windowsHide: true });
    const forward = (signal: NodeJS.Signals): void => {
      child.kill(signal);
    };
    for (const signal of FORWARDED) {
      process.on(signal, forward);
    }
    const settle = (outcome: Outcome): void => {
      for (const signal of FORWARDED) {
        process.off(signal, forward);
      }
      resolve(outcome);
    };
    child.on('error', (error) => {
      console.error(`rulebearing: could not start ${path}: ${error.message}`);
      settle({ code: EXIT_UNTRUSTWORTHY });
    });
    child.on('exit', (code, signal) => {
      settle(signal === null ? { code: code ?? EXIT_UNTRUSTWORTHY } : { signal });
    });
  });
}

/** The side effects `launch` has on the launcher process, replaceable in tests. */
export interface LaunchEffects {
  readonly setExitCode: (code: number) => void;
  readonly kill: (signal: NodeJS.Signals) => void;
}

const processEffects: LaunchEffects = {
  setExitCode: (code) => {
    process.exitCode = code;
  },
  kill: (signal) => {
    // Re-raise the binary's signal on ourselves so the caller sees the same termination; if
    // the signal is ignored here, fall back to the shell convention of 128 + its number.
    process.kill(process.pid, signal);
    process.exitCode = 128 + constants.signals[signal];
  },
};

/** Resolves and runs the binary for this host; the entry point of `bin/rulebearing.js`. */
export async function launch(
  args: readonly string[] = process.argv.slice(2),
  env: NodeJS.ProcessEnv = process.env,
  host: Host = detectHost(),
  resolve: (name: string) => string = resolvePackageJson,
  effects: LaunchEffects = processEffects,
): Promise<void> {
  const resolution = resolveBinary(host, env, resolve);
  if (resolution.kind === 'missing') {
    console.error(resolution.message);
    effects.setExitCode(EXIT_UNTRUSTWORTHY);
    return;
  }
  const outcome = await runBinary(resolution.path, args);
  if ('signal' in outcome) {
    effects.kill(outcome.signal);
  } else {
    effects.setExitCode(outcome.code);
  }
}
