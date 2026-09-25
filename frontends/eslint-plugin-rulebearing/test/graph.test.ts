// Resolution off the graph's modules[] (plan 0002, Step 13): recorded specifiers, relative
// specifiers the graph has not recorded yet, bare specifiers, and the cache entry the plugin reads.
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, realpathSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, sep } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  EXTENSIONS,
  GraphError,
  candidates,
  comparable,
  configAgrees,
  fileIn,
  findCachedGraph,
  graphPath,
  hashFiles,
  head,
  headFromFiles,
  holds,
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

  it('falls back to a file on disk for a relative specifier no module matches', () => {
    const onDisk = new Set(['src/db/fresh.ts', 'src/db/newdir/index.ts', 'src/db/store.js']);
    const isFile = (path: string): boolean => onDisk.has(path);
    expect(resolveSpecifier(graph, 'src/ui/view.ts', '../db/fresh', isFile)).toBe(
      'src/db/fresh.ts',
    );
    expect(resolveSpecifier(graph, 'src/ui/view.ts', '../db/newdir', isFile)).toBe(
      'src/db/newdir/index.ts',
    );
    // A module of the graph still wins over a file on disk.
    expect(resolveSpecifier(graph, 'src/ui/view.ts', '../db/store', isFile)).toBe(
      'src/db/store.ts',
    );
    expect(resolveSpecifier(graph, 'src/ui/view.ts', './missing', isFile)).toBeUndefined();
    // A bare specifier is never looked up on disk.
    expect(resolveSpecifier(graph, 'src/ui/view.ts', 'fresh', () => true)).toBeUndefined();
  });
});

describe('fileIn', () => {
  it('accepts files under the folder and refuses folders and missing paths', () => {
    const dir = mkdtempSync(join(tmpdir(), 'rb-plugin-filein-'));
    mkdirSync(join(dir, 'src', 'sub'), { recursive: true });
    writeFileSync(join(dir, 'src', 'a.ts'), '');
    const isFile = fileIn(dir);
    expect(isFile('src/a.ts')).toBe(true);
    expect(isFile('src/sub')).toBe(false);
    expect(isFile('src/b.ts')).toBe(false);
    rmSync(dir, { recursive: true, force: true });
  });
});

describe('comparable and holds', () => {
  it.each([
    ['//?/C:/repo', 'C:/repo'],
    ['\\\\?\\C:\\repo', 'C:/repo'],
    ['//?/c:/repo/', 'C:/repo'],
    ['c:\\repo\\sub', 'C:/repo/sub'],
    ['//?/UNC/host/share/repo', '//host/share/repo'],
    ['/home/me/repo/', '/home/me/repo'],
    ['/', '/'],
  ])('%s compares as %s', (given, expected) => {
    expect(comparable(given)).toBe(expected);
  });

  it.each([
    ['//?/C:/repo', 'c:\\repo\\src', true],
    ['//?/C:/repo', 'C:\\repo', true],
    ['C:/repo', 'C:/repository', false],
    ['//?/D:/repo', 'C:/repo', false],
    ['/r', '/r/a', true],
    ['/r/', '/r', true],
    ['/r', '/rr', false],
  ])('%s holds %s: %s', (root, here, expected) => {
    expect(holds(root, here)).toBe(expected);
  });
});

describe('hashFiles', () => {
  it('is hash_files of rb-cli: length-prefixed fields in name order', () => {
    // The configuration hash rb-cli records for one file named rulebearing.yaml holding "x\n",
    // computed independently here from the definition.
    const expected = createHash('sha256');
    for (const field of [Buffer.from('rulebearing.yaml'), Buffer.from('x\n')]) {
      const length = Buffer.alloc(8);
      length.writeBigUInt64BE(BigInt(field.length));
      expected.update(length);
      expected.update(field);
    }
    expect(hashFiles([['rulebearing.yaml', Buffer.from('x\n')]])).toBe(expected.digest('hex'));
    const ab = hashFiles([
      ['b', Buffer.from('2')],
      ['a', Buffer.from('1')],
    ]);
    expect(ab).toBe(
      hashFiles([
        ['a', Buffer.from('1')],
        ['b', Buffer.from('2')],
      ]),
    );
    expect(hashFiles([])).toBe(createHash('sha256').digest('hex'));
  });
});

describe('headFromFiles', () => {
  const A = '0123456789abcdef0123456789abcdef01234567';
  const B = '89abcdef0123456789abcdef0123456789abcdef';
  let dir: string;
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'rb-plugin-head-'));
  });
  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });
  function put(file: string, text: string): void {
    const path = join(dir, file);
    mkdirSync(join(path, '..'), { recursive: true });
    writeFileSync(path, text);
  }

  it('reads detached, loose, packed and linked-worktree HEADs as rb-cli does', () => {
    expect(headFromFiles(dir)).toBeUndefined();
    put('.git/HEAD', `${A}\n`);
    expect(headFromFiles(dir)).toBe(A);
    put('.git/HEAD', 'ref: refs/heads/main\n');
    put('.git/refs/heads/main', `${B}\n`);
    expect(headFromFiles(dir)).toBe(B);
    rmSync(join(dir, '.git/refs/heads/main'));
    put(
      '.git/packed-refs',
      `# pack-refs with: peeled\n${A} refs/heads/other\n^${B}\n${B} refs/heads/main\n`,
    );
    expect(headFromFiles(dir)).toBe(B);
    put('linked/.git', 'gitdir: ../.git/worktrees/linked\n');
    put('.git/worktrees/linked/HEAD', 'ref: refs/heads/feature\n');
    put('.git/worktrees/linked/commondir', '../..\n');
    put('.git/refs/heads/feature', `${A}\n`);
    expect(headFromFiles(join(dir, 'linked'))).toBe(A);
    put('.git/HEAD', 'ref: refs/heads/unborn\n');
    expect(headFromFiles(dir)).toBeUndefined();
    put('.git/HEAD', 'garbage\n');
    expect(headFromFiles(dir)).toBeUndefined();
    put('.git/HEAD', 'ref: refs/heads/loop\n');
    put('.git/refs/heads/loop', 'ref: refs/heads/loop\n');
    expect(headFromFiles(dir)).toBeUndefined();
    put('other/.git', 'no gitdir line\n');
    expect(headFromFiles(join(dir, 'other'))).toBeUndefined();
  });

  it('is empty outside a repository', () => {
    // The temporary folder is outside any repository on CI; git says so and head is empty.
    const outside = head(dir);
    expect(outside === '' || /^[0-9a-f]{40,64}$/.test(outside)).toBe(true);
    put('.git/HEAD', `${A}\n`);
    expect(head(dir)).toBe(A);
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
  const HEAD = '0123456789abcdef0123456789abcdef01234567';
  const OTHER = '89abcdef0123456789abcdef0123456789abcdef';
  const CONFIG = 'forbidden: []\n';
  let dir: string;
  let root: string;
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'rb-plugin-cache-'));
    root = realpathSync(dir).split(sep).join('/');
    mkdirSync(join(dir, '.git'));
    writeFileSync(join(dir, '.git', 'HEAD'), `${HEAD}\n`);
    writeFileSync(join(dir, 'rulebearing.yaml'), CONFIG);
  });
  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  const configHash = (text = CONFIG): string =>
    hashFiles([['rulebearing.yaml', Buffer.from(text)]]);

  function entry(name: string, key: unknown, mtime: number, graph = GRAPH): string {
    const folder = join(dir, '.graph', 'cache', name);
    mkdirSync(folder, { recursive: true });
    writeFileSync(join(folder, 'key.json'), typeof key === 'string' ? key : JSON.stringify(key));
    const path = join(folder, 'graph.json');
    writeFileSync(path, graph);
    utimesSync(path, mtime, mtime);
    return path;
  }

  function key(overrides: Record<string, unknown> = {}): Record<string, unknown> {
    return {
      root,
      head: HEAD,
      configHash: configHash(),
      configFiles: ['rulebearing.yaml'],
      version: '0.1.0',
      inputs: 'x',
      ...overrides,
    };
  }

  it('is undefined with no cache folder', () => {
    expect(findCachedGraph(dir)).toBeUndefined();
  });

  it('takes the newest entry of this root, HEAD and configuration', () => {
    const older = entry('a', key(), 1_000);
    const newer = entry('b', key({ root: `${root}/` }), 2_000);
    entry('c', key({ root: '/somewhere/else' }), 3_000);
    entry('d', '{ not json', 4_000);
    entry('e', '{"root":7}', 5_000);
    entry('h', key({ head: OTHER }), 6_000);
    entry('i', key({ configHash: configHash('forbidden: [x]\n') }), 7_000);
    entry('j', key({ configFiles: [] }), 8_000);
    entry('k', key({ configFiles: 'rulebearing.yaml' }), 8_500);
    entry('l', key({ head: 7 }), 8_700);
    mkdirSync(join(dir, '.graph', 'cache', 'f'));
    expect(findCachedGraph(dir)).toBe(newer);
    utimesSync(older, 9_000, 9_000);
    expect(findCachedGraph(dir)).toBe(older);
    // A new commit: no entry agrees until the binary writes one.
    writeFileSync(join(dir, '.git', 'HEAD'), `${OTHER}\n`);
    expect(findCachedGraph(dir)).toBe(join(dir, '.graph', 'cache', 'h', 'graph.json'));
    // An edited configuration: none agrees.
    writeFileSync(join(dir, 'rulebearing.yaml'), 'forbidden: [y]\n');
    expect(findCachedGraph(dir)).toBeUndefined();
  });

  it('matches the configuration option and a root written the Windows way', () => {
    writeFileSync(join(dir, 'other.yaml'), 'x: 1\n');
    const mine = entry('a', key(), 1_000);
    entry(
      'b',
      key({
        configFiles: ['other.yaml'],
        configHash: hashFiles([['other.yaml', Buffer.from('x: 1\n')]]),
      }),
      2_000,
    );
    expect(findCachedGraph(dir, 'rulebearing.yaml')).toBe(mine);
    expect(findCachedGraph(dir, 'other.yaml')).toBe(
      join(dir, '.graph', 'cache', 'b', 'graph.json'),
    );
    expect(findCachedGraph(dir, 'absent.yaml')).toBeUndefined();
    expect(
      configAgrees(
        key({
          configFiles: [`${root}/rulebearing.yaml`],
          configHash: hashFiles([[`${root}/rulebearing.yaml`, Buffer.from(CONFIG)]]),
        }),
        root,
        undefined,
      ),
    ).toBe(true);
    expect(configAgrees(key({ configHash: 7 }), root, undefined)).toBe(false);
  });

  it('reads a graph once per modification and names what it cannot read', () => {
    const path = entry('a', key(), 1_000);
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
