// `rulebearing/config-utl/extract-babel-config`: dependency-cruiser's function of the same name,
// which is Babel's own `loadOptionsSync` over the file, run with the caller's Babel.
//
// Decision: docs/adr/0062-the-node-binding-reads-typescript-and-babel-configs-with-the-callers-packages.md.
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 22. Requirement: FR-DIST-02.
// Specification: dependency-cruiser 18.2.0, src/config-utl/extract-babel-config.mjs, ported line for
// line; JSON5 is read with the `json5` Babel itself depends on, so this package adds no dependency.
//
// `cruise()` accepts the result as `transpileOptions.babelConfig` by the file it records
// (`filename`) and reads that file itself (rb_cli::api).

import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { extname, isAbsolute, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import type extractBabelConfigUpstream from '../../../types/dependency-cruiser/config-utl/extract-babel-config.mjs';
import { requireHere, tryRequire } from './transpilers.js';

/** The part of Babel's API this function calls. */
export interface BabelApi {
  loadOptionsSync(options: object): object | null;
}

/** dependency-cruiser's `makeAbsolute`. */
function makeAbsolute(pFilename: string): string {
  return isAbsolute(pFilename) ? pFilename : join(process.cwd(), pFilename);
}

async function getJSConfig(pBabelConfigFileName: string): Promise<object> {
  let lReturnValue: unknown;
  try {
    const lModule = (await import(pathToFileURL(makeAbsolute(pBabelConfigFileName)).href)) as {
      default: unknown;
    };
    lReturnValue = lModule.default;
  } catch (pError) {
    throw new Error(
      `${
        `Encountered an error while parsing babel config '${pBabelConfigFileName}':` +
        `\n\n          ${String(pError)}`
      }\n\n         At this time dependency-cruiser only supports babel configurations\n         in either commonjs or json5.\n`,
      { cause: pError },
    );
  }
  if (typeof lReturnValue === 'function') {
    throw new TypeError(
      `The babel config '${pBabelConfigFileName}' returns a function. At this time\n` +
        `         dependency-cruiser doesn't support that yet.`,
    );
  }
  return lReturnValue as object;
}

/** `json5` from where Babel is installed: Babel depends on it. */
function json5(): { parse(text: string): unknown } {
  const babelEntry = requireHere.resolve('@babel/core');
  return createRequire(babelEntry)('json5') as { parse(text: string): unknown };
}

async function getJSON5Config(pBabelConfigFileName: string): Promise<object> {
  let lReturnValue: unknown;
  try {
    lReturnValue = json5().parse(await readFile(pBabelConfigFileName, 'utf8'));
  } catch (pError) {
    throw new Error(
      `Encountered an error while parsing the babel config '${pBabelConfigFileName}':` +
        `\n\n          ${String(pError)}\n`,
      { cause: pError },
    );
  }
  if (pBabelConfigFileName.endsWith('package.json')) {
    lReturnValue = (lReturnValue as { babel?: unknown } | null)?.babel ?? {};
  }
  return lReturnValue as object;
}

async function getConfig(pBabelConfigFileName: string): Promise<object> {
  const lExtensionToParseFunction = new Map<string, (pFileName: string) => Promise<object>>([
    ['.js', getJSConfig],
    ['.cjs', getJSConfig],
    ['.mjs', getJSConfig],
    ['', getJSON5Config],
    ['.json', getJSON5Config],
    ['.json5', getJSON5Config],
  ]);
  const lExtension = extname(pBabelConfigFileName);
  const parse = lExtensionToParseFunction.get(lExtension);
  if (parse === undefined) {
    throw new Error(
      `The babel config '${pBabelConfigFileName}' is in a format ('${lExtension}')\n         dependency-cruiser doesn't support yet.\n`,
    );
  }
  return await parse(pBabelConfigFileName);
}

/** The Babel options `pBabelConfigFileName` holds with `babel` given, or `{}` without it. */
export async function extractWith(
  babel: BabelApi | undefined,
  pBabelConfigFileName: string,
): Promise<object> {
  if (babel === undefined) {
    return {};
  }
  const lConfig: { presets?: unknown; filename: string } = {
    ...(await getConfig(pBabelConfigFileName)),
    // under some circumstances babel (and/ or its plugins) needs
    // a filename to go with the config - so we pass it
    filename: pBabelConfigFileName,
  };
  return {
    ...babel.loadOptionsSync(lConfig),
    // according to the babel documentation a config parsed & expanded through
    // loadOptions can be passed to the parser. With some plugins/ presets
    // this does not seem to be true anymore, though
    ...(lConfig.presets ? { presets: lConfig.presets } : {}),
  };
}

/**
 * The Babel options `pBabelConfigFileName` holds, loaded by the caller's Babel; `{}` when
 * `@babel/core` is not installed in a supported version.
 */
async function extractBabelConfig(pBabelConfigFileName: string): Promise<object> {
  return await extractWith(tryRequire('@babel/core') as BabelApi | undefined, pBabelConfigFileName);
}

/** Typed by dependency-cruiser's declaration: the signature test. */
const typed: typeof extractBabelConfigUpstream = extractBabelConfig;
export default typed;
