// Rewrites a relative link in a source document into the link the built page carries: a page of
// the site, a file the site serves (the schemas), the file on GitHub at the commit the site was
// built from, or plain text for a link into a folder that is kept local and never published.
//
// Architecture: docs/architecture.md#distribution. Decisions: docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md,
// docs/adr/0023-documentation-link-and-lint-gates.md (the sources' links are checked there),
// docs/adr/0039-plans-and-design-kept-local.md. Plan: docs/plans/pending/0004-wave-4-reach.md,
// Step 3. Requirements: FR-REACH-01, NFR-DOC-01.

import { posix } from 'node:path';
import { isLocalOnly, pagePath, relativeUrl } from './pages.js';

/** What the resolver knows about the repository and the site being built. */
export interface LinkContext {
  /** Repository paths of every page source. */
  readonly pages: ReadonlySet<string>;
  /** Repository paths of every tracked file. */
  readonly tracked: ReadonlySet<string>;
  /** Repository paths the site serves as files, mapped to their site path. */
  readonly served: ReadonlyMap<string, string>;
  /** The repository's GitHub URL, without a trailing `/`. */
  readonly repoUrl: string;
  /** The commit the site is built from. */
  readonly commit: string;
}

/** How one link is written on the built page. */
export type Resolved =
  | { readonly kind: 'href'; readonly href: string }
  | { readonly kind: 'local-only'; readonly path: string }
  | { readonly kind: 'broken'; readonly reason: string };

const SCHEME = /^[a-z][a-z0-9+.-]*:/iu;

/** The folders that hold at least one tracked file, derived once per build. */
export function trackedFolders(tracked: Iterable<string>): Set<string> {
  const folders = new Set<string>();
  for (const file of tracked) {
    let dir = posix.dirname(file);
    while (dir !== '.' && !folders.has(dir)) {
      folders.add(dir);
      dir = posix.dirname(dir);
    }
  }
  return folders;
}

/**
 * Resolves `href`, written in the page built from `source`. Absolute URLs and in-page fragments
 * are kept as written; every other link is resolved against the repository.
 */
export function resolveLink(
  href: string,
  source: string,
  context: LinkContext,
  folders: ReadonlySet<string>,
): Resolved {
  if (href === '' || href.startsWith('#') || href.startsWith('//') || SCHEME.test(href)) {
    return { kind: 'href', href };
  }
  const hash = href.indexOf('#');
  const rawPath = hash === -1 ? href : href.slice(0, hash);
  const fragment = hash === -1 ? '' : href.slice(hash);
  let decoded: string;
  try {
    decoded = decodeURIComponent(rawPath);
  } catch {
    return { kind: 'broken', reason: `not a valid URL path: ${rawPath}` };
  }
  const joined = posix.normalize(posix.join(posix.dirname(source), decoded));
  const target = joined === '.' || joined === './' ? '' : joined.replace(/\/$/u, '');
  if (target.startsWith('..')) {
    return { kind: 'broken', reason: `climbs out of the repository: ${href}` };
  }
  if (isLocalOnly(target)) {
    return { kind: 'local-only', path: target };
  }
  const from = pagePath(source);
  const readme = target === '' ? 'README.md' : `${target}/README.md`;
  if (context.pages.has(target)) {
    return { kind: 'href', href: encodeURI(relativeUrl(from, pagePath(target))) + fragment };
  }
  if (context.pages.has(readme)) {
    return { kind: 'href', href: encodeURI(relativeUrl(from, pagePath(readme))) + fragment };
  }
  const served = context.served.get(target);
  if (served !== undefined) {
    return { kind: 'href', href: encodeURI(relativeUrl(from, served)) + fragment };
  }
  if (context.tracked.has(target)) {
    return {
      kind: 'href',
      href: `${context.repoUrl}/blob/${context.commit}/${encodeURI(target)}${fragment}`,
    };
  }
  if (target === '' || folders.has(target)) {
    const suffix = target === '' ? '' : `/${encodeURI(target)}`;
    return { kind: 'href', href: `${context.repoUrl}/tree/${context.commit}${suffix}${fragment}` };
  }
  return { kind: 'broken', reason: `no tracked file or folder ${target}` };
}
