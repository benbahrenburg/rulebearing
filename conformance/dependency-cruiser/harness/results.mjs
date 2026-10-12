// Writes a gate's results file, target/conformance/<name>.json, in the one shape the live coverage
// tables read (testbeds/scripts/coverage_status.py): each item that passed or failed, and whether
// all of them passed. Keys are sorted so two runs over the same results write the same bytes.
//
// Plan: docs/plans/pending/0004-wave-4-reach.md, Step 4. Decisions:
// docs/adr/0009-conformance-suites-as-specification.md,
// docs/adr/0074-the-coverage-ledger-is-tracked-and-its-status-is-computed.md.
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');

/**
 * The results document for `items` (item to `true` when it passed).
 * @param {string} name
 * @param {Map<string, boolean>} items
 */
export function resultsDocument(name, items) {
    const sorted = [...items.entries()].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
    const passed = sorted.filter(([, ok]) => ok).length;
    return {
        items: Object.fromEntries(sorted.map(([item, ok]) => [item, ok ? 'pass' : 'fail'])),
        ok: sorted.length > 0 && passed === sorted.length,
        passed,
        results: name,
        total: sorted.length,
    };
}

/**
 * Writes target/conformance/<name>.json and returns its path.
 * @param {string} name
 * @param {Map<string, boolean>} items
 */
export function writeResults(name, items) {
    const path = join(root, 'target', 'conformance', `${name}.json`);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, `${JSON.stringify(resultsDocument(name, items), null, 2)}\n`);
    return path;
}
