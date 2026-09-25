// Module loader hooks for export-expectations.mjs: wraps dependency-cruiser's extraction surfaces
// so recorder.mjs sees every call a test makes to them. The modules are loaded unmodified (as
// `<url>?rb-original`); what a spec imports is a thin module that re-exports everything and
// replaces the wrapped export with a recording one.
//
// Plan: docs/plans/pending/0000-wave-0-spike.md, Step 5 item 1. Registered by export-expectations.mjs.

const RECORDER = new URL('./recorder.mjs', import.meta.url).href;
let sourceRoot = '';

/**
 * What to wrap, keyed by the path under the checkout's `src/`. `default` wraps the default export
 * as a recorded surface; `named` wraps named exports, either as surfaces or as taggers that let
 * the recorder map an AST or a normalised options object back to the input that produced it.
 */
const TARGETS = {
    'extract/tsc/parse.mjs': { named: { getASTFromSource: 'tagAst:tsc' } },
    'extract/acorn/parse.mjs': { named: { getASTFromSource: 'tagAst:acorn' } },
    'extract/swc/parse.mjs': { named: { getASTFromSource: 'tagAst:swc' } },
    'extract/tsc/extract-typescript-deps.mjs': { default: 'walk-tsc' },
    'extract/acorn/extract-cjs-deps.mjs': { default: 'walk-acorn-cjs' },
    'extract/acorn/extract-es6-deps.mjs': { default: 'walk-acorn-es6' },
    'extract/acorn/extract-amd-deps.mjs': { default: 'walk-acorn-amd' },
    'extract/swc/extract-swc-deps.mjs': { default: 'walk-swc' },
    'extract/extract-dependencies.mjs': { default: 'extract-dependencies' },
    'extract/resolve/index.mjs': { default: 'resolve' },
    'extract/resolve/determine-dependency-types.mjs': { default: 'determine-dependency-types' },
    'extract/index.mjs': { default: 'extract' },
    'extract/gather-initial-sources.mjs': { default: 'gather-initial-sources' },
    'extract/extract-stats.mjs': { default: 'extract-stats' },
    'extract/acorn/extract.mjs': { named: { getStats: 'stats-acorn' } },
    'extract/tsc/extract.mjs': { named: { getStats: 'stats-tsc' } },
    'main/options/normalize.mjs': { named: { normalizeCruiseOptions: 'tagCruiseOptions' } },
    'main/resolve-options/normalize.mjs': { default: 'tagResolveOptions' },
};

export function initialize({ upstream }) {
    sourceRoot = new URL(`file://${upstream.split('\\').join('/')}/src/`).href;
}

function wrapper(kind, reference) {
    if (kind.startsWith('tagAst:')) {
        return `tagAst(${JSON.stringify(kind.slice('tagAst:'.length))}, ${reference})`;
    }
    if (kind === 'tagCruiseOptions' || kind === 'tagResolveOptions') {
        return `${kind}(${reference})`;
    }
    return `surface(${JSON.stringify(kind)}, ${reference})`;
}

export async function load(url, context, nextLoad) {
    if (!url.startsWith(sourceRoot) || url.includes('?rb-original')) {
        return nextLoad(url, context);
    }
    const target = TARGETS[url.slice(sourceRoot.length)];
    if (!target) {
        return nextLoad(url, context);
    }
    const original = JSON.stringify(`${url}?rb-original`);
    const lines = [
        `import * as original from ${original};`,
        `export * from ${original};`,
        `import { surface, tagAst, tagCruiseOptions, tagResolveOptions } from ${JSON.stringify(RECORDER)};`,
    ];
    for (const [name, kind] of Object.entries(target.named ?? {})) {
        lines.push(`export const ${name} = ${wrapper(kind, `original.${name}`)};`);
    }
    lines.push(
        target.default
            ? `export default ${wrapper(target.default, 'original.default')};`
            : `export default original.default;`,
    );
    return { format: 'module', shortCircuit: true, source: lines.join('\n') };
}
