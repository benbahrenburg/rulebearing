// The theme: the page frame, the content security policy and the playground component.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3, § 1.8 and § 1.11.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import {
  CSP,
  PLAYGROUND_OUTPUT_TYPES,
  escapeHtml,
  layout,
  playgroundComponent,
} from '../src/layout.js';
import type { SiteInfo } from '../src/layout.js';

const site: SiteInfo = {
  sources: ['README.md', 'docs/cli.md', 'docs/adr/0001-x.md'],
  titles: new Map([
    ['README.md', 'Rulebearing'],
    ['docs/cli.md', 'The <CLI>'],
  ]),
  repoUrl: 'https://github.com/o/r',
  commit: '0123456789abcdef',
  schemaRef: 'v1.2.3',
};

describe('layout', () => {
  const html = layout(site, {
    path: 'docs/cli.html',
    title: 'The <CLI>',
    body: '<p>body</p>',
    source: 'docs/cli.md',
    scripts: ['assets/playground.js'],
  });

  it('carries the content security policy, with no inline script allowed', () => {
    expect(html).toContain(`<meta http-equiv="Content-Security-Policy" content="${CSP}">`);
    expect(CSP).toContain("script-src 'self' 'wasm-unsafe-eval'");
    expect(CSP).not.toContain('unsafe-inline');
    expect(html).not.toMatch(/<script(?![^>]*\ssrc=)[^>]*>/u);
  });

  it('links relative to the page and marks the current page', () => {
    expect(html).toContain('<link rel="stylesheet" href="../assets/site.css">');
    expect(html).toContain('<script type="module" src="../assets/playground.js"></script>');
    expect(html).toContain('<a href="cli.html" aria-current="page">The &lt;CLI&gt;</a>');
    expect(html).toContain('<a href="../index.html">Rulebearing</a>');
    expect(html).toContain('<details open><summary>Guides</summary>');
    expect(html).toContain('<details><summary>Decisions</summary>');
    expect(html).toContain('>docs/adr/0001-x.md</a>');
  });

  it('names the commit, the schema release and the source', () => {
    expect(html).toContain('<code>0123456789ab</code>');
    expect(html).toContain('release <code>v1.2.3</code>');
    expect(html).toContain('href="https://github.com/o/r/blob/0123456789abcdef/docs/cli.md"');
  });

  it('omits the source link for a generated page', () => {
    expect(layout(site, { path: 'playground.html', title: 'P', body: '' })).not.toContain(
      'View the source',
    );
  });
});

describe('the playground component', () => {
  const html = playgroundComponent('cookbook/x.html', {
    config: 'a: "<b>"',
    graph: '{}',
    outputType: 'json',
  });

  it('loads the module and links the ADR relative to its page', () => {
    expect(html).toContain('data-module="../playground/rb_wasm.js"');
    expect(html).toContain(
      'href="../docs/adr/0067-the-playground-facade-and-an-optional-javascript-engine.html"',
    );
  });

  it('is preloaded, escaped, and starts disabled until the module loads', () => {
    expect(html).toContain('a: &quot;&lt;b&gt;&quot;</textarea>');
    expect(html).toContain('<option value="json" selected>json</option>');
    expect(html).toContain('<button type="button" data-run disabled>');
  });

  it('says which inputs it cannot take', () => {
    expect(html).toContain('A JavaScript configuration is not accepted');
    expect(html).toContain('.NET assemblies are not accepted');
  });

  it("offers every output type rb-report has, except x-dot-webpage, which needs GraphViz's dot", () => {
    const source = readFileSync(
      resolve(import.meta.dirname, '../../crates/rb-report/src/lib.rs'),
      'utf8',
    );
    const table =
      /pub const OUTPUT_TYPES: &\[\(&str, u8\)\] = &\[([^\]]*)\];/u.exec(source)?.[1] ?? '';
    const names = [...table.matchAll(/\("([^"]+)", \d+\)/gu)].map((m) => m[1]);
    expect(names.length).toBeGreaterThan(20);
    expect(PLAYGROUND_OUTPUT_TYPES).toEqual(names.filter((n) => n !== 'x-dot-webpage'));
  });
});

describe('escapeHtml', () => {
  it('escapes the four characters that matter in text and attributes', () => {
    expect(escapeHtml('<a href="x">&</a>')).toBe('&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;');
  });
});
