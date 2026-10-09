// The caller's own TypeScript and Babel, found as dependency-cruiser finds them (its
// src/utl/try-import.mjs): resolved from this package, refused outside the range dependency-cruiser
// 18.2.0 supports, `undefined` when absent.
//
// Decision: docs/adr/0062-the-node-binding-reads-typescript-and-babel-configs-with-the-callers-packages.md.
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 22.

import { createRequire } from 'node:module';

/** The ranges dependency-cruiser 18.2.0 supports (its src/meta.cjs), as [lowest major, first major past]. */
export const SUPPORTED: Readonly<Record<'typescript' | '@babel/core', readonly [number, number]>> =
  {
    typescript: [2, 7],
    '@babel/core': [7, 8],
  };

/** `require` from this package's location, as dependency-cruiser's own `import` resolves. */
export const requireHere = createRequire(import.meta.url);

/** The installed version's major, or `undefined` when it cannot be read. */
export function majorOf(version: unknown): number | undefined {
  if (typeof version !== 'string') {
    return undefined;
  }
  const major = Number.parseInt(version, 10);
  return Number.isNaN(major) ? undefined : major;
}

/** Whether `version` is in `name`'s supported range; an unreadable version is let through, as semver's `coerce` failing is. */
export function supported(name: keyof typeof SUPPORTED, version: unknown): boolean {
  const major = majorOf(version);
  if (major === undefined) {
    return true;
  }
  const [lowest, past] = SUPPORTED[name];
  return major >= lowest && major < past;
}

/** `name`'s module when it is installed in a supported version, else `undefined`. */
export function tryRequire(
  name: keyof typeof SUPPORTED,
  load: (id: string) => unknown = requireHere,
): unknown {
  try {
    const manifest = load(`${name}/package.json`) as { version?: unknown };
    if (!supported(name, manifest.version)) {
      return undefined;
    }
    return load(name);
  } catch {
    return undefined;
  }
}
