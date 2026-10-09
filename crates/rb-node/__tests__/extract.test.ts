// The four configuration extractors under dependency-cruiser 18.2.0's own specs
// (test/config-utl/*.spec.mjs), ported to vitest over its own fixtures (fixtures/PROVENANCE.md).
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 22 ("each extractor
// function on its fixture"). Decision: docs/adr/0062-the-node-binding-reads-typescript-and-babel-configs-with-the-callers-packages.md.
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import extractBabelConfig, {
  extractWith as babelWith,
} from '../../../wrappers/npm/src/api/config-utl/extract-babel-config.js';
import extractDepcruiseConfig from '../../../wrappers/npm/src/api/config-utl/extract-depcruise-config.js';
import extractTSConfig, {
  extractWith as typescriptWith,
} from '../../../wrappers/npm/src/api/config-utl/extract-ts-config.js';
import extractWebpackResolveConfig from '../../../wrappers/npm/src/api/config-utl/extract-webpack-resolve-config.js';
import {
  majorOf,
  supported,
  tryRequire,
} from '../../../wrappers/npm/src/api/config-utl/transpilers.js';

const FIXTURES = fileURLToPath(new URL('./fixtures/', import.meta.url));

/** A fixture's absolute path, as the specs' `getFullPath` gives it. */
function fixture(path: string): string {
  return join(FIXTURES, path);
}

/** dependency-cruiser's `pathToPosix`. */
function posix(path: string): string {
  return path.split('\\').join('/');
}

function json(path: string): unknown {
  return JSON.parse(readFileSync(fixture(path), 'utf8'));
}

describe('extractDepcruiseConfig (test/config-utl/extract-depcruise-config/index.spec.mjs)', () => {
  it('a rule set without an extends returns just that rule set', async () => {
    expect(
      await extractDepcruiseConfig(fixture('depcruise/rules.sub-not-allowed-error.json')),
    ).toEqual(json('depcruise/rules.sub-not-allowed-error.json'));
  });

  it('a rule set with an extends returns that rule set, extending the mentioned base', async () => {
    expect(await extractDepcruiseConfig(fixture('depcruise/extends/extending.json'))).toEqual(
      json('depcruise/extends/merged.json'),
    );
  });

  it('a rule set with an extends array (0 members) returns that rule set', async () => {
    expect(
      await extractDepcruiseConfig(
        fixture('depcruise/extends/extending-array-with-zero-members.json'),
      ),
    ).toEqual({ forbidden: [{ name: 'rule-from-the-base', from: {}, to: {} }] });
  });

  it('a rule set with an extends array (1 member) returns that rule set, extending the mentioned base', async () => {
    expect(
      await extractDepcruiseConfig(
        fixture('depcruise/extends/extending-array-with-one-member.json'),
      ),
    ).toEqual(json('depcruise/extends/merged-array-1.json'));
  });

  it('a rule set with an extends array (>1 member) returns that rule set, extending the mentioned bases', async () => {
    expect(
      await extractDepcruiseConfig(
        fixture('depcruise/extends/extending-array-with-two-members.json'),
      ),
    ).toEqual(json('depcruise/extends/merged-array-2.json'));
  });

  it('a rule set with an extends from node_modules gets merged properly as well', async () => {
    expect(
      await extractDepcruiseConfig(fixture('depcruise/extends/extending-from-node-modules.json')),
    ).toEqual({
      allowed: [{ from: { path: 'src' }, to: { path: 'src' } }],
      allowedSeverity: 'warn',
      options: { doNotFollow: 'node_modules' },
    });
  });

  it('borks on a circular extends (1 step), naming both files', async () => {
    const one = fixture('depcruise/extends/circular-one.js');
    const two = fixture('depcruise/extends/circular-two.js');
    const thrown = await extractDepcruiseConfig(one).then(
      () => '',
      (e: unknown) => String(e),
    );
    expect(thrown).toMatch(/circular/);
    expect(thrown).toContain(one);
    expect(thrown).toContain(two);
  });

  it('adds every configuration read to alreadyVisited, resolves against the base directory, and refuses one already visited', async () => {
    const visited = new Set<string>();
    await extractDepcruiseConfig('./extending.json', visited, fixture('depcruise/extends'));
    expect([...visited]).toEqual([
      fixture('depcruise/extends/extending.json'),
      fixture('depcruise/extends/base-for-extends.js'),
      fixture('depcruise/extends/base-for-base.js'),
    ]);
    await expect(
      extractDepcruiseConfig('./base-for-base.js', visited, fixture('depcruise/extends')),
    ).rejects.toThrow(/circular/);
    await expect(extractDepcruiseConfig(undefined as never)).rejects.toThrow(TypeError);
    await expect(extractDepcruiseConfig('./no-such-config.json')).rejects.toThrow();
  });
});

describe('extractWebpackResolveConfig (test/config-utl/extract-webpack-resolve-config-native.spec.mjs)', () => {
  const ALIASSY = { alias: { configSpullenAlias: './configspullen' }, bustTheCache: true };
  const MERLIN = { alias: { config: 'src/config', magic$: 'src/merlin/browserify/magic' } };

  it('throws when no config file name is passed, or one that does not exist', async () => {
    await expect(extractWebpackResolveConfig(undefined as never)).rejects.toThrow();
    await expect(extractWebpackResolveConfig('config-does-not-exist')).rejects.toThrow();
  });

  it('throws when a config file is passed that does not contain valid javascript', async () => {
    await expect(
      extractWebpackResolveConfig(fixture('webpack/invalid.config.js')),
    ).rejects.toThrow();
  });

  it("returns an empty object when a config file is passed without a 'resolve' section", async () => {
    expect(await extractWebpackResolveConfig(fixture('webpack/noresolve.config.js'))).toEqual({});
  });

  it("returns the resolve section of the webpack config if there's any (.js and .mjs)", async () => {
    expect(await extractWebpackResolveConfig(fixture('webpack/hasaresolve.config.js'))).toEqual(
      MERLIN,
    );
    expect(await extractWebpackResolveConfig(fixture('webpack/webpack.config.mjs'))).toEqual(
      MERLIN,
    );
  });

  it('returns the resolve section for the environment asked for', async () => {
    expect(
      await extractWebpackResolveConfig(fixture('webpack/hastwoseparateresolves.config.js'), {
        production: true,
      }),
    ).toEqual(MERLIN);
    expect(
      await extractWebpackResolveConfig(fixture('webpack/hastwoseparateresolves.config.js'), {
        develop: true,
      }),
    ).toEqual({ alias: { config: 'src/dev-config', magic$: 'src/merlin/browserify/hipsterlib' } });
  });

  it('returns the resolve section of a function, an array, and a function in an array', async () => {
    for (const file of [
      'webpack.functionexport.config.js',
      'webpack.arrayexport.config.js',
      'webpack.functionarrayexport.config.js',
    ]) {
      expect(await extractWebpackResolveConfig(fixture(`webpack/aliassy/${file}`))).toEqual(
        ALIASSY,
      );
    }
  });
});

describe('extractTSConfig (test/config-utl/extract-ts-config.spec.mjs)', () => {
  const options = (path: string): unknown =>
    (extractTSConfig(fixture(`typescript/${path}`)) as { options: unknown }).options;
  const configFilePath = (path: string): string => posix(fixture(`typescript/${path}`));

  it('throws when no config file name is passed, one that does not exist, or one that is not json', () => {
    expect(() => extractTSConfig(undefined as never)).toThrow();
    expect(() => extractTSConfig('config-does-not-exist')).toThrow();
    expect(() => extractTSConfig(fixture('typescript/tsconfig.invalid.json'))).toThrow();
  });

  it('returns an empty object when an empty config file, with or without comments, is passed', () => {
    for (const file of ['tsconfig.empty.json', 'tsconfig.withcomments.json']) {
      expect(options(file)).toEqual({ configFilePath: configFilePath(file) });
    }
  });

  it("returns an object with a bunch of options when the default ('--init') config file is passed", () => {
    expect(options('tsconfig.asgeneratedbydefault.json')).toEqual({
      configFilePath: configFilePath('tsconfig.asgeneratedbydefault.json'),
      esModuleInterop: true,
      module: 1,
      strict: true,
      target: 1,
    });
  });

  it('throws on an extends to a non-existing file, and on a circular reference', () => {
    expect(() => extractTSConfig(fixture('typescript/tsconfig.extendsnonexisting.json'))).toThrow();
    expect(() => extractTSConfig(fixture('typescript/tsconfig.circular.json'))).toThrow(
      /error TS18000: Circularity detected while resolving configuration/,
    );
  });

  it("returns an empty object (even no 'extend') when a config with an extend to an empty base is passed", () => {
    expect(options('tsconfig.simpleextends.json')).toEqual({
      configFilePath: configFilePath('tsconfig.simpleextends.json'),
    });
  });

  it('returns an object with properties from base, extends & overrides from extends - non-compilerOptions', () => {
    const wildcardDirectories: Record<string, number> = {};
    wildcardDirectories[posix(fixture('typescript/override from extends here'))] = 1;
    expect(extractTSConfig(fixture('typescript/tsconfig.noncompileroptionsextends.json'))).toEqual({
      options: { configFilePath: configFilePath('tsconfig.noncompileroptionsextends.json') },
      fileNames: [posix(fixture('typescript/dummysrc.ts'))],
      projectReferences: undefined,
      typeAcquisition: { enable: false, include: [], exclude: [] },
      raw: {
        extends: './tsconfig.noncompileroptionsbase.json',
        exclude: ['only in the extends'],
        include: ['override from extends here'],
        compileOnSave: false,
        files: ['./dummysrc.ts'],
      },
      watchOptions: undefined,
      errors: [],
      wildcardDirectories,
      compileOnSave: false,
    });
  });

  it('returns an object with properties from base, extends & overrides from extends - compilerOptions', () => {
    expect(options('tsconfig.compileroptionsextends.json')).toEqual({
      configFilePath: configFilePath('tsconfig.compileroptionsextends.json'),
      allowJs: true,
      allowUnreachableCode: false,
      rootDirs: ['foo', 'bar', 'baz'].map((d) => posix(fixture(`typescript/${d}`))),
    });
  });

  it('returns an object with properties from base, extends compilerOptions.lib array', () => {
    expect(options('tsconfig.compileroptionsextendslib.json')).toEqual({
      configFilePath: configFilePath('tsconfig.compileroptionsextendslib.json'),
      lib: ['lib.dom.iterable.d.ts'],
    });
  });

  it('returns {} without TypeScript, as dependency-cruiser does', () => {
    expect(typescriptWith(undefined, fixture('typescript/tsconfig.empty.json'))).toEqual({});
  });
});

describe('extractBabelConfig (test/config-utl/extract-babel-config.spec.mjs)', () => {
  const DEFAULT_EMPTY_BABEL_OPTIONS_OBJECT = {
    babelrc: false,
    cloneInputAst: true,
    configFile: false,
    passPerPreset: false,
    // Babel's own rule; vitest sets NODE_ENV to `test`, where upstream's mocha leaves it unset.
    envName: process.env.BABEL_ENV ?? process.env.NODE_ENV ?? 'development',
    cwd: process.cwd(),
    root: process.cwd(),
    rootMode: 'root',
    plugins: [],
    presets: [],
    assumptions: {},
    browserslistConfigFile: false,
    targets: {},
  };

  /** The options without `filename`, which must name the file. */
  async function withoutFilename(path: string): Promise<Record<string, unknown>> {
    const config = (await extractBabelConfig(fixture(path))) as Record<string, unknown>;
    expect(posix(String(config.filename))).toContain(posix(path));
    const copy = structuredClone(config);
    delete copy.filename;
    return copy;
  }

  it('throws when no config file name is passed, one that does not exist, invalid json5, or a non-babel option', async () => {
    await expect(extractBabelConfig(undefined as never)).rejects.toThrow();
    await expect(extractBabelConfig('config-does-not-exist')).rejects.toThrow();
    await expect(extractBabelConfig(fixture('babel/babelrc.invalid.json'))).rejects.toThrow();
    await expect(
      extractBabelConfig(fixture('babel/babelrc.not-a-babel-option.json')),
    ).rejects.toThrow();
  });

  it('returns a default options object when an empty config file is passed', async () => {
    expect(await withoutFilename('babel/babelrc.empty.json')).toEqual(
      DEFAULT_EMPTY_BABEL_OPTIONS_OBJECT,
    );
  });

  it("reads the 'babel' key when a package.json is passed, and the defaults when it has none", async () => {
    const config = (await extractBabelConfig(fixture('babel/package.json'))) as {
      plugins: unknown[];
    };
    expect(config.plugins).toHaveLength(1);
    expect(await withoutFilename('babel/no-babel-config-in-this-package.json')).toEqual(
      DEFAULT_EMPTY_BABEL_OPTIONS_OBJECT,
    );
  });

  it('returns a babel config when a javascript file with a regular object export is passed', async () => {
    const config = (await extractBabelConfig(
      fixture('babel-js/babel.object-export.config.js'),
    )) as {
      plugins: unknown[];
    };
    expect(config.plugins).toHaveLength(1);
  });

  it('throws when a javascript file with a syntax error is passed', async () => {
    const thrown = await extractBabelConfig(fixture('babel-js/babel.syntax-error.config.js')).then(
      () => '',
      (e: unknown) => (e as Error).message,
    );
    expect(thrown).toContain('Encountered an error while parsing babel config');
    expect(thrown).toContain('babel.syntax-error.config.js');
  });

  it('returns a babel config _including_ the array of plugins when a config with presets is passed', async () => {
    const config = (await extractBabelConfig(fixture('babel/babelrc.with-a-preset.json'))) as {
      presets: unknown[];
    };
    expect(config.presets).toEqual(['@babel/preset-typescript']);
  });

  it('throws when a javascript file with a function export, or an unsupported extension, is passed', async () => {
    await expect(
      extractBabelConfig(fixture('babel-js/babel.function-export.config.js')),
    ).rejects.toThrow(TypeError);
    await expect(
      extractBabelConfig(fixture('babel-js/babel.config.wildly-unsupported-extension')),
    ).rejects.toThrow(/in a format/);
  });

  it('returns a babel config even when an es module is passed', async () => {
    const config = (await extractBabelConfig(fixture('babel-js/babel.es-module.config.mjs'))) as {
      plugins: { key: string }[];
    };
    expect(config.plugins).toHaveLength(1);
    expect(config.plugins[0]?.key).toBe('transform-modules-commonjs');
  });

  it('returns {} without Babel, as dependency-cruiser does', async () => {
    expect(await babelWith(undefined, fixture('babel/babelrc.empty.json'))).toEqual({});
  });
});

describe('the transpilers found as dependency-cruiser finds them', () => {
  it('reads a major version and keeps to the supported ranges', () => {
    expect(majorOf('5.9.3')).toBe(5);
    expect(majorOf(undefined)).toBeUndefined();
    expect(majorOf('next')).toBeUndefined();
    expect(supported('typescript', '6.0.3')).toBe(true);
    expect(supported('typescript', '7.0.0')).toBe(false);
    expect(supported('typescript', '1.8.0')).toBe(false);
    expect(supported('@babel/core', '8.0.0')).toBe(false);
    expect(supported('@babel/core', 'unknown')).toBe(true);
  });

  it('gives undefined for a package that is absent or out of range', () => {
    expect(tryRequire('typescript')).toBeDefined();
    const missing = (): never => {
      throw new Error('absent');
    };
    expect(tryRequire('typescript', missing)).toBeUndefined();
    const old = (id: string): unknown => (id.endsWith('package.json') ? { version: '1.0.0' } : {});
    expect(tryRequire('typescript', old)).toBeUndefined();
  });
});
