// The link check over a built site. Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3.
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { checkSite, idsOf, linksOf, listFiles, unescapeAttribute } from '../src/linkcheck.js';

let dir: string;

function write(path: string, text: string): void {
  const full = join(dir, path);
  mkdirSync(join(full, '..'), { recursive: true });
  writeFileSync(full, text);
}

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'rb-site-check-'));
});

afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
});

describe('checkSite', () => {
  it('accepts a site whose links and anchors all resolve', () => {
    write(
      'index.html',
      '<a href="docs/a.html#top">a</a><a href="docs/">d</a><a href="#here" id="here">h</a>',
    );
    write(
      'docs/a.html',
      '<h1 id="top">A</h1><a href="../index.html">home</a><a href="https://x.y/z#q">x</a>',
    );
    write('docs/index.html', '<link href="../assets/s.css"><script src="../assets/p.js"></script>');
    write('assets/s.css', '');
    write('assets/p.js', '');
    write(
      'p.html',
      '<section data-module="playground/m.js"></section><a href="schema/v1.json">s</a>',
    );
    write('playground/m.js', '');
    write('schema/v1.json', '{}');
    expect(checkSite(dir)).toEqual([]);
  });

  it('reports a missing file and a missing anchor', () => {
    write(
      'index.html',
      '<a href="gone.html">g</a><a href="a.html#nope">n</a><a href="../out.html">o</a>',
    );
    write('a.html', '<h1 id="yes">A</h1>');
    expect(checkSite(dir)).toEqual([
      { page: 'index.html', target: 'gone.html', reason: 'no such file' },
      { page: 'index.html', target: 'a.html#nope', reason: 'no such anchor' },
      { page: 'index.html', target: '../out.html', reason: 'no such file' },
    ]);
  });

  it('decodes escaped and percent-encoded targets', () => {
    write('index.html', '<a href="my%20page.html?x=1&amp;y=2#a%20b">p</a>');
    write('my page.html', '<h2 id="a b">x</h2>');
    expect(checkSite(dir)).toEqual([]);
  });
});

describe('the parts', () => {
  it('reads ids and links', () => {
    expect([...idsOf('<h1 id="a">x</h1><p id="b&amp;c">')]).toEqual(['a', 'b&c']);
    expect(linksOf('<a href="x">x</a> <img src="y"> <div data-module="z">')).toEqual([
      'x',
      'y',
      'z',
    ]);
    expect(unescapeAttribute('&lt;&gt;&quot;&amp;amp;')).toBe('<>"&amp;');
  });

  it('lists files sorted with / separators', () => {
    write('b/c.txt', '');
    write('a.txt', '');
    expect(listFiles(dir)).toEqual(['a.txt', 'b/c.txt']);
  });
});
