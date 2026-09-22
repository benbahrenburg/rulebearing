// Conformance gate 1 layer 2: dependency-cruiser's own test/validate and test/graph-utl specs,
// run unmodified, with the unit under test replaced by Rulebearing (layer2-hooks.mjs, shim.mjs).
//
// Plan: docs/plans/pending/0000-wave-0-spike.md, Step 5 items 3 and 4.
// Decision: docs/adr/0009-conformance-suites-as-specification.md (excluded.json may only shrink).
//
// Usage: node run-layer-2.mjs <dependency-cruiser checkout> [--record]
//
//   gate mode (default)  every spec runs. A failure in a spec not listed in excluded.json fails
//                        the run; a listed spec that now passes is reported so the list can shrink.
//   --record             rewrites conformance/excluded.json with the specs that fail now, keeping
//                        each surviving entry's reason. Run it only to shrink the list: the
//                        ratchet job rejects a pull request that grows it.
import { readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { createRequire, register } from 'node:module';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const excludedPath = join(here, '..', '..', 'excluded.json');
const args = process.argv.slice(2);
const record = args.includes('--record');
const upstreamArgument = args.find((a) => !a.startsWith('--'));
if (!upstreamArgument) {
    console.error('usage: node run-layer-2.mjs <dependency-cruiser checkout> [--record]');
    process.exit(2);
}
const upstream = realpathSync(resolve(upstreamArgument));
process.chdir(upstream);
process.env.NO_COLOR = '1';

register('./layer2-hooks.mjs', import.meta.url);

const Mocha = createRequire(join(upstream, 'package.json'))('mocha');
const { globSync } = await import('node:fs');
const specs = ['test/validate', 'test/graph-utl']
    .flatMap((dir) => globSync(`${dir}/**/*.spec.mjs`, { cwd: upstream }))
    .map((spec) => spec.split('\\').join('/'))
    .sort();

const excludedFile = JSON.parse(readFileSync(excludedPath, 'utf8'));
const excluded = new Map(excludedFile['dependency-cruiser'].map((entry) => [entry.spec, entry]));

const failedSpecs = new Set();
const mocha = new Mocha({ timeout: 20_000, reporter: 'base' });
for (const spec of specs) {
    mocha.addFile(join(upstream, spec));
}
await mocha.loadFilesAsync();
const runner = mocha.run();
runner.on('fail', (test) => {
    const file = test.file ?? test.parent?.file;
    if (file) {
        failedSpecs.add(relative(upstream, file).split('\\').join('/'));
    }
});
await new Promise((done) => {
    runner.on('end', done);
});

const unexpected = specs.filter((spec) => failedSpecs.has(spec) && !excluded.has(spec));
const nowPassing = specs.filter((spec) => !failedSpecs.has(spec) && excluded.has(spec));
console.log(
    `layer2: specs=${specs.length} failing=${failedSpecs.size} excluded=${excluded.size} ` +
        `unexpected-failures=${unexpected.length} excluded-but-passing=${nowPassing.length}`,
);

if (record) {
    const kept = specs
        .filter((spec) => failedSpecs.has(spec))
        .map(
            (spec) =>
                excluded.get(spec) ?? {
                    spec,
                    reason: 'wave-1: rule engine not yet implemented',
                    plan: '0001',
                },
        );
    excludedFile['dependency-cruiser'] = kept;
    writeFileSync(excludedPath, `${JSON.stringify(excludedFile, null, 2)}\n`);
    console.log(`layer2: wrote ${kept.length} entries to conformance/excluded.json`);
    process.exit(0);
}
for (const spec of nowPassing) {
    console.log(`layer2: ${spec} passes; remove it from conformance/excluded.json`);
}
for (const spec of unexpected) {
    console.error(`layer2: ${spec} fails and is not excluded`);
}
process.exit(unexpected.length > 0 ? 1 : 0);
