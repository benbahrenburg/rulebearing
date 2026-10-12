// A whole build and the command line, over a small repository made for the test: pages from tracked
// documents only, the schemas from the tag, the playground preloaded, and a broken link failing.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3; docs/adr/0039-plans-and-design-kept-local.md.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  PLAYGROUND_FIXTURE,
  PLAYGROUND_REPOS,
  buildSite,
  playgroundExamples,
} from '../src/build.js';
import { main, parseArgs } from '../src/cli.js';
import { SCHEMA_BASE } from '../src/schemas.js';
import { COVERAGE_DATA, SCALE_DATA } from '../src/tables.js';

const realRepo = resolve(import.meta.dirname, '../..');
let repo: string;
let work: string;

function write(path: string, text: string): void {
  const full = join(repo, path);
  mkdirSync(dirname(full), { recursive: true });
  writeFileSync(full, text);
}

function git(...args: string[]): void {
  execFileSync('git', args, { cwd: repo, stdio: ['ignore', 'pipe', 'pipe'] });
}

function capture(): {
  io: { out: (t: string) => void; err: (t: string) => void };
  out: string[];
  err: string[];
} {
  const out: string[] = [];
  const err: string[] = [];
  return { io: { out: (t) => out.push(t), err: (t) => err.push(t) }, out, err };
}

beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), 'rb-site-build-'));
  repo = join(work, 'repo');
  mkdirSync(repo);
  git('init', '-q');
  git('config', 'user.email', 'test@example.com');
  git('config', 'user.name', 'test');
  for (const name of ['v1.json', 'config-v1.json']) {
    write(`schema/${name}`, JSON.stringify({ $id: SCHEMA_BASE + name }));
  }
  write(
    'README.md',
    '# Home\n\nSee [the CLI](docs/cli.md#usage) and [the schema](schema/v1.json).\n',
  );
  write(
    'docs/cli.md',
    '# CLI\n\n## Usage\n\n[Plan](plans/pending/p.md), [ADR](adr/0067-the-playground-facade-and-an-optional-javascript-engine.md).\n',
  );
  write('docs/adr/0067-the-playground-facade-and-an-optional-javascript-engine.md', '# ADR-0067\n');
  // The live tables' data and the decisions their pages link (Step 4).
  for (const adr of [
    '0009-conformance-suites-as-specification',
    '0074-the-coverage-ledger-is-tracked-and-its-status-is-computed',
  ]) {
    write(`docs/adr/${adr}.md`, `# ${adr}\n`);
  }
  for (const data of [COVERAGE_DATA, SCALE_DATA]) {
    write(data, readFileSync(join(realRepo, data), 'utf8'));
  }
  write('schema/README.md', '# Schemas\n\n[v1](v1.json)\n');
  write('cookbook/README.md', '# Cookbook\n');
  write('site/web/site.css', 'body {}\n');
  for (const file of ['rulebearing.yaml', 'graph.json']) {
    write(
      join(PLAYGROUND_FIXTURE, file),
      readFileSync(join(realRepo, PLAYGROUND_FIXTURE, file), 'utf8'),
    );
  }
  for (const { dir } of PLAYGROUND_REPOS) {
    const files = execFileSync('git', ['ls-files', '-z', dir], { cwd: realRepo, encoding: 'utf8' })
      .split('\0')
      .filter((f) => f !== '');
    for (const path of files) {
      write(path, readFileSync(join(realRepo, path), 'utf8'));
    }
  }
  git('add', '-f', '.');
  git('commit', '-qm', 'one');
  git('tag', 'v0.2.0');
  // Kept local, never tracked: it must not become a page.
  write('docs/plans/pending/p.md', '# Plan\n');
  for (const [dir, files] of [
    ['wasm', ['rb_wasm.js', 'rb_wasm_bg.wasm']],
    ['web', ['playground.js', 'playground-core.js', 'playground-files.js']],
  ] as const) {
    for (const file of files) {
      mkdirSync(join(work, dir), { recursive: true });
      writeFileSync(join(work, dir, file), '');
    }
  }
});

afterEach(() => {
  rmSync(work, { recursive: true, force: true });
});

const args = (): string[] => [
  'build',
  '--repo',
  repo,
  '--out',
  join(work, 'site'),
  '--wasm',
  join(work, 'wasm'),
  '--web',
  join(work, 'web'),
];

describe('buildSite', () => {
  it('builds the tracked documents, the playground and the released schemas', () => {
    const result = buildSite({
      repo,
      out: join(work, 'site'),
      wasmDir: join(work, 'wasm'),
      webDir: join(work, 'web'),
      schemaRef: 'v0.2.0',
      commit: 'c0ffee',
    });
    expect(result.pages).toEqual([
      'cookbook/index.html',
      'coverage/archunitnet.html',
      'coverage/dependency-cruiser.html',
      'coverage/index.html',
      'coverage/scale.html',
      'docs/adr/0009-conformance-suites-as-specification.html',
      'docs/adr/0067-the-playground-facade-and-an-optional-javascript-engine.html',
      'docs/adr/0074-the-coverage-ledger-is-tracked-and-its-status-is-computed.html',
      'docs/cli.html',
      'index.html',
      'playground.html',
      'schema/index.html',
    ]);
    expect(result.brokenSources).toEqual([]);
    expect(result.brokenSite).toEqual([]);
    const site = join(work, 'site');
    expect(existsSync(join(site, 'docs/plans'))).toBe(false);
    expect(readFileSync(join(site, 'coverage/index.html'), 'utf8')).toContain('rows proven');
    expect(readFileSync(join(site, 'coverage/scale.html'), 'utf8')).toContain(
      'blob/c0ffee/testbeds/results/scale.json',
    );
    expect(readFileSync(join(site, 'index.html'), 'utf8')).toContain('href="docs/cli.html#usage"');
    expect(readFileSync(join(site, 'docs/cli.html'), 'utf8')).toContain('class="local-only"');
    const playground = readFileSync(join(site, 'playground.html'), 'utf8');
    expect(playground).toContain('name: no-circular');
    for (const id of ['no-circular', 'typescript', 'python']) {
      expect(playground).toContain(`data-example="${id}"`);
      expect(playground).toContain(`data-example-data="${id}"`);
    }
    for (const file of [
      'assets/site.css',
      'assets/playground.js',
      'assets/playground-files.js',
      'playground/rb_wasm_bg.wasm',
    ]) {
      expect(existsSync(join(site, file))).toBe(true);
    }
  });

  it('serves the schemas of the tag, not of the checkout', () => {
    write('schema/v1.json', JSON.stringify({ $id: SCHEMA_BASE + 'v1.json', wip: true }));
    git('commit', '-qam', 'wip');
    buildSite({
      repo,
      out: join(work, 'site'),
      wasmDir: join(work, 'wasm'),
      webDir: join(work, 'web'),
      schemaRef: 'v0.2.0',
    });
    expect(readFileSync(join(work, 'site/schema/v1.json'), 'utf8')).not.toContain('wip');
  });

  it('needs the data of the live tables', () => {
    rmSync(join(repo, SCALE_DATA));
    expect(() =>
      buildSite({
        repo,
        out: join(work, 'site'),
        wasmDir: join(work, 'wasm'),
        webDir: join(work, 'web'),
        schemaRef: 'v0.2.0',
      }),
    ).toThrow(/testbeds\/results\/scale.json is missing; the nightly writes it/u);
  });

  it('names a missing build input', () => {
    rmSync(join(work, 'wasm'), { recursive: true });
    expect(() =>
      buildSite({
        repo,
        out: join(work, 'site'),
        wasmDir: join(work, 'wasm'),
        webDir: join(work, 'web'),
        schemaRef: 'v0.2.0',
      }),
    ).toThrow(/rb_wasm.js is missing/u);
  });
});

describe('the cookbook question pages', () => {
  const slug = 'domain-does-no-io';

  function addQuestion(index: string): void {
    const dir = `cookbook/questions/${slug}`;
    const files = execFileSync('git', ['ls-files', '-z', dir], { cwd: realRepo, encoding: 'utf8' })
      .split('\0')
      .filter((f) => f !== '');
    for (const path of files) {
      write(path, readFileSync(join(realRepo, path), 'utf8'));
    }
    // The page links the PRD and two ADRs (and the plan and the design, which are local-only);
    // give the small repository each target and anchor.
    write('docs/prd.md', '# PRD\n\n## FR-REACH-01\n');
    write('docs/adr/0013-ruff-parser-for-python.md', '# ADR-0013\n');
    write('docs/adr/0038-a-rule-narrows-the-graph-it-sees.md', '# ADR-0038\n');
    write('cookbook/questions/README.md', index);
    git('add', '-f', '.');
    git('commit', '-qm', 'question');
  }

  const options = (): Parameters<typeof buildSite>[0] => ({
    repo,
    out: join(work, 'site'),
    wasmDir: join(work, 'wasm'),
    webDir: join(work, 'web'),
    schemaRef: 'v0.2.0',
    commit: 'c0ffee',
  });

  it('renders a question from its fixture, with its playground preloaded', () => {
    addQuestion(`# Questions\n\n[The domain does no I/O](${slug}/README.md)\n`);
    const result = buildSite(options());
    expect(result.brokenSources).toEqual([]);
    expect(result.brokenSite).toEqual([]);
    expect(result.pages).toContain(`cookbook/questions/${slug}/index.html`);
    const page = readFileSync(join(work, 'site', `cookbook/questions/${slug}/index.html`), 'utf8');
    expect(page).toContain('<script type="module" src="../../../assets/playground.js"></script>');
    expect(page).toContain(`data-preload="${slug}"`);
    expect(page).toContain('<code>.dependency-cruiser.json</code>');
    expect(page).toContain('<code>shop/domain/invoice.py</code>');
    expect(page).not.toContain('RBCOOKBOOKMARKER');
    expect(readFileSync(join(work, 'site', 'cookbook/questions/index.html'), 'utf8')).toContain(
      `href="${slug}/index.html"`,
    );
  });

  it('refuses a question the index does not link', () => {
    addQuestion('# Questions\n');
    expect(() => buildSite(options())).toThrow(
      `cookbook/questions/README.md does not link the question page(s) ${slug}`,
    );
  });
});

describe('playgroundExamples', () => {
  it("embeds each fixture repository's tracked files by their path inside it", () => {
    const tracked = execFileSync('git', ['ls-files', '-z'], { cwd: repo, encoding: 'utf8' })
      .split('\0')
      .filter((f) => f !== '');
    const [ts, py] = playgroundExamples(repo, tracked);
    const files = (ts?.data.files ?? {}) as Record<string, string>;
    expect(Object.keys(files)).toContain('node_modules/lodash/index.js');
    expect(Object.keys(files)).toContain('src/order/discount.ts');
    expect(ts?.data).toMatchObject({ mode: 'files', language: 'typescript', rule: 'no-circular' });
    expect(ts?.data.config).toBe(files['rulebearing.yaml']);
    expect(py?.data).toMatchObject({ language: 'python', rule: 'ui-not-to-models' });
    expect(Object.keys((py?.data.files ?? {}) as Record<string, string>)).toContain(
      '.venv/pyvenv.cfg',
    );
  });

  it('needs the configuration of each repository', () => {
    expect(() => playgroundExamples(repo, [])).toThrow(
      /rulebearing.yaml is not tracked; the playground's typescript example needs it/u,
    );
  });
});

describe('the command line', () => {
  it('builds with the highest release tag by default and reports success', async () => {
    const { io, out, err } = capture();
    expect(await main(args(), io)).toBe(0);
    expect(err).toEqual([]);
    expect(out.join('')).toBe('site: 12 pages, schemas from v0.2.0, 0 broken link(s)\n');
    const check = capture();
    expect(await main(['check', join(work, 'site')], check.io)).toBe(0);
    expect(check.out.join('')).toBe('site: 0 broken link(s)\n');
  });

  it('fails on a broken link in a source and on the built site', async () => {
    write('docs/cli.md', '# CLI\n\n[gone](gone.md) [anchor](../README.md#nowhere)\n');
    git('commit', '-qam', 'break');
    const { io, err } = capture();
    expect(await main(args(), io)).toBe(1);
    expect(err.join('')).toContain('docs/cli.md: gone.md: no tracked file or folder docs/gone.md');
    expect(err.join('')).toContain('docs/cli.html: no such anchor -> ../index.html#nowhere');
    const check = capture();
    expect(await main(['check', join(work, 'site')], check.io)).toBe(1);
  });

  it('needs a release tag, the folders and a known command', async () => {
    git('tag', '-d', 'v0.2.0');
    const noTag = capture();
    expect(await main(args(), noTag.io)).toBe(2);
    expect(noTag.err.join('')).toContain('no release tag');
    const noOut = capture();
    expect(await main(['build', '--repo', repo, '--schema-ref', 'v0.2.0'], noOut.io)).toBe(2);
    expect(noOut.err.join('')).toBe('--out is required\n');
    const unknown = capture();
    expect(await main(['publish'], unknown.io)).toBe(2);
    expect(unknown.err.join('')).toContain('usage:');
    const noDir = capture();
    expect(await main(['check'], noDir.io)).toBe(2);
    expect(await main(['serve'], noDir.io)).toBe(2);
  });

  it('names a port it cannot serve on', async () => {
    const { io, err } = capture();
    expect(await main(['serve', work, '--port', '-1'], io)).toBe(2);
    expect(err.join('')).toMatch(/port/iu);
  });

  it("checks the site's npm licences against deny.toml", async () => {
    const { io, out } = capture();
    expect(await main(['licences', '--repo', realRepo], io)).toBe(0);
    expect(out.join('')).toBe('licences: 0 package(s) outside the allow-list\n');
  });

  it('parses flags and refuses a flag without a value', () => {
    expect(parseArgs(['a', '--x', '1', 'b'])).toEqual({
      positional: ['a', 'b'],
      flags: new Map([['x', '1']]),
    });
    expect(() => parseArgs(['--x'])).toThrow('--x needs a value');
    expect(() => parseArgs(['--x', '--y', '1'])).toThrow('--x needs a value');
  });
});
