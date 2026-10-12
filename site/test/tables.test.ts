// The live coverage and scale tables: a row shows the tab's claim only when the data says it is
// proven, any other row reads "unproven", every cell's Markdown is rendered and escaped, and the
// committed data files render.
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 4; docs/adr/0074-the-coverage-ledger-is-tracked-and-its-status-is-computed.md.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import {
  COVERAGE_DATA,
  COVERAGE_INDEX,
  SCALE_DATA,
  SCALE_PAGE,
  cellHtml,
  indexPage,
  parseCoverage,
  parseScale,
  scalePage,
  sectionHtml,
  tabPage,
  tabPath,
  tablePages,
} from '../src/tables.js';
import type { Coverage, CoverageSection, CoverageTab } from '../src/tables.js';

const realRepo = resolve(import.meta.dirname, '../..');

const proven = {
  label: 'gate 1 layer 2: test/validate/index.spec.mjs',
  proven: true,
  source: 'https://example.com/spec',
  upstream: true,
  run: '42',
  runUrl: 'https://github.com/benbahrenburg/rulebearing/actions/runs/42',
};
const localRun = {
  label: 'gate 2: every ported ArchUnitNET case',
  proven: true,
  source: 'https://example.com/ported',
  upstream: true,
  run: '42',
};
const missing = {
  label: 'rb-cli/cache: every test',
  proven: false,
  source: 'https://example.com/cache.rs',
  upstream: false,
};

const section: CoverageSection = {
  title: 'Result document (`cruise-result` schema)',
  headers: ['Attribute', 'Rulebearing', 'Wave'],
  rows: [
    { cells: ['`a`', 'Parity+', '1'], claim: 'Parity+', status: 'proven', evidence: [proven] },
    {
      cells: ['`b <script>`', 'Parity', '2'],
      claim: 'Parity',
      status: 'unproven',
      evidence: [proven, missing],
    },
    { cells: ['`c`', '', '3'], claim: '', status: 'proven', evidence: [localRun] },
    { cells: ['`d`', 'Kept', '1'], claim: 'Kept', status: 'unproven', evidence: [] },
  ],
};

const tab: CoverageTab = {
  id: 'dependency-cruiser-18.2.0',
  title: 'dependency-cruiser 18.2.0 coverage',
  counts: { proven: 2, unproven: 2 },
  sections: [section],
};

const coverage: Coverage = { run: '42', tabs: [tab] };

describe('a coverage section', () => {
  const html = sectionHtml(section);
  const rows = html.split('<tr class=').slice(1);

  it('shows the claim only for a proven row', () => {
    expect(rows[0]).toContain('<td>Parity+</td>');
    expect(rows[1]).toContain('<strong class="unproven">unproven</strong>');
    expect(rows[1]).toContain('<span class="tab-claim">(the tab: Parity)</span>');
    expect(rows[1]).not.toContain('<td>Parity</td>');
    expect(rows[2]).toContain('<span class="proven">proven</span>');
    expect(rows[3]).toContain(
      '<strong class="unproven">unproven</strong> <span class="tab-claim">(the tab: Kept)</span>',
    );
    expect(rows[3]).toContain('No evidence is named for this row.');
  });

  it('links each piece of evidence and says whether it passed', () => {
    expect(rows[0]).toContain(
      '<li class="evidence-upstream"><a href="https://example.com/spec">gate 1 layer 2: test/validate/index.spec.mjs</a>: <a class="proven" href="https://github.com/benbahrenburg/rulebearing/actions/runs/42">passed in run 42</a></li>',
    );
    expect(rows[1]).toContain(
      '<li class="evidence-rulebearing"><a href="https://example.com/cache.rs">rb-cli/cache: every test</a>: <span class="unproven">no passing result</span></li>',
    );
    expect(rows[2]).toContain('<span class="proven">passed (42)</span>');
  });

  it("renders the tab's Markdown and escapes its HTML", () => {
    expect(html).toContain(
      '<h2 id="result-document-cruise-result-schema">Result document (<code>cruise-result</code> schema)</h2>',
    );
    expect(rows[1]).toContain('<code>b &lt;script&gt;</code>');
    expect(cellHtml('a <b>bold</b>')).toBe('a &lt;b&gt;bold&lt;/b&gt;');
  });

  it('adds a status column where the tab has none', () => {
    const selectors = sectionHtml({
      title: 'Selectors',
      headers: ['ArchUnitNET', 'Rulebearing key', 'Languages'],
      rows: [
        {
          cells: ['`Types()`', '`kind: type`', 'all'],
          claim: 'Parity',
          status: 'unproven',
          evidence: [],
        },
      ],
    });
    expect(selectors).toContain('<th>Languages</th><th>Status</th><th>Evidence</th>');
    expect(selectors).toContain('<td>all</td><td><strong class="unproven">unproven</strong>');
  });
});

describe('the pages', () => {
  it('names the run and counts the rows', () => {
    const page = tabPage(coverage, tab, tabPath(tab));
    expect(page.path).toBe('coverage/dependency-cruiser.html');
    expect(page.body).toContain('2 of 4 rows proven; 2 unproven.');
    expect(page.body).toContain(
      'Written from nightly <a href="https://github.com/benbahrenburg/rulebearing/actions/runs/42">run 42</a>.',
    );
    expect(page.body).toContain(
      'href="../docs/adr/0074-the-coverage-ledger-is-tracked-and-its-status-is-computed.html"',
    );
    const unrun = tabPage({ ...coverage, run: '' }, tab, 'coverage/x.html');
    expect(unrun.body).toContain('No run has written this file yet.');
    const local = indexPage({ ...coverage, run: 'local' });
    expect(local.body).toContain(
      'Written from local, not a nightly run; the next nightly run replaces it.',
    );
    expect(local.body).toContain(
      '<a href="dependency-cruiser.html">dependency-cruiser 18.2.0 coverage</a>: 2 of 4 rows proven.',
    );
    expect(local.body).toContain('<a href="scale.html">The scale table</a>');
  });

  it('renders the scale table as the README does', () => {
    const page = scalePage({
      rows: [
        {
          repo: 'dotnet/aspnetcore',
          mode: 'compiled',
          sha: 'abc',
          detail: 'modules: dotnet 1',
          seconds: 33.65,
          peakMb: 5613,
          run: '7',
          stale: true,
        },
        { repo: 'n8n-io/n8n', mode: '', stale: true },
      ],
      stopHook: { repo: 'dotnet/aspnetcore', p95: 1.6, edits: 200, cpus: 4, run: '7' },
    });
    expect(page.path).toBe(SCALE_PAGE);
    expect(page.body).toContain(
      '<tr><td><a href="https://github.com/dotnet/aspnetcore/tree/abc">dotnet/aspnetcore</a></td><td>compiled</td><td>33.65 s</td><td>5613 MB</td><td>dotnet 1</td><td><a href="https://github.com/benbahrenburg/rulebearing/actions/runs/7">run 7</a> (last completed)</td></tr>',
    );
    expect(page.body).toContain(
      '<tr><td>n8n-io/n8n</td><td>-</td><td>-</td><td>-</td><td>-</td><td>not measured yet</td></tr>',
    );
    expect(page.body).toContain(
      'p95 1.6 s over 200 edits on 4 CPUs, <a href="https://github.com/benbahrenburg/rulebearing/actions/runs/7">run 7</a>.',
    );
    const stale = scalePage({
      rows: [],
      stopHook: { repo: 'r', p95: 1, edits: 1, cpus: 1, run: 'x', stale: true },
    });
    expect(stale.body).toContain(', x (last completed).');
  });

  it('refuses data that is not what the scripts write', () => {
    expect(() => parseCoverage('{"tabs": []}')).toThrow(/coverage_status.py write/u);
    expect(() =>
      parseCoverage(JSON.stringify({ run: '', tabs: [{ ...tab, sections: [{ title: 1 }] }] })),
    ).toThrow(/not a coverage document/u);
    expect(() => parseScale('{"rows": [{"repo": 1}]}')).toThrow(/scale_table.py measure/u);
    expect(() => parseScale('[]')).toThrow(/not a scale document/u);
  });

  it('renders the committed data files', () => {
    const pages = tablePages(
      readFileSync(resolve(realRepo, COVERAGE_DATA), 'utf8'),
      readFileSync(resolve(realRepo, SCALE_DATA), 'utf8'),
    );
    expect(pages.map((p) => p.path)).toEqual([
      COVERAGE_INDEX,
      'coverage/dependency-cruiser.html',
      'coverage/archunitnet.html',
      SCALE_PAGE,
    ]);
    const data = parseCoverage(readFileSync(resolve(realRepo, COVERAGE_DATA), 'utf8'));
    const rows = data.tabs.flatMap((t) => t.sections.flatMap((s) => s.rows));
    expect(rows).toHaveLength(190);
    for (const row of rows) {
      if (row.status === 'proven') {
        expect(row.evidence.length).toBeGreaterThan(0);
        expect(row.evidence.every((e) => e.proven)).toBe(true);
      }
    }
  });
});
