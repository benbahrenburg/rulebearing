// The playground's wasm module, as a browser loads it, run under Node: `evaluate()` over each
// guard-catalogue fixture must find what the command line found (the fixture's `expected.json`,
// which the `guards` job byte-compares against the CLI), a JavaScript configuration is refused
// with the way out, and the module is under 8 MB compressed.
//
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 2 ("comparing `evaluate()` output to the
// committed CLI JSON"; "a size check"). Requirement: docs/prd.md#fr-reach-01.
//
//   node crates/rb-wasm/tests/wasm.mjs <wasm-bindgen --target nodejs output dir>
//
// The output is CommonJS: inside this repository, whose package.json says "module", the output
// directory needs a package.json of `{ "type": "commonjs" }` (the `wasm` CI job writes one).

import assert from 'node:assert/strict';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../../..');
const out = path.resolve(process.argv[2] ?? 'target/rb-wasm-node');
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

process.stdout.write(
    `rb-wasm: ${String(fixtures)} fixture(s) equal the CLI; ${String(compressed)} bytes compressed\n`,
);
