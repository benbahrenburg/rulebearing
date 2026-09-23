// Conformance gate 1, layer 3: dependency-cruiser 18.2.0's report specs for the wave 1 reporters,
// run unmodified, with each reporter import forwarded to `rulebearing report` (layer3-hooks.mjs).
// The specs compare output byte for byte against upstream's fixtures (teamcity's per-session
// flowId and timestamp aside, which the spec itself removes), so a pass is a byte-compare pass.
//
// Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 12. Decision:
// docs/adr/0009-conformance-suites-as-specification.md.
// Usage: node run-layer-3.mjs <dependency-cruiser checkout> [spec ...]
import { realpathSync } from 'node:fs';
import { register, createRequire } from 'node:module';
import { join, relative, resolve } from 'node:path';

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
const only = args.filter((arg) => arg.endsWith('.spec.mjs'));
const specs = only.length > 0 ? only : WAVE_1;

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
process.exit(failedSpecs.size > 0 ? 1 : 0);
