// How a relative link in a source document is written on the built page.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3; docs/adr/0039-plans-and-design-kept-local.md.
import { describe, expect, it } from 'vitest';
import { resolveLink, trackedFolders } from '../src/links.js';
import type { LinkContext } from '../src/links.js';

const tracked = [
  'README.md',
  'LICENSE',
  'docs/cli.md',
  'docs/adr/README.md',
  'docs/adr/0001-x.md',
  'schema/v1.json',
  'crates/rb-model/src/lib.rs',
  'docs/my file.md',
];
const context: LinkContext = {
  pages: new Set([
    'README.md',
    'docs/cli.md',
    'docs/adr/README.md',
    'docs/adr/0001-x.md',
    'docs/my file.md',
  ]),
  tracked: new Set(tracked),
  served: new Map([['schema/v1.json', 'schema/v1.json']]),
  repoUrl: 'https://github.com/o/r',
  commit: 'abc',
};
const folders = trackedFolders(tracked);
const resolve = (href: string, source = 'docs/cli.md') =>
  resolveLink(href, source, context, folders);

describe('resolveLink', () => {
  it.each([
    ['https://example.com/x', 'https://example.com/x'],
    ['mailto:a@b.c', 'mailto:a@b.c'],
    ['#section', '#section'],
    ['//cdn.example/x', '//cdn.example/x'],
    ['', ''],
  ])('keeps %s', (href, expected) => {
    expect(resolve(href)).toEqual({ kind: 'href', href: expected });
  });

  it.each([
    ['adr/0001-x.md#context', 'adr/0001-x.html#context'],
    ['adr/', 'adr/index.html'],
    ['adr', 'adr/index.html'],
    ['../README.md', '../index.html'],
    ['../', '../index.html'],
    ['../schema/v1.json', '../schema/v1.json'],
    ['./cli.md#top', 'cli.html#top'],
    ['my%20file.md', 'my%20file.html'],
  ])('rewrites %s to the site page %s', (href, expected) => {
    expect(resolve(href)).toEqual({ kind: 'href', href: expected });
  });

  it('points a tracked file that is not a page at GitHub, at the commit', () => {
    expect(resolve('../LICENSE')).toEqual({
      kind: 'href',
      href: 'https://github.com/o/r/blob/abc/LICENSE',
    });
    expect(resolve('../crates/rb-model/src/lib.rs#L1')).toEqual({
      kind: 'href',
      href: 'https://github.com/o/r/blob/abc/crates/rb-model/src/lib.rs#L1',
    });
    expect(resolve('../crates/rb-model/')).toEqual({
      kind: 'href',
      href: 'https://github.com/o/r/tree/abc/crates/rb-model',
    });
  });

  it('shows a link into a local-only folder as text', () => {
    expect(resolve('plans/pending/0004.md#x')).toEqual({
      kind: 'local-only',
      path: 'docs/plans/pending/0004.md',
    });
    expect(resolve('artifacts/')).toEqual({ kind: 'local-only', path: 'docs/artifacts' });
  });

  it('reports what resolves to nothing', () => {
    expect(resolve('missing.md').kind).toBe('broken');
    expect(resolve('../../outside.md').kind).toBe('broken');
    expect(resolve('%E0%A4%A.md').kind).toBe('broken');
  });

  it('lists every folder that holds a tracked file', () => {
    expect([...trackedFolders(['a/b/c.md', 'a/d.md', 'e.md'])].sort()).toEqual(['a', 'a/b']);
  });

  it('resolves from the root README', () => {
    expect(resolve('docs/cli.md', 'README.md')).toEqual({ kind: 'href', href: 'docs/cli.html' });
    expect(resolve('.', 'README.md')).toEqual({ kind: 'href', href: 'index.html' });
  });
});
