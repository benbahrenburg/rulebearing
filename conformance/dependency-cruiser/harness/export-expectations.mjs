// Records conformance gate 1 layer 1: every test in dependency-cruiser's test/extract, run under
// the pinned version, with the input and output of the extraction function it exercises.
//
// Plan: docs/plans/pending/0000-wave-0-spike.md, Step 5 item 1 and § 1.6 ("Spike A fixture
// expectations without Node at test time"). Decision: docs/adr/0009-conformance-suites-as-specification.md.
// Called by scripts/vendor.sh. Usage: node export-expectations.mjs <dependency-cruiser checkout> <output dir>
//
// How it works. The upstream specs are run unmodified with mocha. A module loader hook (hooks.mjs)
// wraps the default export of each extraction surface listed in SURFACES below, and recorder.mjs
// notes every call a test makes to one of them, outermost call only, with its arguments and its
// return value. A passing test's return value is by definition what dependency-cruiser 18.2.0
// expects, so the recording is the specification. The Rust test
// crates/rb-extract-ts/tests/extract_fixtures.rs replays each recorded call against the Rust
// extractor and compares.
//
// Which tests are layer 1 is decided mechanically: a test is a layer 1 case when it calls at least
// one surface. The surfaces are the functions whose output is part of a cruise result (a
// dependency list, a resolution, a module list, the initial file list, experimentalStats,
// dependencyTypes). Tests that call none of them exercise dependency-cruiser's internal helpers
// or its transpiler wrappers; they are listed in INDEX.json under `notLayer1`, grouped by spec
// with the reason from NOT_LAYER_1_REASONS, so the denominator is auditable.
import {
    copyFileSync,
    cpSync,
    existsSync,
    lstatSync,
    mkdirSync,
    readdirSync,
    realpathSync,
    writeFileSync,
} from 'node:fs';
import { register } from 'node:module';
import { dirname, join, relative, resolve } from 'node:path';

const [upstreamArgument, outArgument] = process.argv.slice(2);
if (!upstreamArgument || !outArgument) {
    console.error('usage: node export-expectations.mjs <dependency-cruiser checkout> <output dir>');
    process.exit(2);
}
// The physical path: module URLs are physical, and the hooks match on them (macOS /var is a symlink).
const upstream = realpathSync(resolve(upstreamArgument));
const out = resolve(outArgument);

// Upstream specs resolve fixtures relative to the working directory, as `npm test` does.
process.chdir(upstream);
process.env.NO_COLOR = '1';

register('./hooks.mjs', import.meta.url, { data: { upstream } });
const recorder = await import('./recorder.mjs');
recorder.configure(upstream);

const requireFromUpstream = (await import('node:module')).createRequire(
    join(upstream, 'package.json'),
);
const Mocha = requireFromUpstream('mocha');

/** Why the tests in a spec that calls no surface are outside layer 1, keyed by spec path prefix. */
const NOT_LAYER_1_REASONS = [
    [
        'test/extract/transpile/',
        'asserts the output of a transpiler dependency-cruiser wraps (Babel, TypeScript, Svelte, Vue, CoffeeScript, LiveScript): generated JavaScript, not a graph. oxc parses TypeScript and JSX directly; Vue and Svelte script blocks are plan 0001; CoffeeScript and LiveScript go to the sidecar (ADR-0017)',
    ],
    [
        'test/extract/acorn/parse.spec.mjs',
        "asserts the shape of acorn's AST, a parser Rulebearing does not use (ADR-0012)",
    ],
    [
        'test/extract/acorn/extract-stats.spec.mjs',
        'hand-built acorn AST objects; the file-level statistics in the same spec are recorded',
    ],
    [
        'test/extract/tsc/extract-stats.spec.mjs',
        'hand-built TypeScript AST objects; the file-level statistics in the same spec are recorded',
    ],
    [
        'test/extract/helpers-',
        "an internal helper of dependency-cruiser's extractor (equality, query stripping, module attributes, pre-compilation detection); its effect reaches the cruise result only through the recorded surfaces",
    ],
    [
        'test/extract/resolve/',
        "an internal helper of dependency-cruiser's resolver (manifest reading and merging, module classifiers, built-in lists, licence and deprecation lookups); its effect reaches the cruise result only through resolve() and determineDependencyTypes(), which are recorded",
    ],
];

function reasonFor(spec) {
    const match = NOT_LAYER_1_REASONS.find(([prefix]) => spec.startsWith(prefix));
    return match ? match[1] : 'calls no extraction surface';
}

function walk(directory) {
    return readdirSync(directory)
        .sort()
        .flatMap((entry) => {
            const path = join(directory, entry);
            return lstatSync(path).isDirectory() ? walk(path) : [path];
        });
}

const specs = walk(join(upstream, 'test', 'extract')).filter((file) => file.endsWith('.spec.mjs'));

const perSpec = new Map();
const mocha = new Mocha({
    timeout: 20_000,
    reporter: 'base',
    rootHooks: {
        beforeEach() {
            recorder.begin();
        },
        afterEach() {
            const test = this.currentTest;
            const spec = relative(upstream, test.file).split('\\').join('/');
            const calls = recorder.end();
            if (!perSpec.has(spec)) {
                perSpec.set(spec, []);
            }
            perSpec.get(spec).push({ title: test.fullTitle(), state: test.state, calls });
        },
    },
});
for (const spec of specs) {
    mocha.addFile(spec);
}
await mocha.loadFilesAsync();
const failures = await new Promise((done) => {
    mocha.run(done);
});

const index = {
    pin: requireFromUpstream('./package.json').version,
    tests: 0,
    failedUpstream: failures,
    cases: 0,
    bySurface: {},
    specs: [],
    notLayer1: [],
};

for (const spec of [...perSpec.keys()].sort()) {
    const tests = perSpec.get(spec);
    index.tests += tests.length;
    const cases = [];
    const outside = [];
    for (const test of tests) {
        if (test.state !== 'passed') {
            // A test failing upstream specifies nothing; it is reported and stops the export below.
            outside.push({
                title: test.title,
                reason: `failed under dependency-cruiser ${index.pin}`,
            });
            continue;
        }
        if (test.calls.length === 0) {
            outside.push({ title: test.title, reason: reasonFor(spec) });
            continue;
        }
        test.calls.forEach((call, position) => {
            const id = `${spec}#${cases.length + 1}`;
            cases.push({ id, title: test.title, call: position + 1, ...call });
            index.bySurface[call.surface] = (index.bySurface[call.surface] ?? 0) + 1;
        });
    }
    if (cases.length > 0) {
        const file = `expectations/${spec.replace(/^test\/extract\//u, '').replace(/\.spec\.mjs$/u, '.json')}`;
        mkdirSync(dirname(join(out, file)), { recursive: true });
        writeFileSync(join(out, file), `${JSON.stringify(cases, null, 2)}\n`);
        index.specs.push({ spec, file, cases: cases.length, tests: tests.length });
        index.cases += cases.length;
    }
    if (outside.length > 0) {
        index.notLayer1.push({ spec, reason: reasonFor(spec), tests: outside.map((o) => o.title) });
    }
}
index.bySurface = Object.fromEntries(Object.entries(index.bySurface).sort());

// The inputs the recorded calls read: test/extract verbatim, specs included for reference.
const inputs = join(out, 'test', 'extract');
mkdirSync(inputs, { recursive: true });
cpSync(join(upstream, 'test', 'extract'), inputs, { recursive: true, verbatimSymlinks: true });

// Bare specifiers resolve against the checkout's own package.json and node_modules, and the npm
// dependency types and licences come from them. Only what the expectations name is copied: the
// root manifest, and for each root-level package the expectations resolve into, its package.json
// and the resolved files.
copyFileSync(join(upstream, 'package.json'), join(out, 'package.json'));
const rootPackages = new Map();
function collect(value) {
    if (Array.isArray(value)) {
        value.forEach(collect);
    } else if (value !== null && typeof value === 'object') {
        for (const [key, inner] of Object.entries(value)) {
            const match =
                key === 'resolved' && typeof inner === 'string'
                    ? /^(?:\.\.\/)*node_modules\/((?:@[^/]+\/)?[^/]+)\/(.+)$/u.exec(inner)
                    : null;
            if (match) {
                const files = rootPackages.get(match[1]) ?? new Set(['package.json']);
                files.add(match[2]);
                rootPackages.set(match[1], files);
            }
            collect(inner);
        }
    }
}
for (const { file } of index.specs) {
    const recorded = (await import(`file://${join(out, file)}`, { with: { type: 'json' } }))
        .default;
    recorded.forEach((entry) => collect(entry.expected));
}
index.rootPackages = {};
for (const [name, files] of [...rootPackages].sort(([a], [b]) => a.localeCompare(b))) {
    const copied = [];
    for (const file of [...files].sort()) {
        const from = join(upstream, 'node_modules', name, file);
        if (existsSync(from) && lstatSync(from).isFile()) {
            const to = join(out, 'node_modules', name, file);
            mkdirSync(dirname(to), { recursive: true });
            copyFileSync(from, to);
            copied.push(file);
        }
    }
    if (copied.length > 1) {
        index.rootPackages[name] = copied;
    }
}

writeFileSync(join(out, 'INDEX.json'), `${JSON.stringify(index, null, 2)}\n`);
console.log(
    `export-expectations: ${index.tests} upstream tests, ${index.cases} layer 1 cases across ${index.specs.length} specs, ` +
        `${index.notLayer1.reduce((n, s) => n + s.tests.length, 0)} tests outside layer 1; upstream failures: ${failures}`,
);
if (failures > 0) {
    console.error(
        'export-expectations: upstream tests failed; the recording is not a specification',
    );
    process.exit(1);
}
