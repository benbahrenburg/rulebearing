// Resolution off the graph's modules[] (plan 0002, Step 13): recorded specifiers, relative
// specifiers the graph has not recorded yet, bare specifiers, and the cache entry the plugin reads.
import { mkdirSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, sep } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  EXTENSIONS,
  GraphError,
  candidates,
  findCachedGraph,
  graphPath,
  isRelative,
  loadGraph,
  parseGraph,
  resolveSpecifier,
} from '../src/graph.js';

const GRAPH = JSON.stringify({
  modules: [
    {
      source: 'src/ui/view.ts',
      dependencies: [
        { module: '../db/query', resolved: 'src/db/query.ts' },
        { module: 'left-pad', resolved: 'left-pad', couldNotResolve: true },
        { module: '@/alias', resolved: 'src/lib/alias.ts' },
        { module: 7, resolved: 'ignored' },
        'not an edge',
      ],
    },
    {
      source: 'src/api/index.ts',
      dependencies: [
        { module: 'react', resolved: 'node_modules/react/index.js' },
        { module: '@/alias', resolved: 'src/lib/other.ts' },
      ],
    },
    { source: 'src/db/query.ts' },
    { source: 'src/db/store.ts', dependencies: [] },
    { source: 'src/db/index.tsx', dependencies: [] },
    { source: 'src/lib/esm.mts', dependencies: [] },
    { source: 'src/lib/data.json', dependencies: [] },
    { source: 'src/lib/view.ts', dependencies: [] },
    { source: 'src/lib/view.js', dependencies: [] },
    { name: 'no source' },
  ],
});

describe('parseGraph', () => {
  it('indexes modules by source and bare specifiers in source order', () => {
    const graph = parseGraph(GRAPH, 'g.json');
    expect([...graph.modules.keys()]).toEqual([
      'src/api/index.ts',
      'src/db/index.tsx',
      'src/db/query.ts',
      'src/db/store.ts',
      'src/lib/data.json',
      'src/lib/esm.mts',
      'src/lib/view.js',
      'src/lib/view.ts',
      'src/ui/view.ts',
    ]);
    expect(graph.modules.get('src/ui/view.ts')?.dependencies).toHaveLength(3);
    expect(graph.modules.get('src/db/query.ts')?.dependencies).toEqual([]);
    // The first module in source order names an alias; an unresolved package names nothing.
    expect(graph.bare.get('@/alias')).toBe('src/lib/other.ts');
    expect(graph.bare.has('left-pad')).toBe(false);
  });

  it.each([
    ['{', /g\.json is not JSON/],
    ['[]', /has no modules\[\]/],
    ['{"modules":{}}', /has no modules\[\]/],
    ['null', /has no modules\[\]/],
  ])('refuses %s', (text, reason) => {
    expect(() => parseGraph(text, 'g.json')).toThrow(reason);
    expect(() => parseGraph(text, 'g.json')).toThrow(GraphError);
  });
});

describe('resolveSpecifier', () => {
  const graph = parseGraph(GRAPH, 'g.json');
  it.each([
    // Recorded by the extractor for this module: its resolution, aliases included.
    ['src/ui/view.ts', '../db/query', 'src/db/query.ts'],
    ['src/ui/view.ts', '@/alias', 'src/lib/alias.ts'],
    ['src/ui/view.ts', 'left-pad', undefined],
    // Not recorded yet: a relative path among the modules, by extension and index.
    ['src/ui/view.ts', '../db/store', 'src/db/store.ts'],
    ['src/ui/view.ts', '../db/store.js', 'src/db/store.ts'],
    ['src/ui/view.ts', '../db', 'src/db/index.tsx'],
    ['src/ui/view.ts', '../db/', 'src/db/index.tsx'],
    ['src/ui/view.ts', '../lib/esm.mjs', 'src/lib/esm.mts'],
    ['src/ui/view.ts', '../lib/data.json', 'src/lib/data.json'],
    ['src/ui/view.ts', '../lib/view', 'src/lib/view.ts'],
    ['src/ui/view.ts', '../lib/view.js', 'src/lib/view.js'],
    ['src/ui/view.ts', './missing', undefined],
    ['src/new/file.ts', '../db/query', 'src/db/query.ts'],
    ['src/db/query.ts', '.', 'src/db/index.tsx'],
    ['src/db/deep/x.ts', '..', 'src/db/index.tsx'],
    // Bare: what any module resolved it to.
    ['src/ui/view.ts', 'react', 'node_modules/react/index.js'],
    ['src/ui/view.ts', 'vue', undefined],
  ])('%s: %s -> %s', (from, specifier, expected) => {
    expect(resolveSpecifier(graph, from, specifier)).toBe(expected);
  });
});

describe('candidates and isRelative', () => {
  it('tries the path, its TypeScript source, each extension, then each index file', () => {
    const list = candidates('src/a.js');
    expect(list.slice(0, 3)).toEqual(['src/a.js', 'src/a.ts', 'src/a.tsx']);
    expect(list).toHaveLength(3 + 2 * EXTENSIONS.length);
    expect(candidates('src/a')).toHaveLength(1 + 2 * EXTENSIONS.length);
    expect(candidates('src.d/a').slice(0, 2)).toEqual(['src.d/a', 'src.d/a.ts']);
  });

  it.each([
    ['.', true],
    ['..', true],
    ['./a', true],
    ['../a', true],
    ['a', false],
    ['@/a', false],
    ['.a', false],
    ['/abs', false],
  ])('%s relative: %s', (specifier, relative) => {
    expect(isRelative(specifier)).toBe(relative);
  });
});

describe('graphPath', () => {
  it('is relative to the folder and /-separated', () => {
    const cwd = join(sep, 'repo');
    expect(graphPath(cwd, join(cwd, 'src', 'a.ts'))).toBe('src/a.ts');
    expect(graphPath(cwd, join(sep, 'elsewhere', 'b.ts'))).toBe(
      join(sep, 'elsewhere', 'b.ts').split(sep).join('/'),
    );
  });
});

describe('findCachedGraph and loadGraph', () => {
  let dir: string;
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'rb-plugin-cache-'));
  });
  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  function entry(name: string, root: unknown, mtime: number, graph = GRAPH): string {
    const folder = join(dir, '.graph', 'cache', name);
    mkdirSync(folder, { recursive: true });
    writeFileSync(
      join(folder, 'key.json'),
      typeof root === 'string' ? JSON.stringify({ root }) : String(root),
    );
    const path = join(folder, 'graph.json');
    writeFileSync(path, graph);
    utimesSync(path, mtime, mtime);
    return path;
  }

  it('is undefined with no cache folder', () => {
    expect(findCachedGraph(dir)).toBeUndefined();
  });

  it('takes the newest entry whose worktree root holds the folder', async () => {
    const { realpathSync } = await import('node:fs');
    const root = realpathSync(dir).split(sep).join('/');
    const older = entry('a', root, 1_000);
    const newer = entry('b', `${root}/`, 2_000);
    entry('c', '/somewhere/else', 3_000);
    entry('d', '{ not json', 4_000);
    entry('e', '{"root":7}', 5_000);
    mkdirSync(join(dir, '.graph', 'cache', 'f'));
    expect(findCachedGraph(dir)).toBe(newer);
    utimesSync(older, 9_000, 9_000);
    expect(findCachedGraph(dir)).toBe(older);
    const parent = root.slice(0, root.lastIndexOf('/'));
    const outer = entry('g', parent, 10_000);
    expect(findCachedGraph(dir)).toBe(outer);
  });

  it('reads a graph once per modification and names what it cannot read', () => {
    const path = entry('a', dir, 1_000);
    const first = loadGraph(path);
    expect(loadGraph(path)).toBe(first);
    writeFileSync(path, JSON.stringify({ modules: [{ source: 'x.ts' }] }));
    utimesSync(path, 2_000, 2_000);
    const second = loadGraph(path);
    expect(second).not.toBe(first);
    expect([...second.modules.keys()]).toEqual(['x.ts']);
    expect(() => loadGraph(join(dir, 'absent.json'))).toThrow(/cannot read the graph/);
  });
});
