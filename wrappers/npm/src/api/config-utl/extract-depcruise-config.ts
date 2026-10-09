// `rulebearing/config-utl/extract-depcruise-config`: dependency-cruiser's function of the same
// name, answered by rb-config's loader (rb_cli::api::extract_depcruise_config).
//
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 22. Requirement: FR-DIST-02.
// Specification: dependency-cruiser 18.2.0, src/config-utl/extract-depcruise-config/index.mjs.

import type { IConfiguration } from '../../../types/dependency-cruiser/configuration.mjs';
import type extractDepcruiseConfigUpstream from '../../../types/dependency-cruiser/config-utl/extract-depcruise-config.mjs';
import { addon } from '../index.js';

/**
 * The configuration `pConfigFileName` names, resolved against `pBaseDirectory` as an `extends`
 * entry is, with its `extends` merged. A configuration already in `pAlreadyVisited` is circular;
 * every configuration read is added to it, as dependency-cruiser does.
 */
async function extractDepcruiseConfig(
  pConfigFileName: string,
  pAlreadyVisited: Set<string> = new Set<string>(),
  pBaseDirectory: string = process.cwd(),
): Promise<IConfiguration> {
  if (typeof pConfigFileName !== 'string') {
    throw new TypeError('extractDepcruiseConfig: the configuration file name is a string');
  }
  const { config, read } = await addon().extractDepcruiseConfigRead(
    pConfigFileName,
    [...pAlreadyVisited],
    pBaseDirectory,
  );
  for (const file of read) {
    pAlreadyVisited.add(file);
  }
  return config as IConfiguration;
}

/** Typed by dependency-cruiser's declaration: the signature test. */
const typed: typeof extractDepcruiseConfigUpstream = extractDepcruiseConfig;
export default typed;
