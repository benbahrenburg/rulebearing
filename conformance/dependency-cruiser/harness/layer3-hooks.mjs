// Module loader hooks for run-layer-3.mjs: when a dependency-cruiser report spec imports one of
// the wave 1 reporters, it gets a module whose default export forwards to `rulebearing report`,
// so upstream's own report specs run unmodified against Rulebearing's reporters.
//
// Plan: docs/plans/pending/0001-wave-1-typescript-parity.md, Step 12 (gate 1 layer 3).
// Protocol: `rulebearing report --output-type <type>` reads { result, options } on stdin and
// answers { output, exitCode }, the value a dependency-cruiser reporter returns.

const FORWARD = 'rb-report:';
const REPORTERS = new Map([
    ['#report/error.mjs', 'err'],
    ['#report/error-long.mjs', 'err-long'],
    ['#report/text.mjs', 'text'],
    ['#report/csv.mjs', 'csv'],
    ['#report/teamcity.mjs', 'teamcity'],
    ['#report/azure-devops.mjs', 'azure-devops'],
    ['#report/null.mjs', 'null'],
    ['#report/json.mjs', 'json'],
    ['#report/baseline.mjs', 'baseline'],
]);
const SHIM = new URL('./shim.mjs', import.meta.url).href;

export function resolve(specifier, context, nextResolve) {
    const outputType = REPORTERS.get(specifier);
    if (outputType && context.parentURL?.endsWith('.spec.mjs')) {
        return { url: `${FORWARD}${outputType}`, shortCircuit: true };
    }
    return nextResolve(specifier, context);
}

export function load(url, context, nextLoad) {
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
