// The playground's wasm module, as a browser loads it, run under Node: `evaluate()` over each
// guard-catalogue fixture must find what the command line found (the fixture's `expected.json`,
// which the `guards` job byte-compares against the CLI); `cruise_ts()` and `cruise_py()` over the
// playground's fixtures must print, reporter by reporter, the bytes the command line prints
// there, and `explain()` likewise (the fixtures' `expected/`, which rb-cli's
// `tests/playground_fixtures.rs` holds the CLI to); a JavaScript configuration and what needs
// the disk or a process are refused with the way out; and the module is under 8 MB compressed.
//
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 2 ("comparing `evaluate()` output to the
// committed CLI JSON"; "the `cruise_ts` and `cruise_py` fixtures"; "a size check").
// Requirement: docs/prd.md#fr-reach-01. Decision: docs/adr/0073-extractors-read-files-through-rb-model.md.
//
//   node crates/rb-wasm/tests/wasm.mjs <wasm-bindgen --target nodejs output dir>
//
// The output is CommonJS: inside this repository, whose package.json says "module", the output
// directory needs a package.json of `{ "type": "commonjs" }` (the `wasm` CI job writes one).

import assert from 'node:assert/strict';
import { readFileSync, readdirSync, existsSync, statSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
import { createRequire } from 'node:module';
import { performance } from 'node:perf_hooks';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../../..');
const out = path.resolve(process.argv[2] ?? 'target/rb-wasm-node');

// The command line's fixtures were printed at SOURCE_DATE_EPOCH 2026-10-10T12:00:00Z; the module
// reads the clock through `new Date()`, so the page's clock is set to the same moment.
const NativeDate = Date;
const EPOCH = Date.UTC(2026, 9, 10, 12, 0, 0);
globalThis.Date = class extends NativeDate {
    constructor(...args) {
        if (args.length === 0) {
            super(EPOCH);
        } else {
            super(...args);
        }
    }
};

const require = createRequire(import.meta.url);
const rb = require(path.join(out, 'rb_wasm.js'));

const limit = 8 * 1024 * 1024;
const compressed = gzipSync(readFileSync(path.join(out, 'rb_wasm_bg.wasm')), { level: 9 }).length;
assert.ok(compressed < limit, `the module is ${String(compressed)} bytes compressed, over 8 MB`);

const guards = path.join(root, 'cookbook/guards');
let fixtures = 0;
for (const slug of readdirSync(guards)) {
    const dir = path.join(guards, slug);
    if (!existsSync(path.join(dir, 'expected.json'))) {
        continue;
    }
    const config = readFileSync(path.join(dir, 'rulebearing.yaml'), 'utf8');
    const graph = readFileSync(path.join(dir, 'graph.json'), 'utf8');
    const result = JSON.parse(rb.evaluate(config, graph, 'json'));
    const expected = JSON.parse(readFileSync(path.join(dir, 'expected.json'), 'utf8'));
    assert.deepEqual(result.summary.violations, expected.violations, `${slug}: violations`);
    assert.deepEqual(
        result.summary.vacuousRules ?? [],
        expected.vacuousRules,
        `${slug}: vacuous rules`,
    );
    fixtures += 1;
}
assert.ok(fixtures > 0, 'no guard fixture found');

const graph = readFileSync(path.join(guards, 'no-circular/graph.json'), 'utf8');
assert.throws(
    () => rb.evaluate('module.exports = {};', graph, 'json'),
    /this build has no JavaScript engine/u,
);
assert.throws(() => rb.evaluate('forbidden: []\n', graph, 'nope'), /no output type `nope`/u);
assert.match(rb.version(), /^\d+\.\d+\.\d+/u);

// The reporters rb-cli's tests/playground_fixtures.rs compares (it names the four it leaves out).
const COMPARED = [
    'err',
    'err-long',
    'err-html',
    'json',
    'text',
    'csv',
    'teamcity',
    'azure-devops',
    'agent',
    'null',
    'dot',
    'ddot',
    'archi',
    'cdot',
    'flat',
    'fdot',
    'mermaid',
    'baseline',
    'metrics',
    'junit',
    'trx',
    'html',
    'markdown',
    'plantuml',
];
const EXPLAINED = {
    typescript: ['no-circular', 'no-unresolvable'],
    python: ['no-circular', 'ui-not-to-models'],
};

/** Every file under `dir`, by its path from `dir`: a dropped folder. */
function dropped(dir) {
    const files = new Map();
    const walk = (folder) => {
        for (const name of readdirSync(folder)) {
            const full = path.join(folder, name);
            if (statSync(full).isDirectory()) {
                walk(full);
            } else {
                files.set(
                    path.relative(dir, full).split(path.sep).join('/'),
                    readFileSync(full, 'utf8'),
                );
            }
        }
    };
    walk(dir);
    return files;
}

const playground = path.join(here, 'fixtures');
const timings = [];
let compared = 0;
for (const [name, cruise] of [
    ['typescript', rb.cruise_ts],
    ['python', rb.cruise_py],
]) {
    const files = dropped(path.join(playground, name, 'repo'));
    const config = files.get('rulebearing.yaml') ?? '';
    const expected = (file) => readFileSync(path.join(playground, name, 'expected', file), 'utf8');
    for (const outputType of COMPARED) {
        const started = performance.now();
        const printed = cruise(files, config, outputType);
        timings.push(performance.now() - started);
        assert.equal(printed, expected(`${outputType}.out`), `${name}: ${outputType}`);
        compared += 1;
    }
    // A plain object of paths to text is a dropped folder too.
    assert.equal(cruise(Object.fromEntries(files), config, 'err'), expected('err.out'));
    const cruised = expected('json.out');
    for (const rule of EXPLAINED[name]) {
        assert.equal(
            rb.explain(config, cruised, rule),
            expected(`explain-${rule}.out`),
            `${name}: ${rule}`,
        );
        compared += 1;
    }
}

assert.throws(
    () => rb.cruise_ts('not files', 'forbidden: []\n', 'json'),
    /a Map \(or an object\)/u,
);
assert.throws(
    () => rb.cruise_ts(new Map([['a.ts', 3]]), 'forbidden: []\n', 'json'),
    /both strings/u,
);
assert.throws(
    () => rb.cruise_ts(new Map([['../a.ts', '']]), 'forbidden: []\n', 'json'),
    /not a file inside the dropped folder/u,
);
assert.throws(
    () =>
        rb.cruise_ts(
            new Map([['src/a.ts', '']]),
            'languages:\n  typescript:\n    externalModuleResolutionStrategy: yarn-pnp\n',
            'json',
        ),
    /Yarn Plug'n'Play cannot be used/u,
);
assert.throws(() => rb.explain('forbidden: []\n', graph, 'nope'), /no rule `nope`/u);

timings.sort((a, b) => a - b);
const median = timings[Math.floor(timings.length / 2)] ?? 0;
process.stdout.write(
    `rb-wasm: ${String(fixtures)} guard fixture(s) and ${String(compared)} cruise and explain outputs equal the CLI; ` +
        `cruise median ${median.toFixed(1)} ms; ${String(compressed)} bytes compressed\n`,
);
