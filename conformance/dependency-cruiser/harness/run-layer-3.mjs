// Conformance gate 1, layer 3: dependency-cruiser 18.2.0's report specs for the wave 1 and wave 2
// reporters, run unmodified, with each reporter import forwarded to `rulebearing report`
// (layer3-hooks.mjs) and each reporter-internal import to `rulebearing validate`. The specs compare
// output byte for byte against upstream's fixtures (teamcity's per-session flowId and timestamp
// aside, which the spec itself removes), so a pass is a byte-compare pass.
//
// Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 12;
// docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 10. Decision:
// docs/adr/0009-conformance-suites-as-specification.md.
// Usage: node run-layer-3.mjs <dependency-cruiser checkout> [spec ...]
import { readFileSync, readdirSync, realpathSync, statSync } from 'node:fs';
import { register, createRequire } from 'node:module';
import { join, relative, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { report } from './shim.mjs';

const args = process.argv.slice(2);
const upstreamArgument = args.find((arg) => !arg.endsWith('.spec.mjs'));
if (!upstreamArgument) {
    console.error('usage: node run-layer-3.mjs <dependency-cruiser checkout> [spec ...]');
    process.exit(2);
}
const upstream = realpathSync(resolve(upstreamArgument));
process.chdir(upstream);
process.env.NO_COLOR = '1';

register('./layer3-hooks.mjs', import.meta.url);

// The wave 1 reporter set (plan 0001 § 1.3, § Output types).
const WAVE_1 = [
    'test/report/error/error.spec.mjs',
    'test/report/error/error-long.spec.mjs',
    'test/report/text/text.spec.mjs',
    'test/report/csv/csv.spec.mjs',
    'test/report/teamcity/teamcity.spec.mjs',
    'test/report/azure-devops/azure-devops.spec.mjs',
    'test/report/null/null.spec.mjs',
];
// The wave 2 reporters (plan 0002 § 2.10): baseline, the graph reporters, metrics and err-html,
// with the unit specs of their internals.
const WAVE_2 = [
    'test/report/baseline/baseline.spec.mjs',
    'test/report/dot/module-level/index.spec.mjs',
    'test/report/dot/folder-level/folder-level.spec.mjs',
    'test/report/dot/custom-level/index.spec.mjs',
    'test/report/dot/flat-level/index.spec.mjs',
    'test/report/dot/theming.spec.mjs',
    'test/report/dot/module-utl.spec.mjs',
    'test/report/mermaid/mermaid.spec.mjs',
    'test/report/d2/d2.spec.mjs',
    'test/report/metrics/metrics.spec.mjs',
    'test/report/error-html/error-html.spec.mjs',
    'test/report/error-html/utl.spec.mjs',
];
const only = args.filter((arg) => arg.endsWith('.spec.mjs'));
const specs = only.length > 0 ? only : [...WAVE_1, ...WAVE_2];

const Mocha = createRequire(join(upstream, 'package.json'))('mocha');
const mocha = new Mocha({ timeout: 20_000, reporter: 'base' });
for (const spec of specs) {
    mocha.addFile(join(upstream, spec));
}
await mocha.loadFilesAsync();
const failedSpecs = new Set();
let tests = 0;
const runner = mocha.run();
runner.on('test end', () => {
    tests += 1;
});
runner.on('fail', (test, error) => {
    const file = test.file ?? test.parent?.file;
    if (file) {
        failedSpecs.add(relative(upstream, file).split('\\').join('/'));
    }
    const message = String(error?.message ?? error).slice(0, 800);
    console.log(`layer3: FAIL ${test.fullTitle()}\n    ${message.split('\n').join('\n    ')}`);
});
await new Promise((done) => {
    runner.on('end', done);
});
console.log(`layer3: specs=${specs.length} tests=${tests} failing-specs=${failedSpecs.size}`);

// The oracle comparison. The err-html and metrics specs assert with regular expressions, not a
// byte comparison, so every wave 2 reporter is also run against upstream's own implementation
// over every cruise result mock under test/report, and the two outputs must be identical (the
// err-html footer's run date aside, which is the clock's).
const THEME = {
    graph: { splines: 'ortho', ranksep: 1 },
    node: { fontsize: 11 },
    modules: [
        {
            criteria: { source: ['\\.json$', 'index'] },
            attributes: { fillcolor: 'red', shape: 'note' },
        },
        {
            criteria: { 'rules[0].severity': 'warn', orphan: true },
            attributes: { color: 'purple' },
        },
    ],
    dependencies: [
        { criteria: { resolved: 'node_modules' }, attributes: { color: 'blue', penwidth: 3 } },
    ],
};
const ORACLE = [
    [
        'dot',
        'src/report/dot/dot-module.mjs',
        [
            undefined,
            { showMetrics: true },
            { theme: THEME },
            { theme: { ...THEME, replace: true } },
            { filters: { includeOnly: { path: '^src' }, focus: { path: 'index', depth: 2 } } },
            { filters: { reaches: { path: 'utl' }, highlight: { path: 'main' } } },
            { collapsePattern: '^src/[^/]+' },
        ],
    ],
    [
        'ddot',
        'src/report/dot/dot-folder.mjs',
        [
            undefined,
            { showMetrics: true },
            { filters: { exclude: { path: 'test' } }, theme: THEME },
        ],
    ],
    [
        'archi',
        'src/report/dot/dot-custom.mjs',
        [undefined, { collapsePattern: '^[^/]+' }, { theme: THEME }],
    ],
    [
        'flat',
        'src/report/dot/dot-flat.mjs',
        [undefined, { showMetrics: true }, { collapsePattern: '^(src|test)/[^/]+', theme: THEME }],
    ],
    ['mermaid', 'src/report/mermaid.mjs', [undefined, { minify: false }]],
    ['d2', 'src/report/d2.mjs', [undefined]],
    [
        'metrics',
        'src/report/metrics.mjs',
        [
            undefined,
            { hideFolders: true },
            { hideModules: true },
            { orderBy: 'name' },
            { orderBy: 'moduleCount' },
        ],
    ],
    [
        'err-html',
        'src/report/error-html/index.mjs',
        [undefined, { showExternalModulesUnresolved: true, showAliasedModulesUnresolved: true }],
    ],
];
const RUN_DATE = /(<\/a> \/\n {6})[^<]*(<\/p>)/u;

function mocks(folder) {
    return readdirSync(folder).flatMap((name) => {
        const path = join(folder, name);
        if (statSync(path).isDirectory()) {
            return mocks(path);
        }
        return /__mocks?__?\//u.test(path) && /\.(json|mjs)$/u.test(name) ? [path] : [];
    });
}

let compared = 0;
let differing = 0;
let skipped = 0;
const onlySpecs = only.length > 0;
if (!onlySpecs) {
    const results = [];
    for (const path of mocks(join(upstream, 'test/report'))) {
        const loaded = path.endsWith('.json')
            ? JSON.parse(readFileSync(path, 'utf8'))
            : (await import(pathToFileURL(path).href)).default;
        if (loaded && Array.isArray(loaded.modules) && loaded.summary) {
            results.push([relative(upstream, path), loaded]);
        }
    }
    for (const [outputType, module, optionSets] of ORACLE) {
        const upstreamReporter = (await import(pathToFileURL(join(upstream, module)).href)).default;
        for (const [name, result] of results) {
            for (const options of optionSets) {
                let expected;
                try {
                    expected = upstreamReporter(structuredClone(result), options);
                } catch {
                    // A mock upstream's reporter cannot render is not a comparison.
                    skipped += 1;
                    continue;
                }
                const actual = report(outputType, result, options);
                compared += 1;
                const normalise = (text) => text.replace(RUN_DATE, '$1$2');
                if (
                    normalise(actual.output) !== normalise(expected.output) ||
                    actual.exitCode !== expected.exitCode
                ) {
                    differing += 1;
                    console.log(
                        `layer3: ORACLE DIFF ${outputType} ${name} ${JSON.stringify(options ?? null)}`,
                    );
                }
            }
        }
    }
    console.log(
        `layer3: oracle comparisons=${compared} differing=${differing} not-renderable-upstream=${skipped}`,
    );
}
process.exit(failedSpecs.size > 0 || differing > 0 ? 1 : 0);
