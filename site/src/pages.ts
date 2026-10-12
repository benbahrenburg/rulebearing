// Which tracked Markdown files become pages of the docs site, and where each one lands. The pages
// are generated at build time from the repository's own documents; nothing is copied by hand.
//
// Architecture: docs/architecture.md#distribution. Decision: docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md;
// docs/adr/0039-plans-and-design-kept-local.md (the plans and the exported design are never published).
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3. Requirements: FR-REACH-01, NFR-DOC-01.

import { posix } from 'node:path';

/** Repository paths (files, or folders ending in `/`) whose tracked Markdown becomes pages. */
export const PAGE_ROOTS: readonly string[] = [
  'README.md',
  'CONTRIBUTING.md',
  'SECURITY.md',
  'docs/',
  'schema/README.md',
  'cookbook/README.md',
  'cookbook/questions/',
];

/**
 * Folders kept on a developer's machine and out of git (ADR-0039). The site never renders a file
 * under one, even when the build runs in a checkout that has them, and a link into one is shown as
 * text: the published site has no copy to point at.
 */
export const LOCAL_ONLY_ROOTS: readonly string[] = ['docs/plans/', 'docs/artifacts/'];

/** True when `path` (repository-relative, `/`-separated) is under a local-only folder. */
export function isLocalOnly(path: string): boolean {
  return LOCAL_ONLY_ROOTS.some((root) => path === root.slice(0, -1) || path.startsWith(root));
}

/** True when the tracked file `path` becomes a page. */
export function isPageSource(path: string): boolean {
  if (!path.endsWith('.md') || isLocalOnly(path)) {
    return false;
  }
  return PAGE_ROOTS.some((root) => (root.endsWith('/') ? path.startsWith(root) : path === root));
}

/** The pages among the tracked files, sorted so every build lists them in the same order. */
export function pageSources(tracked: readonly string[]): string[] {
  return tracked.filter(isPageSource).sort();
}

/**
 * The site path of the page built from `source`: `README.md` becomes its folder's `index.html`,
 * any other `name.md` becomes `name.html` in the same folder.
 */
export function pagePath(source: string): string {
  const dir = posix.dirname(source);
  const base = posix.basename(source, '.md');
  const file = base === 'README' ? 'index.html' : `${base}.html`;
  return dir === '.' ? file : `${dir}/${file}`;
}

/** The relative URL from the page at site path `from` to the site path `to`. */
export function relativeUrl(from: string, to: string): string {
  const rel = posix.relative(posix.dirname(from), to);
  return rel === '' ? posix.basename(to) : rel;
}

/** One navigation group of the site's sidebar. */
export interface NavGroup {
  readonly title: string;
  readonly sources: readonly string[];
}

/** Groups the page sources for the sidebar, in a fixed order; a group with no page is dropped. */
export function navGroups(sources: readonly string[]): NavGroup[] {
  const groups: { title: string; test: (s: string) => boolean }[] = [
    { title: 'Start', test: (s) => !s.includes('/') },
    { title: 'Cookbook', test: (s) => s.startsWith('cookbook/') },
    { title: 'Guides', test: (s) => /^docs\/[^/]+\.md$/u.test(s) },
    { title: 'Reference', test: (s) => s.startsWith('docs/reference/') || s.startsWith('schema/') },
    { title: 'Adoption', test: (s) => s.startsWith('docs/adoption/') },
    { title: 'Decisions', test: (s) => s.startsWith('docs/adr/') },
  ];
  const placed = new Set<string>();
  const result: NavGroup[] = [];
  for (const group of groups) {
    const members = sources.filter((s) => !placed.has(s) && group.test(s));
    for (const m of members) {
      placed.add(m);
    }
    if (members.length > 0) {
      result.push({ title: group.title, sources: members });
    }
  }
  const rest = sources.filter((s) => !placed.has(s));
  if (rest.length > 0) {
    result.push({ title: 'More', sources: rest });
  }
  return result;
}
