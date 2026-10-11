// The link check over the built site: every relative `href`, `src` and `data-module` in every page
// must name a file the site holds and, with a fragment, an `id` on that page. External URLs are
// not fetched; the build makes no network request beyond the schema tag it was given.
//
// Architecture: docs/architecture.md#verification-strategy. Decisions: docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md,
// docs/adr/0023-documentation-link-and-lint-gates.md (the same rule, over the sources).
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3 ("A link checker runs in the build").
// Requirements: FR-REACH-01, NFR-DOC-01.

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join, posix, relative, sep } from 'node:path';

/** One link on a built page that resolves to nothing. */
export interface BrokenLink {
  /** The page, as a site path. */
  readonly page: string;
  /** The link as written. */
  readonly target: string;
  readonly reason: 'no such file' | 'no such anchor';
}

const ATTRIBUTE = /\s(?:href|src|data-module)="([^"]*)"/gu;
const ID = /\sid="([^"]*)"/gu;
const SCHEME = /^[a-z][a-z0-9+.-]*:/iu;

/** Undoes the escaping the layout applies to an attribute value. */
export function unescapeAttribute(value: string): string {
  return value
    .replaceAll('&quot;', '"')
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&amp;', '&');
}

/** Every file under `dir`, as `/`-separated paths relative to it, sorted. */
export function listFiles(dir: string): string[] {
  const found: string[] = [];
  const walk = (current: string): void => {
    for (const name of readdirSync(current)) {
      const full = join(current, name);
      if (statSync(full).isDirectory()) {
        walk(full);
      } else {
        found.push(relative(dir, full).split(sep).join('/'));
      }
    }
  };
  walk(dir);
  return found.sort();
}

/** The `id`s a page offers. */
export function idsOf(html: string): Set<string> {
  return new Set([...html.matchAll(ID)].map((m) => unescapeAttribute(m[1] ?? '')));
}

/** The links a page carries, as written. */
export function linksOf(html: string): string[] {
  return [...html.matchAll(ATTRIBUTE)].map((m) => unescapeAttribute(m[1] ?? ''));
}

/** Checks every page of the site built in `dir`. */
export function checkSite(dir: string): BrokenLink[] {
  const files = listFiles(dir);
  const pages = files.filter((f) => f.endsWith('.html'));
  const ids = new Map<string, Set<string>>();
  const idsFor = (page: string): Set<string> => {
    let found = ids.get(page);
    if (found === undefined) {
      found = idsOf(readFileSync(join(dir, page), 'utf8'));
      ids.set(page, found);
    }
    return found;
  };
  const broken: BrokenLink[] = [];
  for (const page of pages) {
    const html = readFileSync(join(dir, page), 'utf8');
    for (const target of linksOf(html)) {
      if (target === '' || target.startsWith('//') || SCHEME.test(target)) {
        continue;
      }
      const hash = target.indexOf('#');
      const pathPart = (hash === -1 ? target : target.slice(0, hash)).split('?')[0] ?? '';
      const fragment = hash === -1 ? '' : decodeURIComponent(target.slice(hash + 1));
      let file =
        pathPart === ''
          ? page
          : posix.normalize(posix.join(posix.dirname(page), decodeURIComponent(pathPart)));
      if (
        pathPart.endsWith('/') ||
        (existsSync(join(dir, file)) && statSync(join(dir, file)).isDirectory())
      ) {
        file = posix.join(file, 'index.html');
      }
      if (file.startsWith('..') || !existsSync(join(dir, file))) {
        broken.push({ page, target, reason: 'no such file' });
      } else if (fragment !== '' && file.endsWith('.html') && !idsFor(file).has(fragment)) {
        broken.push({ page, target, reason: 'no such anchor' });
      }
    }
  }
  return broken;
}
