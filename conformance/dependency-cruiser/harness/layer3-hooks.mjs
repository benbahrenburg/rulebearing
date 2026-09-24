// Module loader hooks for run-layer-3.mjs: when a dependency-cruiser report spec imports one of
// the reporters, it gets a module whose default export forwards to `rulebearing report`, so
// upstream's own report specs run unmodified against Rulebearing's reporters. A spec that tests a
// reporter's internals (theming, module-utl, error-html utl) gets each export forwarded to
// `rulebearing validate`, the layer 2 protocol, which the reporters answer for `#report/` modules.
//
// Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 12 (gate 1 layer 3);
// docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 10 (the wave 2 reporters).
// Protocol: `rulebearing report --output-type <type>` reads { result, options } on stdin and
// answers { output, exitCode }, the value a dependency-cruiser reporter returns.

const FORWARD = 'rb-report:';
const FORWARD_DOT = 'rb-report-dot:factory';
const FORWARD_MODULE = 'rb-forward:';
const REPORTERS = new Map([
    ['#report/error.mjs', 'err'],
    ['#report/error-long.mjs', 'err-long'],
    ['#report/text.mjs', 'text'],
    ['#report/csv.mjs', 'csv'],
    ['#report/teamcity.mjs', 'teamcity'],
    ['#report/azure-devops.mjs', 'azure-devops'],
    ['#report/null.mjs', 'null'],
    ['#report/json.mjs', 'json'],
    ['#report/mermaid.mjs', 'mermaid'],
    ['dependency-cruiser/mermaid-reporter-plugin', 'mermaid'],
    ['#report/d2.mjs', 'd2'],
    ['#report/metrics.mjs', 'metrics'],
    ['#report/error-html/index.mjs', 'err-html'],
]);
// `#report/dot/index.mjs` exports the factory `dot(granularity)`; each granularity is an output
// type (`dot()` without one renders at module level, as `dot`).
const DOT_FACTORY = '#report/dot/index.mjs';
const GRANULARITY_TYPES = { module: 'dot', folder: 'ddot', custom: 'archi', flat: 'flat' };
// Reporter internals whose unit specs call them directly.
const INTERNALS = [
    '#report/dot/theming.mjs',
    '#report/dot/module-utl.mjs',
    '#report/error-html/utl.mjs',
];
const SHIM = new URL('./shim.mjs', import.meta.url).href;

export async function resolve(specifier, context, nextResolve) {
    if (!context.parentURL?.endsWith('.spec.mjs')) {
        return nextResolve(specifier, context);
    }
    const outputType = REPORTERS.get(specifier);
    if (outputType) {
        return { url: `${FORWARD}${outputType}`, shortCircuit: true };
    }
    if (specifier === DOT_FACTORY) {
        return { url: FORWARD_DOT, shortCircuit: true };
    }
    if (INTERNALS.includes(specifier)) {
        const original = await nextResolve(specifier, context);
        const target = encodeURIComponent(original.url);
        return {
            url: `${FORWARD_MODULE}${encodeURIComponent(specifier)}?target=${target}`,
            shortCircuit: true,
        };
    }
    return nextResolve(specifier, context);
}

async function forwardedModule(url) {
    const [encodedSpecifier, query] = url.slice(FORWARD_MODULE.length).split('?target=');
    const specifier = decodeURIComponent(encodedSpecifier);
    const target = decodeURIComponent(query);
    const exported = Object.keys(await import(target));
    const lines = [
        `import * as original from ${JSON.stringify(target)};`,
        `import { forward } from ${JSON.stringify(SHIM)};`,
    ];
    for (const name of exported) {
        const value = `forward(${JSON.stringify(specifier)}, ${JSON.stringify(name)}, original[${JSON.stringify(name)}])`;
        lines.push(
            name === 'default' ? `export default ${value};` : `export const ${name} = ${value};`,
        );
    }
    return lines.join('\n');
}

export async function load(url, context, nextLoad) {
    if (url === FORWARD_DOT) {
        const source = [
            `import { report } from ${JSON.stringify(SHIM)};`,
            `const TYPES = ${JSON.stringify(GRANULARITY_TYPES)};`,
            `export default (granularity) => (result, options) => report(TYPES[granularity] ?? 'dot', result, options);`,
        ].join('\n');
        return { format: 'module', shortCircuit: true, source };
    }
    if (url.startsWith(FORWARD_MODULE)) {
        return { format: 'module', shortCircuit: true, source: await forwardedModule(url) };
    }
    if (!url.startsWith(FORWARD)) {
        return nextLoad(url, context);
    }
    const outputType = url.slice(FORWARD.length);
    const source = [
        `import { report } from ${JSON.stringify(SHIM)};`,
        `export default (result, options) => report(${JSON.stringify(outputType)}, result, options);`,
    ].join('\n');
    return { format: 'module', shortCircuit: true, source };
}
