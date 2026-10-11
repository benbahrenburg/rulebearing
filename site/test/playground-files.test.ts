// The playground's file gathering: the map of paths to source text built from a folder pick, a
// file pick or a drop; relative paths, normalisation, the binary and size filters, the language
// inference and the note. Files are plain objects shaped like the DOM's `File`.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3. Decision: docs/adr/0073-extractors-read-files-through-rb-model.md.
import { describe, expect, it } from 'vitest';
import {
  CONFIG_FILES,
  LIMITS,
  MAX_FILE_BYTES,
  configIn,
  countSources,
  describeFiles,
  inferLanguage,
  normalisePath,
  pickedPath,
  readDropped,
  readFiles,
  readPicked,
  relativeTo,
  textOf,
  walkDropped,
} from '../web/playground-files.js';
import type {
  DroppedEntry,
  DroppedFile,
  DroppedFolder,
  PickedFile,
  Skipped,
} from '../web/playground-files.js';

function file(
  name: string,
  content: string | Uint8Array,
  relative = '',
  size?: number,
): PickedFile {
  const bytes = typeof content === 'string' ? new TextEncoder().encode(content) : content;
  return {
    name,
    webkitRelativePath: relative,
    size: size ?? bytes.length,
    arrayBuffer: () => Promise.resolve(bytes.slice().buffer),
  };
}

function droppedFile(fullPath: string, content: string): DroppedFile {
  return {
    isFile: true,
    isDirectory: false,
    fullPath,
    file: (done) => {
      done(file(fullPath.split('/').pop() ?? '', content));
    },
  };
}

/** A dropped folder whose reader hands its entries over two at a time, as browsers batch them. */
function droppedFolder(fullPath: string, entries: DroppedEntry[]): DroppedFolder {
  return {
    isFile: false,
    isDirectory: true,
    fullPath,
    createReader: () => {
      let next = 0;
      return {
        readEntries: (done) => {
          done(entries.slice(next, next + 2));
          next += 2;
        },
      };
    },
  };
}

describe('normalisePath', () => {
  it.each([
    ['src/a.ts', 'src/a.ts'],
    ['./src/a.ts', 'src/a.ts'],
    ['/repo/src/a.ts', 'repo/src/a.ts'],
    ['src\\ui\\view.ts', 'src/ui/view.ts'],
    ['src//./a.ts', 'src/a.ts'],
    ['../a.ts', '../a.ts'],
    ['', ''],
  ])('%s becomes %s', (input, expected) => {
    expect(normalisePath(input)).toBe(expected);
  });

  it('is idempotent and never leaves a leading, trailing or doubled slash', () => {
    const parts = ['a', '.', '..', '', 'b.ts', '\\', '/', 'src'];
    let seed = 7;
    const pick = (): string => {
      seed = (seed * 1103515245 + 12345) % 2147483648;
      return parts[seed % parts.length] ?? '';
    };
    for (let i = 0; i < 500; i += 1) {
      const path = Array.from({ length: 1 + (i % 7) }, pick).join(i % 2 === 0 ? '/' : '');
      const once = normalisePath(path);
      expect(normalisePath(once)).toBe(once);
      expect(once.startsWith('/') || once.endsWith('/') || once.includes('//')).toBe(false);
      expect(once.split('/')).not.toContain('.');
    }
  });
});

describe('relative paths', () => {
  it("takes a folder pick's path, or a file's name", () => {
    expect(pickedPath(file('a.ts', '', 'repo/src/a.ts'))).toBe('repo/src/a.ts');
    expect(pickedPath(file('a.ts', ''))).toBe('a.ts');
  });

  it('removes the one folder every path starts with', () => {
    const f = file('x', '');
    const found = relativeTo([
      { path: 'repo/src/a.ts', file: f },
      { path: '/repo/package.json', file: f },
    ]);
    expect(found.map((x) => x.path)).toEqual(['src/a.ts', 'package.json']);
  });

  it('keeps paths that do not share one folder', () => {
    const f = file('x', '');
    expect(
      relativeTo([
        { path: 'a/x.ts', file: f },
        { path: 'b/y.ts', file: f },
      ]).map((x) => x.path),
    ).toEqual(['a/x.ts', 'b/y.ts']);
    expect(
      relativeTo([
        { path: 'a.ts', file: f },
        { path: 'b.ts', file: f },
      ]).map((x) => x.path),
    ).toEqual(['a.ts', 'b.ts']);
    expect(relativeTo([])).toEqual([]);
  });
});

describe('textOf', () => {
  it('reads UTF-8 text and refuses binary', () => {
    expect(textOf(new TextEncoder().encode('é = 1'))).toBe('é = 1');
    expect(textOf(new Uint8Array([0x61, 0x00, 0x62]))).toBeUndefined();
    expect(textOf(new Uint8Array([0xff, 0xfe, 0x41]))).toBeUndefined();
  });
});

describe('reading a pick', () => {
  it('builds the map in path order, relative to the chosen folder', async () => {
    const set = await readPicked([
      file('b.ts', 'b', 'repo/src/b.ts'),
      file('a.ts', 'a', 'repo/src/a.ts'),
      file('package.json', '{}', 'repo/package.json'),
    ]);
    expect([...set.files]).toEqual([
      ['package.json', '{}'],
      ['src/a.ts', 'a'],
      ['src/b.ts', 'b'],
    ]);
    expect(set.skipped).toEqual([]);
    expect(set.bytes).toBe(4);
  });

  it('skips binary files, large files, version control and what is past the limits', async () => {
    const limits = { fileBytes: 10, totalBytes: 12, files: 3 };
    const set = await readFiles(
      [
        { path: '.git/HEAD', file: file('HEAD', 'ref') },
        { path: 'a.ts', file: file('a.ts', '12345') },
        { path: 'b.png', file: file('b.png', new Uint8Array([0x89, 0x50, 0x00])) },
        { path: 'c.ts', file: file('c.ts', '12345678901') },
        { path: 'd.ts', file: file('d.ts', '12345') },
        { path: 'e.ts', file: file('e.ts', '123') },
        { path: 'f.ts', file: file('f.ts', '1') },
        { path: 'src/.svn/x', file: file('x', '1') },
      ],
      limits,
    );
    expect([...set.files.keys()]).toEqual(['a.ts', 'd.ts', 'f.ts']);
    expect(set.skipped).toEqual<Skipped[]>([
      { path: '.git/HEAD', reason: 'version control' },
      { path: 'b.png', reason: 'binary' },
      { path: 'c.ts', reason: 'too large' },
      { path: 'e.ts', reason: 'over the total limit' },
      { path: 'src/.svn/x', reason: 'version control' },
    ]);
    expect(set.bytes).toBe(11);
  });

  it('stops at the file count', async () => {
    const set = await readFiles(
      [
        { path: 'a.ts', file: file('a.ts', '') },
        { path: 'b.ts', file: file('b.ts', '') },
      ],
      { ...LIMITS, files: 1 },
    );
    expect([...set.files.keys()]).toEqual(['a.ts']);
    expect(set.skipped).toEqual([{ path: 'b.ts', reason: 'over the total limit' }]);
  });

  it('skips a file over the size limit without reading it', async () => {
    const huge: PickedFile = {
      name: 'big.js',
      webkitRelativePath: '',
      size: MAX_FILE_BYTES + 1,
      arrayBuffer: () => Promise.reject(new Error('read')),
    };
    const set = await readPicked([huge]);
    expect(set.skipped).toEqual([{ path: 'big.js', reason: 'too large' }]);
  });
});

describe('reading a drop', () => {
  const tree = (): DroppedEntry[] => [
    droppedFolder('/repo', [
      droppedFile('/repo/package.json', '{}'),
      droppedFolder('/repo/src', [
        droppedFile('/repo/src/a.ts', 'a'),
        droppedFile('/repo/src/b.ts', 'b'),
        droppedFile('/repo/src/c.ts', 'c'),
      ]),
      droppedFolder('/repo/.git', [droppedFile('/repo/.git/HEAD', 'ref')]),
    ]),
  ];

  it('walks folders, batch by batch, and skips version control', async () => {
    const found = await walkDropped(tree());
    expect(found.map((f) => f.path).sort()).toEqual([
      'repo/package.json',
      'repo/src/a.ts',
      'repo/src/b.ts',
      'repo/src/c.ts',
    ]);
  });

  it('builds the map relative to the dropped folder', async () => {
    const set = await readDropped(tree());
    expect([...set.files.keys()]).toEqual(['package.json', 'src/a.ts', 'src/b.ts', 'src/c.ts']);
  });

  it('keeps dropped files by their names, and ignores an entry that is neither', async () => {
    const odd: DroppedEntry = { isFile: false, isDirectory: false, fullPath: '/odd' };
    const set = await readDropped([droppedFile('/a.py', 'x'), droppedFile('/b.py', 'y'), odd]);
    expect([...set.files.keys()]).toEqual(['a.py', 'b.py']);
  });

  it('passes on a read the browser failed', async () => {
    const failing: DroppedFile = {
      isFile: true,
      isDirectory: false,
      fullPath: '/a.ts',
      file: (_done, fail) => {
        fail(new Error('NotReadableError'));
      },
    };
    await expect(readDropped([failing])).rejects.toThrow('NotReadableError');
  });
});

describe('the language', () => {
  it('counts the project sources, not installed packages', () => {
    expect(
      countSources([
        'src/a.ts',
        'src/b.tsx',
        'src/c.mjs',
        'src/d.cjs',
        'src/e.jsx',
        'src/f.mts',
        'node_modules/lodash/index.js',
        'src/g.py',
        'stubs/h.pyi',
        '.venv/lib/python3.12/site-packages/requests/__init__.py',
        'venv/x.py',
        'README.md',
      ]),
    ).toEqual({ typescript: 6, python: 2 });
  });

  it('infers the larger, and asks on a tie', () => {
    expect(inferLanguage(['a.ts', 'b.ts', 'c.py'])).toBe('typescript');
    expect(inferLanguage(['a.ts', 'b.py', 'c.py'])).toBe('python');
    expect(() => inferLanguage(['a.ts', 'b.py'])).toThrow(
      'the language cannot be inferred: the files hold 1 TypeScript or JavaScript and 1 Python source(s); pick the language',
    );
    expect(() => inferLanguage([])).toThrow(/0 TypeScript or JavaScript and 0 Python/u);
  });
});

describe('the configuration file', () => {
  it('is the first one at the root, in order of preference', () => {
    expect(CONFIG_FILES[0]).toBe('rulebearing.yaml');
    expect(
      configIn(
        new Map([
          ['rulebearing.json', ''],
          ['.dependency-cruiser.json', ''],
        ]),
      ),
    ).toBe('rulebearing.json');
    expect(configIn(new Map([['src/rulebearing.yaml', '']]))).toBeUndefined();
  });
});

describe('describeFiles', () => {
  it('says what was read and names the first ten skipped', () => {
    const skipped = Array.from({ length: 12 }, (_, i): Skipped => ({
      path: `f${String(i)}.bin`,
      reason: 'binary',
    }));
    const note = describeFiles({ files: new Map([['a', 'x']]), skipped, bytes: 2048 });
    expect(note).toBe(
      `1 file(s) read, 2.0 KiB, kept in this browser. Skipped 12: ${skipped
        .slice(0, 10)
        .map((s) => `${s.path} (binary)`)
        .join(', ')}, and 2 more.`,
    );
    expect(describeFiles({ files: new Map(), skipped: [], bytes: 0 })).toBe(
      '0 file(s) read, 0.0 KiB, kept in this browser.',
    );
  });
});
