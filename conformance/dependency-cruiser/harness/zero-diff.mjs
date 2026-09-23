// Conformance gate 1, layer 5: the oracle zero-diff. Compares dependency-cruiser's cruise result
// with Rulebearing's for the same repository, tree, configuration and roots, after sorting, and
// fails on any difference that conformance/divergences.md does not record with a reason.
//
// Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 18. Requirements:
// docs/prd.md#nfr-conf-01, docs/prd.md#nfr-conf-03. Decision:
// docs/adr/0009-conformance-suites-as-specification.md. Runner: ../scripts/run-layer-5.sh.
//
// Usage:
//   node zero-diff.mjs --incumbent <dc.json> --rulebearing <rb.json> --repo <owner/name>
//                      [--divergences <divergences.md>] [--expect <expected.json>] [--max <n>]
//
// What is compared (every field dependency-cruiser writes, nothing it does not):
//   modules      the set of `source`s, and every module-level field
//   dependencies each module's `dependencies[]`, sorted, every field
//   violations   `summary.violations`, sorted, every field
// Rulebearing's additions are dropped before comparing, because dependency-cruiser has no value to
// compare them with (docs/adr/0004-graph-document-is-cruise-result-superset.md): `line`,
// `column`, `dependencyKind` and `language` on a dependency or module, and `id`, `fix` and
// `decision` on a violation.
//
// Each difference is one line whose first words are a stable key:
//   module-missing <source> | module-extra <source>
//   module-field <source> <field>
//   dependency-missing <source> -> <resolved> | dependency-extra <source> -> <resolved>
//   dependency-field <source> -> <resolved> <field>
//   violation-missing <rule> <from> -> <to> | violation-extra <rule> <from> -> <to>
// A row of divergences.md whose repository matches (or is `*`) and whose `Match` regular
// expression matches the key accepts that difference. Exit 0: no unaccepted difference. Exit 1:
// at least one. Exit 2: bad input.
//
// `--expect <expected.json>` (the mutation branch, ../mutations/): additionally, each tool's
// `summary.violations` must be exactly the listed `{ rule, from, to }` entries, else exit 1.
import { readFileSync } from 'node:fs';

const DEPENDENCY_ADDITIONS = new Set(['line', 'column', 'dependencyKind', 'language']);
const MODULE_ADDITIONS = new Set(['language']);
const VIOLATION_ADDITIONS = new Set(['id', 'fix', 'decision']);

function parseArguments(argv) {
    const options = { max: 60 };
    for (let index = 0; index < argv.length; index += 1) {
        const name = argv[index];
        const value = argv[index + 1];
        switch (name) {
            case '--incumbent':
            case '--rulebearing':
            case '--repo':
            case '--divergences':
            case '--expect':
            case '--max':
                if (value === undefined) {
                    throw new Error(`${name} needs a value`);
                }
                options[name.slice(2)] = value;
                index += 1;
                break;
            default:
                throw new Error(`unknown argument ${name}`);
        }
    }
    for (const required of ['incumbent', 'rulebearing', 'repo']) {
        if (!options[required]) {
            throw new Error(`--${required} is required`);
        }
    }
    return options;
}

function readResult(path) {
    const result = JSON.parse(readFileSync(path, 'utf8'));
    if (!Array.isArray(result.modules) || !Array.isArray(result.summary?.violations)) {
        throw new Error(`${path} is not a cruise result`);
    }
    return result;
}

/** A value with every object's keys sorted, so two equal values stringify equally. */
function canonical(value) {
    if (Array.isArray(value)) {
        return value.map(canonical);
    }
    if (value !== null && typeof value === 'object') {
        return Object.fromEntries(
            Object.keys(value)
                .sort()
                .map((key) => [key, canonical(value[key])]),
        );
    }
    return value;
}

const text = (value) => JSON.stringify(canonical(value));

function without(object, dropped) {
    return Object.fromEntries(Object.entries(object).filter(([key]) => !dropped.has(key)));
}

/** A dependency's identity within its module: dependency-cruiser keeps one per resolved name. */
const dependencyKey = (dependency) => `${dependency.resolved} (${dependency.module})`;

function compareFields(left, right, report, prefix) {
    const fields = new Set([...Object.keys(left), ...Object.keys(right)]);
    for (const field of [...fields].sort()) {
        const a = text(left[field]);
        const b = text(right[field]);
        if (a !== b) {
            report(
                `${prefix} ${field}`,
                `dependency-cruiser ${a ?? '(absent)'}, rulebearing ${b ?? '(absent)'}`,
            );
        }
    }
}

function byKey(items, key) {
    const map = new Map();
    for (const item of items) {
        const k = key(item);
        map.set(k, [...(map.get(k) ?? []), item]);
    }
    return map;
}

function compareDependencies(source, left, right, report) {
    const leftMap = byKey(left, dependencyKey);
    const rightMap = byKey(right, dependencyKey);
    const keys = [...new Set([...leftMap.keys(), ...rightMap.keys()])].sort();
    for (const key of keys) {
        const as = leftMap.get(key) ?? [];
        const bs = rightMap.get(key) ?? [];
        const target = key.replace(/ \(.*$/u, '');
        const count = Math.max(as.length, bs.length);
        for (let index = 0; index < count; index += 1) {
            const a = as[index];
            const b = bs[index];
            if (a === undefined) {
                report(`dependency-extra ${source} -> ${target}`, text(b));
            } else if (b === undefined) {
                report(`dependency-missing ${source} -> ${target}`, text(a));
            } else {
                compareFields(a, b, report, `dependency-field ${source} -> ${target}`);
            }
        }
    }
}

// A source can stand for more than one module (an unfollowed dependency reached twice), so
// modules are paired in order within each source.
function compareModules(left, right, report) {
    const leftMap = byKey(left, (module) => module.source);
    const rightMap = byKey(right, (module) => module.source);
    const pairs = [...new Set([...leftMap.keys(), ...rightMap.keys()])].sort().flatMap((source) => {
        const as = leftMap.get(source) ?? [];
        const bs = rightMap.get(source) ?? [];
        return Array.from({ length: Math.max(as.length, bs.length) }, (_, at) => [
            source,
            as[at],
            bs[at],
        ]);
    });
    for (const [source, a, b] of pairs) {
        if (a === undefined) {
            report(`module-extra ${source}`, '');
            continue;
        }
        if (b === undefined) {
            report(`module-missing ${source}`, '');
            continue;
        }
        const { dependencies: aDependencies, ...aFields } = a;
        const { dependencies: bDependencies, ...bFields } = without(b, MODULE_ADDITIONS);
        compareFields(aFields, bFields, report, `module-field ${source}`);
        compareDependencies(
            source,
            aDependencies ?? [],
            (bDependencies ?? []).map((dependency) => without(dependency, DEPENDENCY_ADDITIONS)),
            report,
        );
    }
}

const violationKey = (violation) =>
    `${violation.rule?.name ?? '?'} ${violation.from} -> ${violation.to}`;

function compareViolations(left, right, report) {
    const leftMap = byKey(left, violationKey);
    const rightMap = byKey(
        right.map((violation) => without(violation, VIOLATION_ADDITIONS)),
        violationKey,
    );
    const keys = [...new Set([...leftMap.keys(), ...rightMap.keys()])].sort();
    for (const key of keys) {
        const as = (leftMap.get(key) ?? []).map(text).sort();
        const bs = (rightMap.get(key) ?? []).map(text).sort();
        const bRemaining = [...bs];
        for (const a of as) {
            const at = bRemaining.indexOf(a);
            if (at === -1) {
                report(`violation-missing ${key}`, a);
            } else {
                bRemaining.splice(at, 1);
            }
        }
        for (const b of bRemaining) {
            report(`violation-extra ${key}`, b);
        }
    }
}

/** The rows of divergences.md's table: `| Repository | Match | What differs | Reason | Link |`. */
function readDivergences(path) {
    if (!path) {
        return [];
    }
    const rows = [];
    for (const line of readFileSync(path, 'utf8').split('\n')) {
        // A `|` inside a cell is written `\|`, as Markdown tables require.
        const cells = line
            .split(/(?<!\\)\|/u)
            .slice(1, -1)
            .map((cell) => cell.trim().replaceAll('\\|', '|'));
        if (cells.length < 5 || !cells[1].startsWith('`')) {
            continue;
        }
        const pattern = cells[1].replace(/^`|`$/gu, '');
        rows.push({ repo: cells[0].replace(/`/gu, ''), match: new RegExp(pattern, 'u') });
    }
    return rows;
}

const expectedKey = ({ rule, from, to }) => `${rule} ${from} -> ${to}`;

/** The expected violations, `[{ rule, from, to }]`, one or more per rule. */
function readExpected(path) {
    if (!path) {
        return undefined;
    }
    const expected = JSON.parse(readFileSync(path, 'utf8'));
    if (!Array.isArray(expected) || expected.some((entry) => !entry.rule)) {
        throw new Error(`${path} is not a list of { rule, from, to }`);
    }
    return expected;
}

/**
 * Whether each tool reported exactly the expected violations: every one, and nothing else. One
 * line per rule says what each tool found.
 */
function checkExpected(expected, incumbent, rulebearing) {
    const found = (result) =>
        result.summary.violations
            .map((violation) =>
                expectedKey({ rule: violation.rule?.name, from: violation.from, to: violation.to }),
            )
            .sort();
    const want = expected.map(expectedKey).sort();
    const tools = { 'dependency-cruiser': found(incumbent), rulebearing: found(rulebearing) };
    let ok = true;
    for (const rule of [...new Set(expected.map((entry) => entry.rule))]) {
        const mine = want.filter((key) => key.startsWith(`${rule} `));
        const verdicts = Object.entries(tools).map(([tool, keys]) => {
            const got = keys.filter((key) => key.startsWith(`${rule} `));
            const same = got.length === mine.length && got.every((key, at) => key === mine[at]);
            ok &&= same;
            return `${tool} ${String(got.length)}${same ? '' : ' (expected ' + String(mine.length) + ')'}`;
        });
        console.log(`layer5: mutation ${rule}: ${verdicts.join(', ')}`);
    }
    for (const [tool, keys] of Object.entries(tools)) {
        for (const key of keys.filter((k) => !want.includes(k))) {
            console.log(`layer5: mutation unexpected from ${tool}: ${key}`);
            ok = false;
        }
    }
    console.log(
        `layer5: mutations ${ok ? 'all reported, by both tools, and nothing else' : 'FAILED'}`,
    );
    return ok;
}

function main() {
    let options;
    let incumbent;
    let rulebearing;
    let divergences;
    let expected;
    try {
        options = parseArguments(process.argv.slice(2));
        incumbent = readResult(options.incumbent);
        rulebearing = readResult(options.rulebearing);
        divergences = readDivergences(options.divergences);
        expected = readExpected(options.expect);
    } catch (error) {
        console.error(`layer5: ${error instanceof Error ? error.message : String(error)}`);
        return 2;
    }

    const differences = [];
    const report = (key, detail) => {
        differences.push({ key, detail });
    };
    compareModules(incumbent.modules, rulebearing.modules, report);
    compareViolations(incumbent.summary.violations, rulebearing.summary.violations, report);

    const applies = (row) => row.repo === '*' || row.repo === options.repo;
    const accepted = [];
    const unaccepted = [];
    for (const difference of differences) {
        const row = divergences.find((r) => applies(r) && r.match.test(difference.key));
        (row ? accepted : unaccepted).push(difference);
    }

    const max = Number(options.max);
    for (const difference of unaccepted.slice(0, max)) {
        const detail = difference.detail ? `\n    ${difference.detail.slice(0, 400)}` : '';
        console.log(`  ${difference.key}${detail}`);
    }
    if (unaccepted.length > max) {
        console.log(`  ... and ${String(unaccepted.length - max)} more`);
    }

    let failed = unaccepted.length > 0;
    if (expected) {
        failed = !checkExpected(expected, incumbent, rulebearing) || failed;
    }
    const counts = (result) =>
        `${String(result.modules.length)} modules, ${String(result.summary.violations.length)} violations`;
    console.log(
        `layer5: ${options.repo}: dependency-cruiser ${counts(incumbent)}; rulebearing ${counts(rulebearing)}; ` +
            `${String(differences.length)} differences, ${String(accepted.length)} documented, ${String(unaccepted.length)} undocumented`,
    );
    return failed ? 1 : 0;
}

process.exitCode = main();
