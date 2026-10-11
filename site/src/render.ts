// Renders one source document to the HTML body of its page: headings get the link check's anchors,
// links are rewritten for the site, Mermaid diagrams are shown as their source with a link to the
// rendered view on GitHub, and raw HTML is kept only where it is a comment or a line break.
//
// Architecture: docs/architecture.md#distribution. Decision: docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3. Requirements: FR-REACH-01, NFR-DOC-01.

import markdownIt from 'markdown-it';
import type { Env, MarkdownIt, Token } from 'markdown-it';
import { resolveLink } from './links.js';
import type { LinkContext } from './links.js';
import { slug } from './slug.js';

/** A page body and what was learned while rendering it. */
export interface Rendered {
  /** The page title: the text of the first heading, or the source path when there is none. */
  readonly title: string;
  /** The HTML of the document. */
  readonly html: string;
  /** Links that resolve to nothing, as `target: reason`. */
  readonly broken: readonly string[];
}

/** Raw HTML a document may carry through: a line break and an explicit anchor. */
const KEPT_HTML = /^(?:<br\s*\/?>|<a id="[\w.-]+"><\/a>|<a id="[\w.-]+">|<\/a>)$/u;

interface RenderEnv {
  [key: string | symbol]: unknown;
  source: string;
  context: LinkContext;
  folders: ReadonlySet<string>;
  broken: string[];
  title: string | undefined;
}

function isEnv(value: unknown): value is RenderEnv {
  return typeof value === 'object' && value !== null && 'source' in value && 'broken' in value;
}

/** The plain text of an inline token: its text and code spans, without the markup. */
function plainText(inline: Token): string {
  return (inline.children ?? [])
    .filter((t) => t.type === 'text' || t.type === 'code_inline')
    .map((t) => t.content)
    .join('');
}

function rewriteLinks(children: Token[], env: RenderEnv): void {
  const open: Token[] = [];
  for (const token of children) {
    if (token.type === 'link_open') {
      open.push(token);
      const href = token.attrGet('href');
      if (href === null) {
        continue;
      }
      const resolved = resolveLink(String(href), env.source, env.context, env.folders);
      if (resolved.kind === 'href') {
        token.attrSet('href', resolved.href);
      } else if (resolved.kind === 'local-only') {
        token.tag = 'span';
        token.attrs = [
          ['class', 'local-only'],
          ['title', `${resolved.path} is kept out of the published repository (ADR-0039)`],
        ];
      } else {
        env.broken.push(`${String(href)}: ${resolved.reason}`);
      }
    } else if (token.type === 'link_close') {
      const opener = open.pop();
      if (opener?.tag === 'span') {
        token.tag = 'span';
      }
    }
  }
}

/** Builds the Markdown renderer the site uses. */
export function createRenderer(): MarkdownIt {
  const md = markdownIt({ html: true, linkify: false, typographer: false });

  md.core.ruler.push('rulebearing_site', (state) => {
    const env: unknown = state.env;
    if (!isEnv(env)) {
      return;
    }
    const tokens = state.tokens;
    tokens.forEach((token, index) => {
      const next = tokens[index + 1];
      if (token.type === 'heading_open' && next?.type === 'inline') {
        token.attrSet('id', slug(next.content));
        env.title ??= plainText(next);
      }
      if (token.type === 'inline' && token.children !== null) {
        rewriteLinks(token.children, env);
      }
    });
  });

  const escape = md.utils.escapeHtml;
  const keepOrEscape = (content: string): string => {
    const trimmed = content.trim();
    if (trimmed.startsWith('<!--') && trimmed.endsWith('-->')) {
      return '';
    }
    return KEPT_HTML.test(trimmed) ? content : escape(content);
  };
  md.renderer.rules.html_block = (tokens, idx) => keepOrEscape(tokens[idx]?.content ?? '');
  md.renderer.rules.html_inline = (tokens, idx) => keepOrEscape(tokens[idx]?.content ?? '');

  const fence = md.renderer.rules.fence;
  if (fence === undefined) {
    throw new Error('markdown-it has no fence renderer to extend');
  }
  md.renderer.rules.fence = (tokens, idx, options, env: Env | undefined, self) => {
    const token = tokens[idx];
    if (token?.info.trim() === 'mermaid' && isEnv(env)) {
      const view = `${env.context.repoUrl}/blob/${env.context.commit}/${encodeURI(env.source)}`;
      return (
        '<figure class="diagram"><pre><code class="language-mermaid">' +
        escape(token.content) +
        '</code></pre><figcaption>A Mermaid diagram, shown as its source. ' +
        `<a href="${escape(view)}">The rendered diagram is on GitHub</a>.</figcaption></figure>\n`
      );
    }
    return fence(tokens, idx, options, env, self);
  };
  return md;
}

/** Renders the document at repository path `source` with text `markdown`. */
export function renderPage(
  md: MarkdownIt,
  source: string,
  markdown: string,
  context: LinkContext,
  folders: ReadonlySet<string>,
): Rendered {
  const env: RenderEnv = { source, context, folders, broken: [], title: undefined };
  const html = md.render(markdown, env);
  return { title: env.title ?? source, html, broken: env.broken };
}
