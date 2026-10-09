// Finds and loads the native addon (`rulebearing.node`, crates/rb-node) the programmatic API calls.
//
// Architecture: docs/architecture.md#distribution. Decisions: docs/adr/0020-single-name-across-registries.md,
// docs/adr/0062-the-node-binding-reads-typescript-and-babel-configs-with-the-callers-packages.md.
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 22. Requirement: FR-DIST-02.
//
// The addon ships in the same platform package as the binary (`rulebearing-cli-<platform>`),
// beside `bin/`, so one optional dependency per platform carries both. `RULEBEARING_ADDON` names
// an addon to load instead, as `RULEBEARING_BINARY` names a binary for the launcher.

import { existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { describeHost, detectHost, ownVersion, resolvePackageJson } from '../launcher.js';
import type { Host, Resolution } from '../launcher.js';
import { PLATFORM_PACKAGES, platformPackageFor } from '../platforms.js';

/** Environment variable naming an addon to load instead of the platform package's. */
export const ADDON_OVERRIDE = 'RULEBEARING_ADDON';

/** The addon's file name inside a platform package. */
export const ADDON_FILE = 'rulebearing.node';

/** A reporter's answer, as the addon returns it. */
export interface NativeReporterOutput {
  readonly output: unknown;
  readonly exitCode: number;
}

/** What the addon exports (crates/rb-node/src/lib.rs). */
export interface Native {
  cruise(
    files: string[],
    options?: unknown,
    resolveOptions?: unknown,
    transpileOptions?: unknown,
  ): Promise<NativeReporterOutput>;
  format(result: unknown, options?: unknown): Promise<NativeReporterOutput>;
  extractDepcruiseConfigRead(
    fileName: string,
    alreadyVisited?: string[],
    baseDirectory?: string,
  ): Promise<{ readonly config: unknown; readonly read: string[] }>;
  extractWebpackResolveConfigRead(
    fileName: string,
    env?: unknown,
    args?: unknown,
  ): Promise<{ readonly value: unknown }>;
  getAvailableTranspilers(): {
    name: string;
    version: string;
    available: boolean;
    currentVersion: string;
  }[];
  listExtensions(): { extension: string; available: boolean }[];
}

/**
 * Finds the addon: the `RULEBEARING_ADDON` override when set, otherwise `rulebearing.node` in the
 * platform package for `host`.
 */
export function resolveAddon(
  host: Host,
  env: NodeJS.ProcessEnv,
  resolve: (name: string) => string = resolvePackageJson,
  version: () => string = ownVersion,
): Resolution {
  const override = env[ADDON_OVERRIDE];
  if (override !== undefined && override !== '') {
    return existsSync(override)
      ? { kind: 'binary', path: override }
      : {
          kind: 'missing',
          message: `rulebearing: ${ADDON_OVERRIDE} is set to ${override}, which does not exist; unset it or point it at a rulebearing.node built from crates/rb-node.`,
        };
  }
  const detected = describeHost(host);
  const pkg = platformPackageFor(host.platform, host.arch, host.libc);
  if (pkg === undefined) {
    const supported = PLATFORM_PACKAGES.map((p) => p.label).join(', ');
    return {
      kind: 'missing',
      message: `rulebearing: no prebuilt addon for ${detected}; the npm package covers ${supported}. Build crates/rb-node from source (https://github.com/benbahrenburg/rulebearing) and set ${ADDON_OVERRIDE} to it.`,
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
  const path = join(dirname(manifest), ADDON_FILE);
  if (!existsSync(path)) {
    return {
      kind: 'missing',
      message: `rulebearing: ${pkg.name} is installed but ${path} is missing (detected ${detected}); reinstall ${pkg.name}.`,
    };
  }
  return { kind: 'binary', path };
}

/** Loads the addon for this host, or throws with the reason there is none. */
export function loadAddon(
  resolution: Resolution = resolveAddon(detectHost(), process.env),
  load: (path: string) => unknown = createRequire(import.meta.url),
): Native {
  if (resolution.kind === 'missing') {
    throw new Error(resolution.message);
  }
  return load(resolution.path) as Native;
}
