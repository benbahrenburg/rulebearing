// The agreement test of plan 0002, Step 13: over a fixture repository with violations, every
// message `rulebearing/boundaries` reports equals the gate's `junit` message for the same edge,
// with the rule's name in front, so the front-end cannot disagree with the gate.
import { execFileSync } from 'node:child_process';
import { cpSync, existsSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ESLint } from 'eslint';
import type { Linter } from 'eslint';
import tseslint from 'typescript-eslint';
import { afterEach, beforeEach, describe, expect, inject, it, vi } from 'vitest';
import plugin from '../src/index.js';
import { reported } from '../src/boundaries.js';
import type { BoundariesOptions } from '../src/boundaries.js';

const FIXTURE = fileURLToPath(new URL('fixture', import.meta.url));
const binary = inject('binary');
let dir: string;

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'eslint-plugin-rulebearing-'));
  cpSync(FIXTURE, dir, { recursive: true });
  vi.stubEnv('RULEBEARING_BINARY', binary);
});

afterEach(() => {
  vi.unstubAllEnvs();
  rmSync(dir, { recursive: true, force: true });
});

function unescape(text: string): string {
  return text
    .replaceAll('&#10;', '\n')
    .replaceAll('&quot;', '"')
    .replaceAll('&apos;', "'")
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&amp;', '&');
}

interface JunitCase {
  readonly name: string;
  readonly failure?: string;
  readonly output: readonly string[];
}

/** The gate's junit report over the fixture, one entry per test case. */
function junit(cwd: string): JunitCase[] {
  const xml = execFileSync(binary, ['cruise', '-c', 'rulebearing.yaml', '-T', 'junit', 'src'], {
    cwd,
    encoding: 'utf8',
  });
  const cases: JunitCase[] = [];
  for (const match of xml.matchAll(
    /<testcase name="([^"]*)"[^>]*?(?:\/>|>([\s\S]*?)<\/testcase>)/g,
  )) {
    const body = match[2] ?? '';
    const failure = /<failure type="error" message="([^"]*)"/.exec(body)?.[1];
    const output = /<system-out>([\s\S]*?)<\/system-out>/.exec(body)?.[1];
    cases.push({
      name: unescape(match[1] ?? ''),
      ...(failure === undefined ? {} : { failure: unescape(failure) }),
      output: output === undefined ? [] : unescape(output).split('\n'),
    });
  }
  return cases;
}

/** Each failing edge's message as the plugin words it: `<rule>: <fix>` then the junit line. */
function expectedFromJunit(cases: readonly JunitCase[]): string[] {
  return cases
    .flatMap((c) => {
      if (c.failure === undefined) {
        return [];
      }
      const [fix, ...edges] = c.failure.split('\n');
      return edges.map((edge) => `${c.name}: ${fix ?? ''}\n${edge}`);
    })
    .sort();
}

function config(options: BoundariesOptions = { config: 'rulebearing.yaml' }): Linter.Config[] {
  return [
    {
      files: ['**/*.ts'],
      languageOptions: { parser: tseslint.parser },
      plugins: { rulebearing: plugin },
      rules: { 'rulebearing/boundaries': ['error', options] },
    },
  ];
}

async function lint(
  options?: BoundariesOptions,
  files: readonly string[] = ['src/**/*.ts'],
): Promise<ESLint.LintResult[]> {
  const eslint = new ESLint({
    cwd: dir,
    overrideConfigFile: true,
    overrideConfig: config(options),
  });
  return eslint.lintFiles([...files]);
}

function messages(results: readonly ESLint.LintResult[]): string[] {
  return results.flatMap((r) => r.messages.map((m) => m.message)).sort();
}

describe('rulebearing/boundaries against the gate', () => {
  it('reports every failing edge with the junit message of the same edge', async () => {
    const expected = expectedFromJunit(junit(dir));
    expect(expected).toHaveLength(5);
    const results = await lint();
    expect(messages(results)).toEqual(expected);
    // The cache entry the plugin resolved against was written on the first question.
    expect(readdirSync(join(dir, '.graph', 'cache'))).toHaveLength(1);
    for (const result of results) {
      for (const m of result.messages) {
        expect(m.ruleId).toBe('rulebearing/boundaries');
        expect(m.severity).toBe(2);
      }
    }
  });

  it('reports at the import, one message per form', async () => {
    const results = await lint();
    const api = results.find((r) => r.filePath.endsWith(join('src', 'api', 'index.ts')));
    expect(api?.messages.map((m) => [m.line, m.column])).toEqual([
      [1, 1],
      [2, 1],
      [6, 10],
    ]);
    const view = results.find((r) => r.filePath.endsWith(join('src', 'ui', 'view.ts')));
    expect(view?.messages.map((m) => [m.line, m.column])).toEqual([
      [1, 1],
      [5, 16],
    ]);
  });

  it('answers the same from a saved graph as from the cache, byte for byte', async () => {
    execFileSync(
      binary,
      ['cruise', '-c', 'rulebearing.yaml', '-T', 'json', '-f', 'saved.json', 'src'],
      {
        cwd: dir,
      },
    );
    const fromCache = messages(await lint());
    const fromFile = messages(await lint({ config: 'rulebearing.yaml', graph: 'saved.json' }));
    expect(fromFile).toEqual(fromCache);
    expect(messages(await lint())).toEqual(fromCache);
  });

  it('adds the rules that only warn when asked, worded as junit lists them', async () => {
    const cases = junit(dir);
    const warned = cases.find((c) => c.name === 'services-not-to-ui');
    const line = warned?.output[0]?.replace(/^warn: /, '');
    const results = await lint({ config: 'rulebearing.yaml', severity: 'warn' });
    expect(messages(results)).toEqual(
      [
        ...expectedFromJunit(cases),
        `services-not-to-ui: Services never depend on the UI.\n${line ?? ''}`,
      ].sort(),
    );
  });

  it('checks an import written after the graph was cached, as the gate will see it', async () => {
    await lint();
    const added = join(dir, 'src', 'ui', 'added.ts');
    const code = "import { store } from '../db/store';\nexport const size = store.size;\n";
    writeFileSync(added, code);
    const eslint = new ESLint({ cwd: dir, overrideConfigFile: true, overrideConfig: config() });
    const [result] = await eslint.lintText(code, { filePath: added });
    const gate = expectedFromJunit(junit(dir)).filter((m) => m.includes('src/ui/added.ts'));
    expect(gate).toHaveLength(1);
    expect(result?.messages.map((m) => m.message)).toEqual(gate);
  });

  it('asks about a file created after the graph was cached, as the gate will', async () => {
    await lint();
    // A new file in the data layer the cached graph has never seen, and a UI file importing it.
    writeFileSync(join(dir, 'src', 'db', 'fresh.ts'), 'export const fresh = 1;\n');
    const importer = join(dir, 'src', 'ui', 'uses-fresh.ts');
    const code = "import { fresh } from '../db/fresh';\nexport const f = fresh;\n";
    writeFileSync(importer, code);
    const eslint = new ESLint({ cwd: dir, overrideConfigFile: true, overrideConfig: config() });
    const [result] = await eslint.lintText(code, { filePath: importer });
    const gate = expectedFromJunit(junit(dir)).filter((m) => m.includes('src/ui/uses-fresh.ts'));
    expect(gate).toHaveLength(1);
    expect(result?.messages.map((m) => m.message)).toEqual(gate);
  });

  it('leaves alone what the graph cannot name and what is allowed', async () => {
    const eslint = new ESLint({ cwd: dir, overrideConfigFile: true, overrideConfig: config() });
    const code = [
      "import pad from 'left-pad';",
      "import { formatDate } from './format';",
      "import './missing';",
      'const dynamic = require(pad);',
      'const two = require("a", "b");',
      'export { formatDate, dynamic, two };',
      'export const later = import(`../db/${String(pad)}`);',
      '',
    ].join('\n');
    const [result] = await eslint.lintText(code, { filePath: join(dir, 'src', 'ui', 'free.ts') });
    expect(result?.messages).toEqual([]);
  });

  it('reads a template literal without expressions as a specifier', async () => {
    const eslint = new ESLint({ cwd: dir, overrideConfigFile: true, overrideConfig: config() });
    const code = 'export const q = import(`../db/query`);\n';
    const [result] = await eslint.lintText(code, { filePath: join(dir, 'src', 'ui', 'tpl.ts') });
    expect(result?.messages.map((m) => m.message.split('\n')[0])).toEqual([
      'ui-not-to-db: Import from src/services instead of src/db.',
    ]);
  });

  it('says why it could not answer instead of passing silently', async () => {
    vi.stubEnv('RULEBEARING_BINARY', join(dir, 'no-such-binary'));
    const missing = messages(await lint(undefined, ['src/ui/view.ts']));
    expect(missing.length).toBeGreaterThan(0);
    expect(missing[0]).toMatch(
      /^rulebearing could not answer: rulebearing: RULEBEARING_BINARY is set to/,
    );
    vi.stubEnv('RULEBEARING_BINARY', binary);
    writeFileSync(join(dir, 'broken.yaml'), 'forbidden:\n  - name: x\n    severity: loud\n');
    const invalid = messages(
      await lint({ config: 'broken.yaml', graph: 'nope.json' }, ['src/ui/view.ts']),
    );
    expect(invalid[0]).toMatch(/^rulebearing could not answer: /);
    expect(existsSync(join(dir, 'nope.json'))).toBe(false);
  });

  it('reports the binary refusing a question at the import it was asked for', async () => {
    execFileSync(
      binary,
      ['cruise', '-c', 'rulebearing.yaml', '-T', 'json', '-f', 'saved.json', 'src'],
      {
        cwd: dir,
      },
    );
    writeFileSync(join(dir, 'broken.yaml'), 'forbidden:\n  - name: x\n    severity: loud\n');
    const results = await lint({ config: 'broken.yaml', graph: 'saved.json' }, ['src/ui/view.ts']);
    const all = messages(results);
    expect(all).toHaveLength(4);
    for (const m of all) {
      expect(m).toMatch(/^rulebearing could not answer: /);
    }
  });
});

describe('reported', () => {
  const answer = {
    verdict: 'no' as const,
    from: 'a.ts',
    to: 'b.ts',
    violations: [{ name: 'e', severity: 'error', id: 'RB-1' }],
    warnings: [
      { name: 'w', severity: 'warn', id: 'RB-2' },
      { name: 'i', severity: 'info', id: 'RB-3' },
    ],
  };
  it.each([
    [undefined, ['e']],
    ['error', ['e']],
    ['warn', ['e', 'w']],
    ['info', ['e', 'w', 'i']],
  ] as const)('severity %s reports %j', (severity, names) => {
    expect(reported(answer, severity).map((f) => f.name)).toEqual(names);
  });
});

describe('the plugin object', () => {
  it('names itself, carries its version and exports one rule', () => {
    expect(plugin.meta?.name).toBe('eslint-plugin-rulebearing');
    expect(plugin.meta?.version).toMatch(/^\d+\.\d+\.\d+/);
    expect(Object.keys(plugin.rules ?? {})).toEqual(['boundaries']);
  });
});
