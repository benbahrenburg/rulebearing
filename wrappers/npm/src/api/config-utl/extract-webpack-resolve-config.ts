// `rulebearing/config-utl/extract-webpack-resolve-config`: dependency-cruiser's function of the
// same name, answered by rb-config's sandboxed webpack evaluation (rb_cli::api::extract_webpack_resolve_config).
//
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 22. Requirement: FR-DIST-02.
// Specification: dependency-cruiser 18.2.0, src/config-utl/extract-webpack-resolve-config.mjs.

import type extractWebpackResolveConfigUpstream from '../../../types/dependency-cruiser/config-utl/extract-webpack-resolve-config.mjs';
import { addon } from '../index.js';

/**
 * The `resolve` block of the webpack configuration `pWebpackConfigFilename` (relative to the
 * working directory), `{}` when it has none. A configuration that exports a function is called
 * with `pEnvironment` and `pArguments`; one that exports an array gives its first element's.
 */
async function extractWebpackResolveConfig(
  pWebpackConfigFilename: string,
  pEnvironment?: Record<string, unknown>,
  pArguments?: Record<string, unknown> | string,
): Promise<object> {
  if (typeof pWebpackConfigFilename !== 'string') {
    throw new TypeError(
      'extractWebpackResolveConfig: the webpack configuration file name is a string',
    );
  }
  const { value } = await addon().extractWebpackResolveConfigRead(
    pWebpackConfigFilename,
    pEnvironment,
    pArguments,
  );
  return value as object;
}

/** Typed by dependency-cruiser's declaration: the signature test. */
const typed: typeof extractWebpackResolveConfigUpstream = extractWebpackResolveConfig;
export default typed;
