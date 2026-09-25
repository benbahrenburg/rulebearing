// Module loader hooks for run-layer-2.mjs: when a dependency-cruiser spec imports a #validate or
// #graph-utl module, it gets a module whose exports forward to `rulebearing validate` (shim.mjs).
// Imports of the same modules from anywhere else, including from the upstream source itself,
// are left alone, so only the unit under test is replaced.
//
// Plan: docs/plans/pending/0000-wave-0-spike.md, Step 5 item 4.

const SHIM = new URL('./shim.mjs', import.meta.url).href;
const FORWARD = 'rb-forward:';
const REMAPPED = ['#validate/', '#graph-utl/'];

export async function resolve(specifier, context, nextResolve) {
    const remapped = REMAPPED.some((prefix) => specifier.startsWith(prefix));
    if (remapped && context.parentURL?.endsWith('.spec.mjs')) {
        const original = await nextResolve(specifier, context);
        const target = encodeURIComponent(original.url);
        return {
            url: `${FORWARD}${encodeURIComponent(specifier)}?target=${target}`,
            shortCircuit: true,
        };
    }
    return nextResolve(specifier, context);
}

export async function load(url, context, nextLoad) {
    if (!url.startsWith(FORWARD)) {
        return nextLoad(url, context);
    }
    const [encodedSpecifier, query] = url.slice(FORWARD.length).split('?target=');
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
    return { format: 'module', shortCircuit: true, source: lines.join('\n') };
}
