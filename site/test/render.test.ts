// A document rendered for the site: anchors, links, diagrams and raw HTML.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3.
import { describe, expect, it } from 'vitest';
import { trackedFolders } from '../src/links.js';
import type { LinkContext } from '../src/links.js';
import { createRenderer, renderPage } from '../src/render.js';

const context: LinkContext = {
  pages: new Set(['docs/a.md', 'docs/b.md']),
  tracked: new Set(['docs/a.md', 'docs/b.md', 'LICENSE']),
  served: new Map(),
  repoUrl: 'https://github.com/o/r',
  commit: 'abc',
};
const md = createRenderer();
const render = (text: string) =>
  renderPage(md, 'docs/a.md', text, context, trackedFolders(context.tracked));

describe('renderPage', () => {
  it('gives headings the link check anchors and takes the first as the title', () => {
    const page = render('# The `rb-model` crate\n\n## 1.6 Interfaces, frozen\n');
    expect(page.title).toBe('The rb-model crate');
    expect(page.html).toContain('<h1 id="the-rb-model-crate">');
    expect(page.html).toContain('<h2 id="16-interfaces-frozen">');
  });

  it('falls back to the source path when there is no heading', () => {
    expect(render('text only\n').title).toBe('docs/a.md');
  });

  it('rewrites links and shows local-only ones as text', () => {
    const page = render(
      '[b](b.md#x) [lic](../LICENSE) [plan](plans/pending/p.md#s) [web](https://e.x)\n',
    );
    expect(page.html).toContain('<a href="b.html#x">b</a>');
    expect(page.html).toContain('<a href="https://github.com/o/r/blob/abc/LICENSE">lic</a>');
    expect(page.html).toContain(
      '<span class="local-only" title="docs/plans/pending/p.md is kept out of the published repository (ADR-0039)">plan</span>',
    );
    expect(page.html).toContain('<a href="https://e.x">web</a>');
    expect(page.broken).toEqual([]);
  });

  it('reports a link that resolves to nothing', () => {
    expect(render('[gone](gone.md)\n').broken).toEqual([
      'gone.md: no tracked file or folder docs/gone.md',
    ]);
  });

  it('shows a Mermaid diagram as its source, linked to GitHub', () => {
    const page = render('```mermaid\nflowchart LR\n  a --> b\n```\n');
    expect(page.html).toContain(
      '<code class="language-mermaid">flowchart LR\n  a --&gt; b\n</code>',
    );
    expect(page.html).toContain('href="https://github.com/o/r/blob/abc/docs/a.md"');
  });

  it('renders other fences as code', () => {
    expect(render('```yaml\na: 1\n```\n').html).toContain(
      '<code class="language-yaml">a: 1\n</code>',
    );
  });

  it('drops comments, keeps line breaks and anchors, and escapes other HTML', () => {
    const page = render(
      '<!-- marker -->\n\nUse <version> here.<br>\n\n<a id="manual"></a>\n\n<script>x()</script>\n',
    );
    expect(page.html).not.toContain('marker');
    expect(page.html).toContain('Use &lt;version&gt; here.<br>');
    expect(page.html).toContain('<a id="manual"></a>');
    expect(page.html).toContain('&lt;script&gt;x()&lt;/script&gt;');
  });

  it('renders tables', () => {
    expect(render('| a | b |\n| --- | --- |\n| 1 | 2 |\n').html).toContain('<table>');
  });
});
