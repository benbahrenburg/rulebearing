// Assembles the nightly test-bed table from every row's result.json.
//
// Plan: docs/plans/pending/0000-wave-0-spike.md, Step 7 item 3. Requirement: docs/prd.md#nfr-conf-03.
// Usage: node testbeds/summarise.mjs <out-dir> [--readme README.md] [--layer5 <dir>]
//
// With --layer5, each <dir>/<owner>__<repo>/row.json that gate 1's layer 5 wrote gives its row
// the zero diff and Rulebearing's time, taken where both tools ran with the repository's
// dependencies installed (docs/plans/pending/0001-wave-1-typescript-parity.md, Step 18).
//
// A row's `regression` pair (this build and the baseline, timed on one runner by run.sh) passes
// through unchanged; the layer 5 time replaces only `rulebearing`.
//
// Writes <out-dir>/summary.json (machine-readable, what check-regression.sh compares) and
// <out-dir>/summary.md (the table). With --readme it also replaces the text between the
// `<!-- testbeds:start -->` and `<!-- testbeds:end -->` markers in that file.
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const args = process.argv.slice(2);
const readmeIndex = args.indexOf('--readme');
const readme = readmeIndex >= 0 ? args[readmeIndex + 1] : null;
const layer5Index = args.indexOf('--layer5');
const layer5 = layer5Index >= 0 ? args[layer5Index + 1] : null;
const flagValues = new Set([readme, layer5]);
const out = args.find((a) => !a.startsWith('--') && !flagValues.has(a));
if (!out) {
    console.error(
        'usage: node testbeds/summarise.mjs <out-dir> [--readme README.md] [--layer5 <dir>]',
    );
    process.exit(2);
}

const oracleRows = new Map(
    layer5 && existsSync(layer5)
        ? readdirSync(layer5)
              .filter((entry) => existsSync(join(layer5, entry, 'row.json')))
              .map((entry) => JSON.parse(readFileSync(join(layer5, entry, 'row.json'), 'utf8')))
              .map((row) => [row.repo, row])
        : [],
);

const rows = readdirSync(out)
    .filter((entry) => existsSync(join(out, entry, 'result.json')))
    .map((entry) => JSON.parse(readFileSync(join(out, entry, 'result.json'), 'utf8')))
    .map((row) => {
        const oracle = oracleRows.get(row.repo);
        return oracle
            ? { ...row, rulebearing: oracle.rulebearing, zeroDiff: oracle.zeroDiff }
            : row;
    })
    .sort((a, b) => a.role.localeCompare(b.role) || a.repo.localeCompare(b.repo));

const seconds = (timing) =>
    typeof timing?.wall_seconds === 'number' ? `${timing.wall_seconds} s` : '';
const memory = (timing) =>
    typeof timing?.max_rss_kb === 'number' ? `${Math.round(timing.max_rss_kb / 1024)} MB` : '';
const counts = rows.reduce(
    (acc, row) => ({ ...acc, [row.status]: (acc[row.status] ?? 0) + 1 }),
    {},
);

const lines = [
    `Rows: ${rows.length}; ${Object.entries(counts)
        .sort()
        .map(([status, n]) => `${status} ${n}`)
        .join(
            ', ',
        )}. Rulebearing runs on the dependency-cruiser rows (wave 1); its time is the median of three runs, and zero diff compares its result with the incumbent's.`,
    '',
    '| Repository | Role | Incumbent | Status | Incumbent time | Incumbent peak memory | Rulebearing time | Zero diff |',
    '| --- | --- | --- | --- | --- | --- | --- | --- |',
    ...rows.map(
        (row) =>
            `| [${row.repo}](https://github.com/${row.repo}/tree/${row.sha}) | ${row.role} | ${row.tool} | ${row.status} | ${seconds(row.incumbent)} | ${memory(row.incumbent)} | ${seconds(row.rulebearing)} | ${row.zeroDiff ?? ''} |`,
    ),
];
const table = `${lines.join('\n')}\n`;

writeFileSync(join(out, 'summary.md'), table);
writeFileSync(join(out, 'summary.json'), `${JSON.stringify(rows, null, 2)}\n`);
console.log(`summarise: ${rows.length} rows written to ${join(out, 'summary.md')}`);

if (readme) {
    const start = '<!-- testbeds:start -->';
    const end = '<!-- testbeds:end -->';
    const text = readFileSync(readme, 'utf8');
    const from = text.indexOf(start);
    const to = text.indexOf(end);
    if (from < 0 || to < from) {
        console.error(`summarise: ${readme} has no ${start} ... ${end} markers`);
        process.exit(1);
    }
    writeFileSync(readme, `${text.slice(0, from + start.length)}\n${table}${text.slice(to)}`);
    console.log(`summarise: table written between the markers in ${readme}`);
}
