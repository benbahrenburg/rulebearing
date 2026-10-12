// The live tables: the two coverage tables, rendered from site/data/coverage.json, and the scale
// table, rendered from testbeds/results/scale.json. Both files are written by the nightly
// (testbeds/scripts/coverage_status.py, testbeds/bench/scale_table.py) and reach `main` by pull
// request; the site only renders them. A row is shown with the tab's claim only when the data says
// it is proven; any other row reads "unproven", whatever the tab says.
//
// Decisions: docs/adr/0074-the-coverage-ledger-is-tracked-and-its-status-is-computed.md,
// docs/adr/0009-conformance-suites-as-specification.md, docs/adr/0072-the-docs-site-builds-from-docs-and-serves-the-released-schemas.md.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 4. Requirements: NFR-CONF-03, FR-REACH-01.

import markdownIt from 'markdown-it';
import { escapeHtml } from './layout.js';
import { relativeUrl } from './pages.js';
import { slug } from './slug.js';

/** The coverage data, as the nightly writes it. */
export const COVERAGE_DATA = 'site/data/coverage.json';

/** The scale data, as the nightly writes it. */
export const SCALE_DATA = 'testbeds/results/scale.json';

/** The overview page of the live tables. */
export const COVERAGE_INDEX = 'coverage/index.html';

/** The scale table's page. */
export const SCALE_PAGE = 'coverage/scale.html';

/** The repository's Actions runs. */
const RUNS = 'https://github.com/benbahrenburg/rulebearing/actions/runs/';

/** One piece of evidence, as coverage_status.py writes it. */
export interface Evidence {
  readonly label: string;
  readonly proven: boolean;
  readonly source: string;
  readonly upstream: boolean;
  readonly run?: string;
  readonly runUrl?: string;
}

/** One row of a coverage tab. */
export interface CoverageRow {
  readonly cells: readonly string[];
  readonly claim: string;
  readonly status: 'proven' | 'unproven';
  readonly evidence: readonly Evidence[];
}

/** One `##` section of a coverage tab. */
export interface CoverageSection {
  readonly title: string;
  readonly headers: readonly string[];
  readonly rows: readonly CoverageRow[];
}

/** One coverage tab. */
export interface CoverageTab {
  readonly id: string;
  readonly title: string;
  readonly counts: { readonly proven: number; readonly unproven: number };
  readonly sections: readonly CoverageSection[];
}

/** The whole coverage document. */
export interface Coverage {
  readonly run: string;
  readonly tabs: readonly CoverageTab[];
}

/** One row of the scale table. */
export interface ScaleRow {
  readonly repo: string;
  readonly mode: string;
  readonly sha?: string;
  readonly detail?: string;
  readonly seconds?: number;
  readonly peakMb?: number | null;
  readonly run?: string;
  readonly stale?: boolean;
}

/** The scale document. */
export interface Scale {
  readonly run?: string;
  readonly rows: readonly ScaleRow[];
  readonly stopHook?: {
    readonly repo: string;
    readonly p95: number;
    readonly edits: number;
    readonly cpus: number;
    readonly run: string;
    readonly stale?: boolean;
  };
}

/** A page the tables module produces. */
export interface TablePage {
  readonly path: string;
  readonly title: string;
  readonly body: string;
}

/** The columns that hold a tab's claim, by tab: the cell replaced by the computed status. */
export const CLAIM_COLUMNS: readonly string[] = ['Rulebearing', 'Status'];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function strings(value: unknown): value is readonly string[] {
  return Array.isArray(value) && value.every((v) => typeof v === 'string');
}

function isEvidence(value: unknown): value is Evidence {
  return (
    isRecord(value) &&
    typeof value.label === 'string' &&
    typeof value.proven === 'boolean' &&
    typeof value.source === 'string' &&
    typeof value.upstream === 'boolean'
  );
}

function isRow(value: unknown): value is CoverageRow {
  return (
    isRecord(value) &&
    strings(value.cells) &&
    typeof value.claim === 'string' &&
    (value.status === 'proven' || value.status === 'unproven') &&
    Array.isArray(value.evidence) &&
    value.evidence.every(isEvidence)
  );
}

function isSection(value: unknown): value is CoverageSection {
  return (
    isRecord(value) &&
    typeof value.title === 'string' &&
    strings(value.headers) &&
    Array.isArray(value.rows) &&
    value.rows.every(isRow)
  );
}

function isTab(value: unknown): value is CoverageTab {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.title === 'string' &&
    isRecord(value.counts) &&
    typeof value.counts.proven === 'number' &&
    typeof value.counts.unproven === 'number' &&
    Array.isArray(value.sections) &&
    value.sections.every(isSection)
  );
}

/** The coverage document in `text`, or an error naming what is wrong with it. */
export function parseCoverage(text: string): Coverage {
  const data: unknown = JSON.parse(text);
  if (
    !isRecord(data) ||
    typeof data.run !== 'string' ||
    !Array.isArray(data.tabs) ||
    !data.tabs.every(isTab)
  ) {
    throw new Error(
      `${COVERAGE_DATA} is not a coverage document; it is written by testbeds/scripts/coverage_status.py write`,
    );
  }
  return { run: data.run, tabs: data.tabs };
}

function isScaleRow(value: unknown): value is ScaleRow {
  return isRecord(value) && typeof value.repo === 'string' && typeof value.mode === 'string';
}

/** The scale document in `text`, or an error naming what is wrong with it. */
export function parseScale(text: string): Scale {
  const data: unknown = JSON.parse(text);
  if (!isRecord(data) || !Array.isArray(data.rows) || !data.rows.every(isScaleRow)) {
    throw new Error(
      `${SCALE_DATA} is not a scale document; it is written by testbeds/bench/scale_table.py measure`,
    );
  }
  return data as unknown as Scale;
}

const inline = markdownIt({ html: false, linkify: false, typographer: false });

/** A tab cell's inline Markdown (code spans, emphasis) as HTML; raw HTML is escaped. */
export function cellHtml(text: string): string {
  return inline.renderInline(text);
}

function runLink(run: string): string {
  return /^\d+$/u.test(run)
    ? `<a href="${RUNS}${run}">run ${escapeHtml(run)}</a>`
    : escapeHtml(run);
}

function evidenceHtml(evidence: readonly Evidence[]): string {
  if (evidence.length === 0) {
    return '<p class="evidence-none">No evidence is named for this row.</p>';
  }
  const items = evidence.map((e) => {
    const whose = e.upstream ? 'upstream' : 'rulebearing';
    const label = `<a href="${escapeHtml(e.source)}">${escapeHtml(e.label)}</a>`;
    let verdict: string;
    if (!e.proven) {
      verdict = '<span class="unproven">no passing result</span>';
    } else if (e.runUrl !== undefined) {
      verdict = `<a class="proven" href="${escapeHtml(e.runUrl)}">passed in run ${escapeHtml(e.run ?? '')}</a>`;
    } else {
      verdict = `<span class="proven">passed${e.run === undefined ? '' : ` (${escapeHtml(e.run)})`}</span>`;
    }
    return `<li class="evidence-${whose}">${label}: ${verdict}</li>`;
  });
  return `<ul class="evidence">${items.join('')}</ul>`;
}

function statusHtml(row: CoverageRow): string {
  if (row.status === 'proven') {
    return row.claim === '' ? '<span class="proven">proven</span>' : cellHtml(row.claim);
  }
  const tab =
    row.claim === '' ? '' : ` <span class="tab-claim">(the tab: ${cellHtml(row.claim)})</span>`;
  return `<strong class="unproven">unproven</strong>${tab}`;
}

/** One section's table: the tab's columns, the claim column computed, and the evidence. */
export function sectionHtml(section: CoverageSection): string {
  const claimAt = section.headers.findIndex((h) => CLAIM_COLUMNS.includes(h));
  const headers = [...section.headers, ...(claimAt < 0 ? ['Status'] : []), 'Evidence'];
  const head = headers.map((h) => `<th>${cellHtml(h)}</th>`).join('');
  const rows = section.rows.map((row) => {
    const cells = section.headers.map((_, i) =>
      i === claimAt ? statusHtml(row) : cellHtml(row.cells[i] ?? ''),
    );
    if (claimAt < 0) {
      cells.push(statusHtml(row));
    }
    cells.push(evidenceHtml(row.evidence));
    return `<tr class="row-${row.status}">${cells.map((c) => `<td>${c}</td>`).join('')}</tr>`;
  });
  const id = slug(section.title);
  return `<h2 id="${escapeHtml(id)}">${cellHtml(section.title)}</h2>
<div class="table-scroll"><table class="coverage">
<thead><tr>${head}</tr></thead>
<tbody>
${rows.join('\n')}
</tbody>
</table></div>`;
}

function provenance(coverage: Coverage, from: string): string {
  const adr = (file: string, name: string): string =>
    `<a href="${escapeHtml(relativeUrl(from, `docs/adr/${file}.html`))}">${name}</a>`;
  let run: string;
  if (coverage.run === '') {
    run = 'No run has written this file yet.';
  } else if (/^\d+$/u.test(coverage.run)) {
    run = `Written from nightly ${runLink(coverage.run)}.`;
  } else {
    run = `Written from ${escapeHtml(coverage.run)}, not a nightly run; the next nightly run replaces it.`;
  }
  return `<p>A row reads as the tab claims only when every piece of its evidence passed: an upstream spec or ported case in conformance gate 1 or gate 2, or a named Rulebearing test. Any other row reads <strong class="unproven">unproven</strong>, whatever the tab says (${adr('0009-conformance-suites-as-specification', 'ADR-0009')}, ${adr('0074-the-coverage-ledger-is-tracked-and-its-status-is-computed', 'ADR-0074')}). ${run} Evidence marked upstream is the pinned suite's own; the rest is Rulebearing's tests.</p>`;
}

/** The page of one coverage tab. */
export function tabPage(coverage: Coverage, tab: CoverageTab, path: string): TablePage {
  const { proven, unproven } = tab.counts;
  const body = `<h1 id="${escapeHtml(slug(tab.title))}">${escapeHtml(tab.title)}</h1>
${provenance(coverage, path)}
<p class="coverage-counts">${String(proven)} of ${String(proven + unproven)} rows proven; ${String(unproven)} unproven.</p>
${tab.sections.map(sectionHtml).join('\n')}`;
  return { path, title: tab.title, body };
}

/** The site path of a tab's page. */
export function tabPath(tab: CoverageTab): string {
  return `coverage/${tab.id.replace(/-\d+(?:\.\d+)*$/u, '')}.html`;
}

function number(value: number | null | undefined, unit: string): string {
  return typeof value === 'number' ? `${String(value)} ${unit}` : '-';
}

/** The scale table's page, as scale_table.py renders it into the README. */
export function scalePage(scale: Scale): TablePage {
  const rows = scale.rows.map((row) => {
    const repo = escapeHtml(row.repo);
    const name =
      row.sha === undefined || row.sha === ''
        ? repo
        : `<a href="https://github.com/${repo}/tree/${escapeHtml(row.sha)}">${repo}</a>`;
    const when =
      row.run === undefined || row.run === ''
        ? 'not measured yet'
        : `${runLink(row.run)}${row.stale === true ? ' (last completed)' : ''}`;
    const modules = (row.detail ?? '').replace(/^modules: /u, '');
    return `<tr><td>${name}</td><td>${escapeHtml(row.mode === '' ? '-' : row.mode)}</td><td>${number(row.seconds, 's')}</td><td>${number(row.peakMb, 'MB')}</td><td>${escapeHtml(modules === '' ? '-' : modules)}</td><td>${when}</td></tr>`;
  });
  const hook = scale.stopHook;
  const hookLine =
    hook === undefined
      ? ''
      : `<p>Stop hook on ${escapeHtml(hook.repo)} in source mode: p95 ${String(hook.p95)} s over ${String(hook.edits)} edits on ${String(hook.cpus)} CPUs, ${runLink(hook.run)}${hook.stale === true ? ' (last completed)' : ''}.</p>`;
  const body = `<h1 id="scale-table">Scale table</h1>
<p>Rulebearing's wall-clock time and peak memory on the scale repositories, the median of three cruises, measured by the nightly and written to <code>testbeds/results/scale.json</code> by <code>testbeds/bench/scale_table.py</code>. A row that did not complete in the latest night keeps its last measurement, marked as such.</p>
<div class="table-scroll"><table class="scale">
<thead><tr><th>Repository</th><th>.NET read as</th><th>Wall-clock</th><th>Peak memory</th><th>Modules</th><th>Measured in run</th></tr></thead>
<tbody>
${rows.join('\n')}
</tbody>
</table></div>
${hookLine}`;
  return { path: SCALE_PAGE, title: 'Scale table', body };
}

/** The overview: each tab's counts and the scale table, linked. */
export function indexPage(coverage: Coverage): TablePage {
  const link = (to: string, text: string): string =>
    `<a href="${escapeHtml(relativeUrl(COVERAGE_INDEX, to))}">${escapeHtml(text)}</a>`;
  const items = coverage.tabs.map(
    (tab) =>
      `<li>${link(tabPath(tab), tab.title)}: ${String(tab.counts.proven)} of ${String(tab.counts.proven + tab.counts.unproven)} rows proven.</li>`,
  );
  const body = `<h1 id="coverage">Coverage</h1>
${provenance(coverage, COVERAGE_INDEX)}
<ul>
${items.join('\n')}
<li>${link(SCALE_PAGE, 'The scale table')}: wall-clock time and peak memory on the scale repositories.</li>
</ul>`;
  return { path: COVERAGE_INDEX, title: 'Coverage', body };
}

/** Every page of the live tables, from the two data files' text. */
export function tablePages(coverageText: string, scaleText: string): TablePage[] {
  const coverage = parseCoverage(coverageText);
  return [
    indexPage(coverage),
    ...coverage.tabs.map((tab) => tabPage(coverage, tab, tabPath(tab))),
    scalePage(parseScale(scaleText)),
  ];
}
