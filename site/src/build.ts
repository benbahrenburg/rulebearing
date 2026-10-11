// Builds the docs site: a page for every tracked document under the page roots, the playground
// with the wasm module built in the same run, the stylesheet and page scripts, and the released
// schemas; then the link check over the result.
//
// Architecture: docs/architecture.md#distribution. Decisions: docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md,
// docs/adr/0067-the-playground-facade-and-an-optional-javascript-engine.md, docs/adr/0039-plans-and-design-kept-local.md.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3. Requirements: FR-REACH-01, NFR-DOC-01.

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { checkSite } from './linkcheck.js';
import type { BrokenLink } from './linkcheck.js';
import { trackedFolders } from './links.js';
import type { LinkContext } from './links.js';
import { PLAYGROUND_PAGE, escapeHtml, layout, playgroundComponent } from './layout.js';
import type { SiteInfo } from './layout.js';
import { pagePath, pageSources, relativeUrl } from './pages.js';
import { createRenderer, renderPage } from './render.js';
import { SCHEMA_FILES, readSchemas } from './schemas.js';

/** The repository's GitHub URL. */
export const REPO_URL = 'https://github.com/benbahrenburg/rulebearing';

/** The guard fixture the playground page opens with: its CLI output is committed beside it. */
export const PLAYGROUND_FIXTURE = 'cookbook/guards/no-circular';

/** The files of `wasm-bindgen --target web` output the page loads. */
export const WASM_FILES: readonly string[] = ['rb_wasm.js', 'rb_wasm_bg.wasm'];

/** The compiled page scripts, from `tsc -p site/web`. */
export const WEB_FILES: readonly string[] = ['playground.js', 'playground-core.js'];

/** What one build needs. */
export interface BuildOptions {
  /** The repository root. */
  readonly repo: string;
  /** The folder the site is written to; emptied first. */
  readonly out: string;
  /** The `wasm-bindgen --target web` output folder holding {@link WASM_FILES}. */
  readonly wasmDir: string;
  /** The compiled page scripts' folder holding {@link WEB_FILES}. */
  readonly webDir: string;
  /** The release tag the schemas are read from. */
  readonly schemaRef: string;
  /** The commit the site is built from; `HEAD` when absent. */
  readonly commit?: string;
}

/** What a build produced. */
export interface BuildResult {
  /** The site paths of the pages written, sorted. */
  readonly pages: readonly string[];
  /** Links in the sources that resolve to nothing. */
  readonly brokenSources: readonly string[];
  /** Links in the built site that resolve to nothing. */
  readonly brokenSite: readonly BrokenLink[];
}

function git(repo: string, args: readonly string[]): string {
  return execFileSync('git', args, { cwd: repo, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
}

function write(out: string, path: string, text: string): void {
  const full = join(out, path);
  mkdirSync(dirname(full), { recursive: true });
  writeFileSync(full, text);
}

function copy(from: string, out: string, path: string): void {
  if (!existsSync(from)) {
    throw new Error(`${from} is missing; it is built before the site (site/build.sh)`);
  }
  const full = join(out, path);
  mkdirSync(dirname(full), { recursive: true });
  copyFileSync(from, full);
}

/** The body of the playground page. */
export function playgroundBody(repo: string): string {
  const fixture = join(repo, PLAYGROUND_FIXTURE);
  const config = readFileSync(join(fixture, 'rulebearing.yaml'), 'utf8');
  const graph = readFileSync(join(fixture, 'graph.json'), 'utf8');
  const guide = relativeUrl(PLAYGROUND_PAGE, 'cookbook/index.html');
  return `<h1 id="playground">Playground</h1>
<p>Try a rule set before installing anything. The page opens with the <code>no-circular</code> guard from the <a href="${escapeHtml(guide)}">guard catalogue</a> and the cruise result of its fixture; <strong>Evaluate</strong> runs the rules and prints the report the command line prints for <code>rulebearing cruise --graph graph.json -T &lt;output type&gt;</code>.</p>
${playgroundComponent(PLAYGROUND_PAGE, { config, graph, outputType: 'err-long' })}`;
}

/** Builds the site and checks its links; the caller decides what a broken link means. */
export function buildSite(options: BuildOptions): BuildResult {
  const { repo, out } = options;
  const commit = options.commit ?? git(repo, ['rev-parse', 'HEAD']).trim();
  const tracked = git(repo, ['ls-files', '-z'])
    .split('\0')
    .filter((f) => f !== '');
  const sources = pageSources(tracked);
  const schemas = readSchemas(repo, options.schemaRef);

  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });

  const served = new Map(SCHEMA_FILES.map((name) => [`schema/${name}`, `schema/${name}`]));
  const context: LinkContext = {
    pages: new Set(sources),
    tracked: new Set(tracked),
    served,
    repoUrl: REPO_URL,
    commit,
  };
  const folders = trackedFolders(tracked);
  const md = createRenderer();
  const rendered = sources.map((source) => ({
    source,
    page: renderPage(md, source, readFileSync(join(repo, source), 'utf8'), context, folders),
  }));

  const titles = new Map(rendered.map(({ source, page }) => [source, page.title]));
  const site: SiteInfo = {
    sources,
    titles,
    repoUrl: REPO_URL,
    commit,
    schemaRef: options.schemaRef,
  };
  const brokenSources: string[] = [];
  const pages: string[] = [];
  for (const { source, page } of rendered) {
    brokenSources.push(...page.broken.map((b) => `${source}: ${b}`));
    const path = pagePath(source);
    write(out, path, layout(site, { path, title: page.title, body: page.html, source }));
    pages.push(path);
  }

  write(
    out,
    PLAYGROUND_PAGE,
    layout(site, {
      path: PLAYGROUND_PAGE,
      title: 'Playground',
      body: playgroundBody(repo),
      scripts: ['assets/playground.js'],
    }),
  );
  pages.push(PLAYGROUND_PAGE);

  for (const [name, text] of schemas) {
    write(out, `schema/${name}`, text);
  }
  copy(join(repo, 'site/web/site.css'), out, 'assets/site.css');
  for (const file of WEB_FILES) {
    copy(join(options.webDir, file), out, `assets/${file}`);
  }
  for (const file of WASM_FILES) {
    copy(join(options.wasmDir, file), out, `playground/${file}`);
  }

  return { pages: pages.sort(), brokenSources, brokenSite: checkSite(out) };
}
