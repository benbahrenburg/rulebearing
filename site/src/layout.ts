// The theme: the HTML around every page (header, sidebar, footer and the content security policy)
// and the playground component a page embeds.
//
// Architecture: docs/architecture.md#security-posture (the playground runs entirely client-side;
// plan 0004 § 1.8 and § 1.11: a content security policy that forbids inline script).
// Decisions: docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md,
// docs/adr/0067-the-playground-facade-and-an-optional-javascript-engine.md (YAML and JSON configs
// only), docs/adr/0073-extractors-read-files-through-rb-model.md (source files read in memory).
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 3. Requirements: FR-REACH-01, NFR-DOC-01.

import { navGroups, pagePath, relativeUrl } from './pages.js';

/**
 * The content security policy every page carries. Scripts come only from the site itself, with no
 * inline script and no `eval`; `wasm-unsafe-eval` lets the page compile the playground's module.
 */
export const CSP = [
  "default-src 'self'",
  "script-src 'self' 'wasm-unsafe-eval'",
  "style-src 'self'",
  "img-src 'self' data:",
  "connect-src 'self'",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'none'",
].join('; ');

/** Escapes text for an HTML text node or a double-quoted attribute. */
export function escapeHtml(text: string): string {
  return text
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;');
}

/** What the layout needs to know about the whole site. */
export interface SiteInfo {
  /** Every page source, for the sidebar. */
  readonly sources: readonly string[];
  /** The title of each page, by source. */
  readonly titles: ReadonlyMap<string, string>;
  /** The repository's GitHub URL, without a trailing `/`. */
  readonly repoUrl: string;
  /** The commit the site was built from. */
  readonly commit: string;
  /** The release tag the served schemas come from. */
  readonly schemaRef: string;
}

/** One page to lay out. */
export interface PageSpec {
  /** The page's path in the built site. */
  readonly path: string;
  readonly title: string;
  /** The HTML of the page's content. */
  readonly body: string;
  /** The repository file the page is built from, if any, for the "view source" link. */
  readonly source?: string;
  /** Module scripts the page loads, as site paths. */
  readonly scripts?: readonly string[];
}

/** The site path of the playground page. */
export const PLAYGROUND_PAGE = 'playground.html';

function sidebar(site: SiteInfo, current: string): string {
  const groups = navGroups(site.sources).map((group) => {
    const items = group.sources
      .map((source) => {
        const path = pagePath(source);
        const title = site.titles.get(source) ?? source;
        const here = path === current ? ' aria-current="page"' : '';
        return `<li><a href="${escapeHtml(relativeUrl(current, path))}"${here}>${escapeHtml(title)}</a></li>`;
      })
      .join('');
    const open = group.sources.some((s) => pagePath(s) === current) ? ' open' : '';
    return `<details${open}><summary>${escapeHtml(group.title)}</summary><ul>${items}</ul></details>`;
  });
  return `<nav class="sidebar" aria-label="Documentation">${groups.join('')}</nav>`;
}

/** Lays out one page of the site. */
export function layout(site: SiteInfo, page: PageSpec): string {
  const rel = (to: string): string => escapeHtml(relativeUrl(page.path, to));
  const scripts = (page.scripts ?? [])
    .map((s) => `<script type="module" src="${rel(s)}"></script>`)
    .join('');
  const view =
    page.source === undefined
      ? ''
      : ` <a href="${escapeHtml(`${site.repoUrl}/blob/${site.commit}/${page.source}`)}">View the source</a>.`;
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="${CSP}">
<title>${escapeHtml(page.title)} | Rulebearing</title>
<link rel="stylesheet" href="${rel('assets/site.css')}">
${scripts}
</head>
<body>
<header class="masthead">
<a class="brand" href="${rel('index.html')}">Rulebearing</a>
<a href="${rel(PLAYGROUND_PAGE)}">Playground</a>
<a href="${rel('schema/index.html')}">Schemas</a>
<a href="${escapeHtml(site.repoUrl)}">GitHub</a>
</header>
<div class="frame">
${sidebar(site, page.path)}
<main id="content">
${page.body}
</main>
</div>
<footer class="footer">Built from commit <a href="${escapeHtml(`${site.repoUrl}/tree/${site.commit}`)}"><code>${escapeHtml(site.commit.slice(0, 12))}</code></a>; the schemas are those of release <code>${escapeHtml(site.schemaRef)}</code>.${view}</footer>
</body>
</html>
`;
}

/** An example the component embeds, loaded by its button. */
export interface EmbeddedExample {
  /** The example's id, also its button's `data-example`. */
  readonly id: string;
  /** The button's label. */
  readonly title: string;
  /** The example as the page script reads it (`Example` in site/web/playground-core.ts). */
  readonly data: Readonly<Record<string, unknown>>;
}

/** What a playground component starts with. */
export interface PlaygroundPreset {
  /** The configuration text, YAML or JSON. */
  readonly config: string;
  /** The cruise result, JSON. */
  readonly graph: string;
  /** The output type selected at first. */
  readonly outputType: string;
  /** The rule `explain` is preset to. */
  readonly rule: string;
  /** The examples the component offers, embedded in the page. */
  readonly examples: readonly EmbeddedExample[];
}

/**
 * JSON for a data block (`<script type="application/json">`), which the browser never runs and the
 * content security policy therefore does not govern: every `<` is escaped, so no `</script>` or
 * `<!--` inside the data can end the block early.
 */
export function jsonBlock(data: unknown): string {
  return JSON.stringify(data).replaceAll('<', '\\u003c');
}

/**
 * The output types the playground offers: every one of `rb_report::OUTPUT_TYPES` except
 * `x-dot-webpage`, which draws with GraphViz's `dot` from the `PATH` (ADR-0053), and a browser has
 * no `PATH`. A test compares this list with the crate's.
 */
export const PLAYGROUND_OUTPUT_TYPES: readonly string[] = [
  'err',
  'err-long',
  'err-html',
  'json',
  'text',
  'csv',
  'teamcity',
  'azure-devops',
  'github-annotations',
  'agent',
  'null',
  'dot',
  'ddot',
  'archi',
  'cdot',
  'flat',
  'fdot',
  'mermaid',
  'd2',
  'baseline',
  'metrics',
  'sarif',
  'junit',
  'trx',
  'html',
  'markdown',
  'anon',
  'plantuml',
];

/**
 * The playground component, preloaded with `preset`. `pagePath` is the site path of the page that
 * embeds it, so the module and the ADR links resolve from any page.
 */
export function playgroundComponent(pagePathOfHost: string, preset: PlaygroundPreset): string {
  const rel = (to: string): string => escapeHtml(relativeUrl(pagePathOfHost, to));
  const options = PLAYGROUND_OUTPUT_TYPES.map((type) => {
    const selected = type === preset.outputType ? ' selected' : '';
    return `<option value="${type}"${selected}>${type}</option>`;
  }).join('');
  const buttons = preset.examples
    .map(
      (e) =>
        `<button type="button" data-example="${escapeHtml(e.id)}" disabled>${escapeHtml(e.title)}</button>`,
    )
    .join('\n');
  const data = preset.examples
    .map(
      (e) =>
        `<script type="application/json" data-example-data="${escapeHtml(e.id)}">${jsonBlock(e.data)}</script>`,
    )
    .join('\n');
  return `<section class="playground" data-playground data-module="${rel('playground/rb_wasm.js')}">
<p class="playground-note">Everything runs in this browser, with the same library crates as the <code>rulebearing</code> binary: nothing you paste, choose or drop is uploaded or sent anywhere. The configuration is YAML or JSON. A JavaScript configuration is not accepted: the browser build has no JavaScript engine (<a href="${rel('docs/adr/0067-the-playground-facade-and-an-optional-javascript-engine.html')}">ADR-0067</a>). An <code>extends</code> of a file cannot be followed; the bundled presets resolve as on the command line. Errors are the module's own words, shown as they come.</p>
<ul class="playground-note">
<li><strong>A cruise result:</strong> paste the JSON that <code>rulebearing cruise -T json</code> (or dependency-cruiser) wrote, and <strong>Run</strong> evaluates the configuration's rules over it, as <code>rulebearing cruise --graph graph.json -T &lt;output type&gt;</code> does. This is the way in for .NET: assemblies are not accepted, since reading them needs a build a browser cannot run, so paste the cruise result the command line wrote for the solution.</li>
<li><strong>Source files:</strong> choose a folder or files, or drop them, holding TypeScript and JavaScript or Python, and <strong>Run</strong> extracts and evaluates them, as <code>rulebearing cruise -T &lt;output type&gt;</code> does in that folder. Paths are taken from the chosen folder. The language is the one picked, or, left on <em>Infer from the files</em>, the one with more source files outside <code>node_modules</code> and virtual environments (a tie asks you to pick). Files that are not UTF-8 text, files over 1 MiB, anything past 32 MiB or 20,000 files in all, and version-control folders are skipped, and the note under the pickers names them. A configuration file at the folder's root (<code>rulebearing.yaml</code>, <code>rulebearing.yml</code>, <code>rulebearing.json</code> or <code>.dependency-cruiser.json</code>) is loaded into the configuration box. What needs the disk or a process is refused with the reason: the Node sidecar, Yarn Plug'n'Play, a ratchet's budget file, and a path outside the folder.</li>
<li><strong>Explain</strong> prints what <code>rulebearing explain &lt;rule&gt; --graph graph.json</code> prints for the named rule over the current cruise result: the pasted one, or the <code>json</code> cruise of the chosen files.</li>
</ul>
<p class="playground-examples">Examples:
${buttons}</p>
<div class="playground-controls">
<label>Input <select name="mode"><option value="graph" selected>A cruise result (JSON)</option><option value="files">Source files</option></select></label>
</div>
<div class="playground-inputs">
<label>Configuration (YAML or JSON)<textarea name="config" spellcheck="false" rows="16">${escapeHtml(preset.config)}</textarea></label>
<div data-panel="graph"><label>Cruise result (JSON)<textarea name="graph" spellcheck="false" rows="16">${escapeHtml(preset.graph)}</textarea></label></div>
<div data-panel="files" class="playground-files" hidden>
<label>Language <select name="language"><option value="auto" selected>Infer from the files</option><option value="typescript">TypeScript and JavaScript</option><option value="python">Python</option></select></label>
<label>Choose a folder <input type="file" name="folder" webkitdirectory multiple disabled></label>
<label>Or choose files <input type="file" name="files" multiple disabled></label>
<div class="playground-drop" data-drop>Or drop a folder or files here.</div>
<p data-files-note>No files chosen yet.</p>
</div>
</div>
<div class="playground-controls">
<label>Output type <select name="output-type">${options}</select></label>
<button type="button" data-run disabled>Run</button>
<label>Rule <input type="text" name="rule" spellcheck="false" value="${escapeHtml(preset.rule)}"></label>
<button type="button" data-explain disabled>Explain</button>
<span data-status role="status">Loading the rulebearing module.</span>
</div>
<pre class="playground-output" data-output></pre>
${data}
</section>`;
}
