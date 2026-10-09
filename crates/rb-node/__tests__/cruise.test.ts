// `cruise()` and `format()` answer what the command line answers: the same result for the same
// options as a `.dependency-cruiser.json`, the same reporter text, and the reporter's exit code.
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 22 ("`cruise()` on a
// fixture equals the CLI's JSON; `format()` on a saved JSON equals `fmt`").
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeAll, describe, expect, inject, it } from 'vitest';
import api, {
  allExtensions,
  cruise,
  format,
  getAvailableTranspilers,
} from '../../../wrappers/npm/src/api/index.js';
import type {
  ICruiseOptions,
  ICruiseResult,
} from '../../../wrappers/npm/types/dependency-cruiser/dependency-cruiser.mjs';

const FORBIDDEN = [
  {
    name: 'ui-not-to-db',
    severity: 'error',
    comment: 'go through a service',
    from: { path: '^src/ui/' },
    to: { path: '^src/db/' },
  },
  { name: 'no-circular', severity: 'warn', from: {}, to: { circular: true } },
] as const;

const OPTIONS = {
  validate: true,
  ruleSet: { forbidden: FORBIDDEN },
  doNotFollow: { path: 'node_modules' },
} as unknown as ICruiseOptions;

let repository = '';
let started = '';

/** Runs the binary in the fixture repository. */
function cli(args: string[], input?: string): { stdout: string; stderr: string; status: number } {
  const done = spawnSync(inject('binary'), args, {
    cwd: repository,
    encoding: 'utf8',
    ...(input === undefined ? {} : { input }),
  });
  return { stdout: done.stdout, stderr: done.stderr, status: done.status ?? -1 };
}

/** The result with what differs by design between a run from a file and one from an object. */
function comparable(result: unknown): unknown {
  const copy = structuredClone(result) as { summary: { optionsUsed: Record<string, unknown> } };
  delete copy.summary.optionsUsed.rulesFile;
  return copy;
}

beforeAll(() => {
  started = process.cwd();
  repository = realpathSync(mkdtempSync(join(tmpdir(), 'rb-node-cruise-')));
  for (const folder of ['src/ui', 'src/db', 'src/services']) {
    mkdirSync(join(repository, folder), { recursive: true });
  }
  writeFileSync(join(repository, 'package.json'), '{ "name": "fixture" }\n');
  writeFileSync(join(repository, 'src/db/store.ts'), 'export const store = 1;\n');
  writeFileSync(
    join(repository, 'src/services/s.ts'),
    "import { store } from '../db/store';\nexport const s = store;\n",
  );
  writeFileSync(
    join(repository, 'src/ui/page.ts'),
    "import { store } from '../db/store';\nimport { s } from '../services/s';\nexport const page = store + s;\n",
  );
  writeFileSync(
    join(repository, '.dependency-cruiser.json'),
    `${JSON.stringify({ forbidden: FORBIDDEN, options: { doNotFollow: { path: 'node_modules' } } }, null, 2)}\n`,
  );
  process.chdir(repository);
});

afterAll(() => {
  process.chdir(started);
  rmSync(repository, { recursive: true, force: true });
});

describe('cruise()', () => {
  it('without an outputType returns the result object the json reporter prints', async () => {
    const reported = await cruise(['src'], OPTIONS);
    expect(reported.exitCode).toBe(0);
    expect(typeof reported.output).toBe('object');
    const run = cli([
      'cruise',
      '--config',
      '.dependency-cruiser.json',
      '--output-type',
      'json',
      'src',
    ]);
    expect(run.status).toBe(0);
    expect(comparable(reported.output)).toEqual(comparable(JSON.parse(run.stdout)));
    const result = reported.output as ICruiseResult;
    expect(result.summary.error).toBe(1);
    expect(result.summary.violations.map((v) => v.rule.name)).toEqual(['ui-not-to-db']);
  });

  it('with an outputType returns the reporter text and the count it gates on', async () => {
    const reported = await cruise(['src'], { ...OPTIONS, outputType: 'err' });
    const run = cli([
      'cruise',
      '--config',
      '.dependency-cruiser.json',
      '--output-type',
      'err',
      'src',
    ]);
    expect(reported.output).toBe(run.stdout);
    expect(reported.exitCode).toBe(1);
    expect(reported.exitCode).toBe(run.status);
    const json = await cruise(['src'], { ...OPTIONS, outputType: 'json' });
    expect(typeof json.output).toBe('string');
    expect(json.exitCode).toBe(0);
  });

  it('applies a rule set only when validate is true, as dependency-cruiser does', async () => {
    const reported = await cruise(['src'], { ...OPTIONS, validate: false });
    const result = reported.output as ICruiseResult;
    expect(result.summary.error).toBe(0);
    expect(result.modules.map((m) => m.source)).toContain('src/ui/page.ts');
  });

  it('takes the options of ruleSet.options under the options given', async () => {
    const reported = await cruise(['src'], {
      validate: true,
      ruleSet: { forbidden: [...FORBIDDEN], options: { exclude: { path: 'services' } } },
    } as unknown as ICruiseOptions);
    const result = reported.output as ICruiseResult;
    expect(result.modules.map((m) => m.source)).not.toContain('src/services/s.ts');
  });

  it('applies a webpack resolve block passed as resolveOptions', async () => {
    writeFileSync(
      join(repository, 'src/ui/aliased.ts'),
      "import { store } from '@db/store';\nexport const aliased = store;\n",
    );
    try {
      const reported = await cruise(['src'], OPTIONS, {
        alias: { '@db': join(repository, 'src/db') },
      });
      const aliased = (reported.output as ICruiseResult).modules.find(
        (m) => m.source === 'src/ui/aliased.ts',
      );
      expect(aliased?.dependencies.map((d) => d.resolved)).toEqual(['src/db/store.ts']);
    } finally {
      rmSync(join(repository, 'src/ui/aliased.ts'));
    }
  });

  it('reads a transpile object by the file it records, and refuses one that records none', async () => {
    writeFileSync(join(repository, 'tsconfig.json'), '{ "compilerOptions": { "baseUrl": "." } }\n');
    const tsConfig = { options: { configFilePath: join(repository, 'tsconfig.json') } };
    const reported = await cruise(['src'], OPTIONS, undefined, { tsConfig });
    const used = (reported.output as ICruiseResult).summary.optionsUsed as Record<string, unknown>;
    expect(used.tsConfig).toEqual({ fileName: join(repository, 'tsconfig.json') });
    await expect(cruise(['src'], OPTIONS, undefined, { tsConfig: {} })).resolves.toBeDefined();
    await expect(
      cruise(['src'], OPTIONS, undefined, { tsConfig: { options: { strict: true } } }),
    ).rejects.toThrow(/records no file/);
    await expect(
      cruise(['src'], { ...OPTIONS, tsConfig: { fileName: 'other.json' } }, undefined, {
        tsConfig,
      }),
    ).rejects.toThrow(/names other\.json/);
    await expect(
      cruise(['src'], OPTIONS, undefined, { babelConfig: { plugins: [] } }),
    ).rejects.toThrow(/records no file/);
    await expect(cruise(['src'], OPTIONS, undefined, { swc: {} } as never)).rejects.toThrow(
      /not a transpile option/,
    );
  });

  it('rejects what the command line exits 2 or 3 for, with its reason', async () => {
    await expect(cruise(['no-such-folder'], OPTIONS)).rejects.toThrow(/no-such-folder|no modules/);
    await expect(cruise(['src'], { outputType: 'nonsense' })).rejects.toThrow();
    await expect(cruise(['src'], 'not options' as never)).rejects.toThrow(/an object/);
    await expect(cruise('src' as never)).rejects.toThrow(TypeError);
  });
});

describe('format()', () => {
  it('formats a saved result as fmt does, with the count the reporter gates on', async () => {
    const run = cli([
      'cruise',
      '--config',
      '.dependency-cruiser.json',
      '--output-type',
      'json',
      'src',
    ]);
    const saved = JSON.parse(run.stdout) as ICruiseResult;
    const reported = await format(saved, { outputType: 'err' });
    const fmt = cli(['fmt', '--output-type', 'err', '--exit-code', '-'], run.stdout);
    expect(reported.output).toBe(fmt.stdout);
    expect(reported.exitCode).toBe(fmt.status);
    expect(reported.exitCode).toBe(1);
  });

  it('without an outputType returns the re-summarised result object', async () => {
    const run = cli([
      'cruise',
      '--config',
      '.dependency-cruiser.json',
      '--output-type',
      'json',
      'src',
    ]);
    const saved = JSON.parse(run.stdout) as ICruiseResult;
    const reported = await format(saved, { exclude: '^src/ui' });
    expect(reported.exitCode).toBe(0);
    const result = reported.output as ICruiseResult;
    expect(result.modules.map((m) => m.source)).not.toContain('src/ui/page.ts');
    const fmt = cli(['fmt', '--output-type', 'json', '--exclude', '^src/ui', '-'], run.stdout);
    expect(result).toEqual(JSON.parse(fmt.stdout));
    expect((await format(saved)).output).toEqual(
      JSON.parse(cli(['fmt', '-T', 'json', '-'], run.stdout).stdout),
    );
  });

  it('rejects an option it does not know and a result it cannot read', async () => {
    const run = cli([
      'cruise',
      '--config',
      '.dependency-cruiser.json',
      '--output-type',
      'json',
      'src',
    ]);
    const saved = JSON.parse(run.stdout) as ICruiseResult;
    await expect(format(saved, { outputTo: 'x' } as never)).rejects.toThrow(/not a format option/);
    await expect(format({ modules: 1 } as never, { outputType: 'err' })).rejects.toThrow();
  });
});

describe('the package', () => {
  it("exports dependency-cruiser's names", () => {
    expect(Object.keys(api).sort()).toEqual(
      ['allExtensions', 'cruise', 'format', 'getAvailableTranspilers'].sort(),
    );
    expect(api.cruise).toBe(cruise);
  });

  it('lists the extensions and transpilers this build reads', () => {
    expect(allExtensions.map((e) => e.extension)).toEqual([
      '.js',
      '.cjs',
      '.mjs',
      '.jsx',
      '.ts',
      '.tsx',
      '.d.ts',
      '.cts',
      '.d.cts',
      '.mts',
      '.d.mts',
      '.vue',
      '.svelte',
      '.ls',
      '.coffee',
      '.litcoffee',
      '.coffee.md',
      '.csx',
      '.cjsx',
    ]);
    expect(allExtensions.find((e) => e.extension === '.ts')?.available).toBe(true);
    expect(allExtensions.find((e) => e.extension === '.coffee')?.available).toBe(false);
    const transpilers = getAvailableTranspilers();
    expect(transpilers.map((t) => t.name)).toEqual([
      'javascript',
      'babel',
      'coffee-script',
      'coffeescript',
      'livescript',
      'svelte',
      'swc',
      'typescript',
      'vue-template-compiler',
      '@vue/compiler-sfc',
    ]);
    const typescript = transpilers.find((t) => t.name === 'typescript');
    expect(typescript).toMatchObject({ version: '*', available: true });
    expect(typescript?.currentVersion).toMatch(/^oxc \d+\.\d+\.\d+$/);
    expect(transpilers.find((t) => t.name === 'livescript')).toMatchObject({
      version: '>=1.0.0 <2.0.0',
      available: false,
      currentVersion: '-',
    });
  });
});
