// `rulebearing/config-utl/extract-ts-config`: dependency-cruiser's function of the same name, which
// is TypeScript's own `parseJsonConfigFileContent` over the file, run with the caller's TypeScript.
//
// Decision: docs/adr/0062-the-node-binding-reads-typescript-and-babel-configs-with-the-callers-packages.md.
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 22. Requirement: FR-DIST-02.
// Specification: dependency-cruiser 18.2.0, src/config-utl/extract-ts-config.mjs, ported line for line.
//
// `cruise()` accepts the result as `transpileOptions.tsConfig` by the file it records
// (`options.configFilePath`) and reads that file itself (rb_cli::api).

import { dirname, resolve } from 'node:path';
import type extractTSConfigUpstream from '../../../types/dependency-cruiser/config-utl/extract-ts-config.mjs';
import { tryRequire } from './transpilers.js';

/** The part of TypeScript's API this function calls. */
export interface TypeScriptApi {
  readonly sys: {
    readonly useCaseSensitiveFileNames?: boolean;
    readFile(path: string): string | undefined;
  };
  readConfigFile(
    fileName: string,
    readFile: (path: string) => string | undefined,
  ): { config?: unknown; error?: unknown };
  parseJsonConfigFileContent(
    json: unknown,
    host: unknown,
    basePath: string,
    existingOptions: object,
    configFileName: string,
  ): { errors: unknown[] };
  formatDiagnostics(diagnostics: readonly unknown[], host: unknown): string;
}

/** dependency-cruiser's `FORMAT_DIAGNOSTICS_HOST`. */
function diagnosticsHost(typescript: TypeScriptApi): object {
  return {
    getCanonicalFileName(pFileName: string): string {
      return typescript.sys.useCaseSensitiveFileNames === true
        ? pFileName
        : pFileName.toLowerCase();
    },
    getCurrentDirectory(): string {
      return process.cwd();
    },
    getNewLine(): string {
      return '\n';
    },
  };
}

/** The flattened TypeScript configuration with `typescript` given, or `{}` without it. */
export function extractWith(
  typescript: TypeScriptApi | undefined,
  pTSConfigFileName: string,
): object {
  if (typescript === undefined) {
    return {};
  }
  const config = typescript.readConfigFile(pTSConfigFileName, (path) =>
    typescript.sys.readFile(path),
  );
  if (config.error !== undefined) {
    throw new TypeError(typescript.formatDiagnostics([config.error], diagnosticsHost(typescript)));
  }
  const parsed = typescript.parseJsonConfigFileContent(
    config.config,
    typescript.sys,
    dirname(resolve(pTSConfigFileName)),
    {},
    pTSConfigFileName,
  );
  if (parsed.errors.length > 0) {
    throw new Error(typescript.formatDiagnostics(parsed.errors, diagnosticsHost(typescript)));
  }
  return parsed;
}

/**
 * The TypeScript configuration `pTSConfigFileName` names, flattened by the caller's TypeScript (its
 * `extends` followed); `{}` when TypeScript is not installed in a supported version.
 */
function extractTSConfig(pTSConfigFileName: string): ReturnType<typeof extractTSConfigUpstream> {
  return extractWith(
    tryRequire('typescript') as TypeScriptApi | undefined,
    pTSConfigFileName,
  ) as ReturnType<typeof extractTSConfigUpstream>;
}

/** Typed by dependency-cruiser's declaration: the signature test. */
const typed: typeof extractTSConfigUpstream = extractTSConfig;
export default typed;
