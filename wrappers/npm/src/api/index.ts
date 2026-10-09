// dependency-cruiser's programmatic API, answered by Rulebearing: the `rulebearing` package's main
// export, so `import { cruise } from 'rulebearing'` replaces `import { cruise } from 'dependency-cruiser'`.
//
// Architecture: docs/architecture.md#distribution. Decisions: docs/adr/0020-single-name-across-registries.md,
// docs/adr/0062-the-node-binding-reads-typescript-and-babel-configs-with-the-callers-packages.md.
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 22. Requirement: FR-DIST-02.
// Specification: docs/artifacts/dependency-cruiser-18.2.0-coverage.md#programmatic-api; the
// signatures are dependency-cruiser 18.2.0's own (types/dependency-cruiser/, vendored verbatim),
// and this module is checked against them.
//
// Each function is the native addon's (crates/rb-node, over rb_cli::api): the command line's own
// code, so `cruise()` answers what `rulebearing cruise` answers for the same options.

import type * as Upstream from '../../types/dependency-cruiser/dependency-cruiser.mjs';
import { loadAddon } from './addon.js';
import type { Native } from './addon.js';

let native: Native | undefined;

/** The addon, loaded on first use. */
export function addon(): Native {
  native ??= loadAddon();
  return native;
}

/** Cruises files, folders and globs; see dependency-cruiser's `cruise`. */
export async function cruise(
  pFileAndDirectoryArray: string[],
  pCruiseOptions?: Upstream.ICruiseOptions,
  pResolveOptions?: Partial<Upstream.IResolveOptions>,
  pTranspileOptions?: Upstream.ITranspileOptions,
): Promise<Upstream.IReporterOutput> {
  if (!Array.isArray(pFileAndDirectoryArray)) {
    throw new TypeError('cruise: the first argument is an array of files, folders or globs');
  }
  return (await addon().cruise(
    pFileAndDirectoryArray,
    pCruiseOptions,
    pResolveOptions,
    pTranspileOptions,
  )) as Upstream.IReporterOutput;
}

/** Formats a cruise result with a reporter; see dependency-cruiser's `format`. */
export async function format(
  pResult: Upstream.ICruiseResult,
  pFormatOptions: Upstream.IFormatOptions = {},
): Promise<Upstream.IReporterOutput> {
  return (await addon().format(pResult, pFormatOptions)) as Upstream.IReporterOutput;
}

/** The transpilers dependency-cruiser lists, as this build reads their files from here. */
export function getAvailableTranspilers(): Upstream.IAvailableTranspiler[] {
  return addon().getAvailableTranspilers();
}

/** The extensions dependency-cruiser lists, each available when this build reads it from here. */
export const allExtensions: Upstream.IAvailableExtension[] = addon().listExtensions();

/**
 * dependency-cruiser's default export, typed by its own declarations: the type-level test that
 * each function is assignable to dependency-cruiser's signature for it.
 */
const api: {
  cruise: typeof Upstream.cruise;
  format: typeof Upstream.format;
  allExtensions: typeof Upstream.allExtensions;
  getAvailableTranspilers: typeof Upstream.getAvailableTranspilers;
} = { cruise, format, allExtensions, getAvailableTranspilers };

export default api;
