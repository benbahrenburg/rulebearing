// Writes every cruise-result mock under dependency-cruiser's test/report as a JSON file, so the
// rb-model round-trip test (crates/rb-model/tests/round_trip.rs) can read them without Node.
//
// Plan: docs/plans/pending/0000-wave-0-spike.md, Step 3 item 7. Called by scripts/vendor.sh.
// Usage: node export-report-mocks.mjs <dependency-cruiser checkout> <output directory>
//
// A mock counts when its default export has both `modules` and `summary`, the two keys the
// cruise-result schema requires. JSON mocks are already vendored verbatim under fixtures/report/,
// so they are only listed; `.mjs` mocks are imported and serialised into the output directory,
// mirroring their relative path. Each is validated against the pinned cruise-result schema with
// the checkout's own ajv: the ones that validate are what dependency-cruiser 18.2.0 can write, and
// INDEX.json lists them under `valid`; the hand-written mocks that do not (some are deliberately
// malformed, to test a reporter's robustness) are listed under `invalid` with ajv's first error.
// Paths are relative to the fixtures directory and sorted, so a re-vendor gives a reviewable diff.
import { mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';

const [upstream, out] = process.argv.slice(2);
if (!upstream || !out) {
    console.error('usage: node export-report-mocks.mjs <dependency-cruiser checkout> <output dir>');
    process.exit(2);
}
const reportRoot = resolve(upstream, 'test', 'report');
const requireUpstream = createRequire(join(resolve(upstream), 'package.json'));
const Ajv = requireUpstream('ajv').default;
const schema = JSON.parse(
    readFileSync(join(upstream, 'src', 'schema', 'cruise-result.schema.json'), 'utf8'),
);
const validate = new Ajv({ allErrors: false, strict: false }).compile(schema);

function walk(directory) {
    return readdirSync(directory)
        .sort()
        .flatMap((entry) => {
            const path = join(directory, entry);
            return statSync(path).isDirectory() ? walk(path) : [path];
        });
}

function isCruiseResult(value) {
    return value !== null && typeof value === 'object' && 'modules' in value && 'summary' in value;
}

const fixtures = dirname(resolve(out));
const listed = { valid: [], invalid: [] };
for (const file of walk(reportRoot)) {
    const isJson = file.endsWith('.json');
    const isModule = file.endsWith('.mjs') && !file.endsWith('.spec.mjs');
    if (!isJson && !isModule) {
        continue;
    }
    const value = isJson
        ? JSON.parse(readFileSync(file, 'utf8'))
        : (await import(pathToFileURL(file).href)).default;
    if (!isCruiseResult(value)) {
        continue;
    }
    let target = join(fixtures, 'report', relative(reportRoot, file));
    if (isModule) {
        target = join(out, relative(reportRoot, file).replace(/\.mjs$/u, '.json'));
        mkdirSync(dirname(target), { recursive: true });
        writeFileSync(target, `${JSON.stringify(value, null, 2)}\n`);
    }
    const name = relative(fixtures, target).split('\\').join('/');
    if (validate(value)) {
        listed.valid.push(name);
    } else {
        const [first] = validate.errors ?? [];
        listed.invalid.push({
            file: name,
            reason: `${first?.instancePath || '/'} ${first?.message ?? 'invalid'}`,
        });
    }
}
listed.valid.sort();
listed.invalid.sort((a, b) => a.file.localeCompare(b.file));
writeFileSync(join(out, 'INDEX.json'), `${JSON.stringify(listed, null, 2)}\n`);
console.log(
    `export-report-mocks: ${listed.valid.length} valid and ${listed.invalid.length} invalid cruise results listed`,
);
