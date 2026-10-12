// Which documents become pages, where they land, and how the sidebar groups them.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3; docs/adr/0039-plans-and-design-kept-local.md.
import { describe, expect, it } from 'vitest';
import {
  isLocalOnly,
  isPageSource,
  navGroups,
  pagePath,
  pageSources,
  relativeUrl,
} from '../src/pages.js';

describe('page sources', () => {
  it.each([
    ['README.md', true],
    ['CONTRIBUTING.md', true],
    ['docs/architecture.md', true],
    ['docs/adr/0001-record-architecture-decisions.md', true],
    ['schema/README.md', true],
    ['cookbook/README.md', true],
    ['cookbook/questions/domain-does-no-io/README.md', true],
    ['cookbook/guards/no-circular/README.md', false],
    ['docs/plans/pending/0004-wave-4-reach.md', false],
    ['docs/artifacts/design.md', false],
    ['docs/adr/README.txt', false],
    ['crates/rb-model/README.md', false],
    ['LICENSE', false],
  ])('%s is a page: %s', (path, expected) => {
    expect(isPageSource(path)).toBe(expected);
  });

  it('keeps local-only folders out, by path component', () => {
    expect(isLocalOnly('docs/plans')).toBe(true);
    expect(isLocalOnly('docs/plans/README.md')).toBe(true);
    expect(isLocalOnly('docs/plansx.md')).toBe(false);
    expect(isLocalOnly('docs/artifacts')).toBe(true);
  });

  it('sorts the pages', () => {
    expect(pageSources(['docs/b.md', 'README.md', 'docs/a.md', 'src/x.rs'])).toEqual([
      'README.md',
      'docs/a.md',
      'docs/b.md',
    ]);
  });
});

describe('page paths and URLs', () => {
  it.each([
    ['README.md', 'index.html'],
    ['docs/adr/README.md', 'docs/adr/index.html'],
    ['docs/cli.md', 'docs/cli.html'],
    ['SECURITY.md', 'SECURITY.html'],
  ])('%s -> %s', (source, path) => {
    expect(pagePath(source)).toBe(path);
  });

  it.each([
    ['docs/cli.html', 'docs/adr/index.html', 'adr/index.html'],
    ['docs/adr/0001.html', 'docs/cli.html', '../cli.html'],
    ['index.html', 'schema/v1.json', 'schema/v1.json'],
    ['docs/cli.html', 'docs/cli.html', 'cli.html'],
  ])('from %s to %s is %s', (from, to, url) => {
    expect(relativeUrl(from, to)).toBe(url);
  });
});

describe('the sidebar', () => {
  it('groups in a fixed order and drops empty groups', () => {
    const groups = navGroups([
      'README.md',
      'cookbook/README.md',
      'docs/adr/0001-x.md',
      'docs/adoption/2026-10-review.md',
      'docs/cli.md',
      'docs/reference/element-rules.md',
      'schema/README.md',
      'docs/other/page.md',
      'cookbook/questions/README.md',
      'cookbook/questions/domain-does-no-io/README.md',
    ]);
    expect(groups.map((g) => [g.title, g.sources])).toEqual([
      ['Start', ['README.md']],
      [
        'Cookbook',
        [
          'cookbook/README.md',
          'cookbook/questions/README.md',
          'cookbook/questions/domain-does-no-io/README.md',
        ],
      ],
      ['Guides', ['docs/cli.md']],
      ['Reference', ['docs/reference/element-rules.md', 'schema/README.md']],
      ['Adoption', ['docs/adoption/2026-10-review.md']],
      ['Decisions', ['docs/adr/0001-x.md']],
      ['More', ['docs/other/page.md']],
    ]);
    expect(navGroups(['docs/cli.md']).map((g) => g.title)).toEqual(['Guides']);
  });
});
