// Builds the docs site: a page for every tracked document under the page roots, the playground
// with the wasm module built in the same run, the stylesheet and page scripts, and the released
// schemas; then the link check over the result.
//
// Architecture: docs/architecture.md#distribution. Decisions: docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md,
// docs/adr/0067-the-playground-facade-and-an-optional-javascript-engine.md, docs/adr/0039-plans-and-design-kept-local.md,
// docs/adr/0073-extractors-read-files-through-rb-model.md (the source-file examples),
// docs/adr/0074-the-cookbook-question-pages-are-fixtures-beside-the-guard-catalogue.md (the
// cookbook's question pages, filled from their fixtures by cookbook.ts).
// Plan: docs/plans/pending/0004-wave-4-reach.md, Steps 3 and 5. Requirements: FR-REACH-01, NFR-DOC-01.

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import {
  QUESTIONS_ROOT,
  fillQuestion,
  questionSlug,
  questionSlugs,
  readQuestion,
  unlisted,
  withPlaceholders,
} from './cookbook.js';
import { checkSite } from './linkcheck.js';
import type { BrokenLink } from './linkcheck.js';
import { trackedFolders } from './links.js';
import type { LinkContext } from './links.js';
import { PLAYGROUND_PAGE, escapeHtml, layout, playgroundComponent } from './layout.js';
import type { EmbeddedExample, SiteInfo } from './layout.js';
import { pagePath, pageSources, relativeUrl } from './pages.js';
import { createRenderer, renderPage } from './render.js';
import { SCHEMA_FILES, readSchemas } from './schemas.js';

/** The repository's GitHub URL. */
export const REPO_URL = 'https://github.com/benbahrenburg/rulebearing';

/** The guard fixture the playground page opens with: its CLI output is committed beside it. */
export const PLAYGROUND_FIXTURE = 'cookbook/guards/no-circular';

/**
 * The small repositories the playground offers as source-file examples: `rb-wasm`'s fixtures,
 * whose command-line outputs are committed beside them (`expected/`), embedded in the page at
 * build time from the tracked files.
 */
export const PLAYGROUND_REPOS: readonly {
  readonly id: string;
  readonly title: string;
  readonly dir: string;
  readonly language: 'typescript' | 'python';
  readonly rule: string;
}[] = [
  {
    id: 'typescript',
    title: 'A TypeScript repository',
    dir: 'crates/rb-wasm/tests/fixtures/typescript/repo',
    language: 'typescript',
    rule: 'no-circular',
  },
  {
    id: 'python',
    title: 'A Python repository',
    dir: 'crates/rb-wasm/tests/fixtures/python/repo',
    language: 'python',
    rule: 'ui-not-to-models',
  },
];

/** The output type the page and its examples open with. */
export const PLAYGROUND_OUTPUT = 'err-long';

/** The files of `wasm-bindgen --target web` output the page loads. */
export const WASM_FILES: readonly string[] = ['rb_wasm.js', 'rb_wasm_bg.wasm'];

/** The compiled page scripts, from `tsc -p site/web`. */
export const WEB_FILES: readonly string[] = [
  'playground.js',
  'playground-core.js',
  'playground-files.js',
];

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

/**
 * The source-file examples: each repository's tracked files, by their path inside it, and its
 * `rulebearing.yaml` as the configuration.
 */
export function playgroundExamples(
  repo: string,
  tracked: readonly string[],
): readonly EmbeddedExample[] {
  return PLAYGROUND_REPOS.map(({ id, title, dir, language, rule }) => {
    const files: Record<string, string> = {};
    for (const path of tracked.filter((p) => p.startsWith(`${dir}/`)).sort()) {
      files[path.slice(dir.length + 1)] = readFileSync(join(repo, path), 'utf8');
    }
    const config = files['rulebearing.yaml'];
    if (config === undefined) {
      throw new Error(
        `${dir}/rulebearing.yaml is not tracked; the playground's ${id} example needs it`,
      );
    }
    return {
      id,
      title,
      data: { id, mode: 'files', config, outputType: PLAYGROUND_OUTPUT, rule, language, files },
    };
  });
}

/** The body of the playground page. */
export function playgroundBody(repo: string, tracked: readonly string[]): string {
  const fixture = join(repo, PLAYGROUND_FIXTURE);
  const config = readFileSync(join(fixture, 'rulebearing.yaml'), 'utf8');
  const graph = readFileSync(join(fixture, 'graph.json'), 'utf8');
  const guide = relativeUrl(PLAYGROUND_PAGE, 'cookbook/index.html');
  const guard: EmbeddedExample = {
    id: 'no-circular',
    title: 'The no-circular guard',
    data: {
      id: 'no-circular',
      mode: 'graph',
      config,
      outputType: PLAYGROUND_OUTPUT,
      rule: 'no-circular',
      graph,
    },
  };
  const examples = [guard, ...playgroundExamples(repo, tracked)];
  return `<h1 id="playground">Playground</h1>
<p>Try a rule set before installing anything. The page opens with the <code>no-circular</code> guard from the <a href="${escapeHtml(guide)}">guard catalogue</a> and the cruise result of its fixture; <strong>Run</strong> evaluates the rules and prints the report the command line prints for <code>rulebearing cruise --graph graph.json -T &lt;output type&gt;</code>, and <strong>Explain</strong> prints <code>rulebearing explain no-circular --graph graph.json</code>. The other two examples are small TypeScript and Python repositories: loading one switches to source files, and <strong>Run</strong> cruises them as the command line does in that folder.</p>
${playgroundComponent(PLAYGROUND_PAGE, { config, graph, outputType: PLAYGROUND_OUTPUT, rule: 'no-circular', examples })}`;
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
  const slugs = questionSlugs(tracked);
  const questions = new Map(slugs.map((slug) => [slug, readQuestion(repo, tracked, slug)]));
  const rendered = sources.map((source) => {
    const question = questions.get(questionSlug(source) ?? '');
    if (question === undefined) {
      return {
        source,
        page: renderPage(md, source, readFileSync(join(repo, source), 'utf8'), context, folders),
        scripts: [],
      };
    }
    const page = renderPage(md, source, withPlaceholders(question), context, folders);
    return {
      source,
      page: { ...page, html: fillQuestion(page.html, question, pagePath(source)) },
      scripts: ['assets/playground.js'],
    };
  });
  const index = `${QUESTIONS_ROOT}README.md`;
  if (slugs.length > 0) {
    const missing = sources.includes(index)
      ? unlisted(readFileSync(join(repo, index), 'utf8'), slugs)
      : slugs;
    if (missing.length > 0) {
      throw new Error(`${index} does not link the question page(s) ${missing.join(', ')}`);
    }
  }

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
  for (const { source, page, scripts } of rendered) {
    brokenSources.push(...page.broken.map((b) => `${source}: ${b}`));
    const path = pagePath(source);
    write(out, path, layout(site, { path, title: page.title, body: page.html, source, scripts }));
    pages.push(path);
  }

  write(
    out,
    PLAYGROUND_PAGE,
    layout(site, {
      path: PLAYGROUND_PAGE,
      title: 'Playground',
      body: playgroundBody(repo, tracked),
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
