// Records calls to dependency-cruiser's extraction surfaces for export-expectations.mjs.
//
// Plan: docs/plans/pending/0000-wave-0-spike.md, Step 5 item 1.
//
// Only the outermost surface call of a test is recorded: extractDependencies calls resolve, which
// calls determineDependencyTypes, and the specification of the outer call already includes the
// inner ones. Arguments are reduced to plain JSON: an AST is replaced by the source text that
// produced it, a normalised options object by the raw options the test passed in, and every path
// argument is made relative to the checkout root so the Rust replay can run from the vendored copy.
// Absolute paths that remain inside option values (a webpack alias target, a tsconfig's outDir) are
// written with the checkout root replaced by the token `<root>`, which the replay maps back.
import { isAbsolute, relative, resolve } from 'node:path';

let root = '';
let depth = 0;
let calls = null;
const astSources = new WeakMap();
const rawCruiseOptions = new WeakMap();
const rawResolveOptions = new WeakMap();

export function configure(upstream) {
    root = upstream;
}

export function begin() {
    calls = [];
    depth = 0;
}

export function end() {
    const recorded = calls ?? [];
    calls = null;
    return recorded;
}

/** A path argument, relative to the checkout root and posix-separated. */
function rootRelative(path) {
    if (typeof path !== 'string') {
        return path;
    }
    const absolute = isAbsolute(path) ? path : resolve(process.cwd(), path);
    const inside = relative(root, absolute).split('\\').join('/');
    return inside === '' ? '.' : inside;
}

/** Plain JSON: drops functions, turns regular expressions into their source. */
function plain(value) {
    if (value === undefined) {
        return null;
    }
    return JSON.parse(
        JSON.stringify(value, (_key, inner) => {
            if (inner instanceof RegExp) {
                return inner.source;
            }
            if (typeof inner === 'function') {
                return undefined;
            }
            if (inner instanceof Map) {
                return Object.fromEntries(inner);
            }
            if (inner instanceof Set) {
                return [...inner];
            }
            return inner;
        }) ?? 'null',
    );
}

function cruiseOptions(value) {
    const raw = rawCruiseOptions.get(value);
    const options = plain(raw ?? value) ?? {};
    if (typeof options.baseDir === 'string') {
        options.baseDir = rootRelative(options.baseDir);
    }
    return { raw: raw !== undefined, options };
}

function resolveOptions(value) {
    const raw = rawResolveOptions.get(value);
    if (raw) {
        return raw;
    }
    // Passed as a plain object rather than through normalizeResolveOptions (unit tests do this).
    return { resolve: plain(value ?? {}), cruise: null, tsConfig: null, untagged: true };
}

export function tagAst(parser, getAST) {
    return function taggedGetAST(...args) {
        const ast = getAST.apply(this, args);
        const [record] = args;
        const source = typeof record === 'string' ? record : record?.source;
        const extension = typeof record === 'object' ? (record?.extension ?? null) : null;
        if (ast && typeof ast === 'object' && typeof source === 'string') {
            astSources.set(ast, { parser, source, extension });
        }
        return ast;
    };
}

export function tagCruiseOptions(normalize) {
    return function taggedNormalizeCruiseOptions(raw, ...rest) {
        const normalised = normalize.call(this, raw, ...rest);
        if (normalised && typeof normalised === 'object') {
            rawCruiseOptions.set(normalised, raw ?? {});
        }
        return normalised;
    };
}

export function tagResolveOptions(normalize) {
    return async function taggedNormalizeResolveOptions(raw, cruise, tsConfig) {
        const normalised = await normalize(raw, cruise, tsConfig);
        if (normalised && typeof normalised === 'object') {
            rawResolveOptions.set(normalised, {
                resolve: plain(raw ?? {}),
                cruise: cruiseOptions(cruise ?? {}).options,
                tsConfig: tsConfig === undefined ? null : plain(tsConfig),
            });
        }
        return normalised;
    };
}

/** How each surface's arguments become the recorded input. */
const INPUTS = {
    'walk-tsc': ([ast, exotic = [], jsdoc = false, builtin = false]) => ({
        ...astSources.get(ast),
        exoticRequireStrings: exotic,
        detectJSDocImports: jsdoc,
        detectProcessBuiltinModuleCalls: builtin,
    }),
    'walk-swc': ([ast, exotic = []]) => ({ ...astSources.get(ast), exoticRequireStrings: exotic }),
    'walk-acorn-cjs': ([ast, , moduleSystem, exotic = []]) => ({
        ...astSources.get(ast),
        form: 'cjs',
        moduleSystem,
        exoticRequireStrings: exotic,
    }),
    'walk-acorn-es6': ([ast]) => ({ ...astSources.get(ast), form: 'es6' }),
    'walk-acorn-amd': ([ast, , exotic = []]) => ({
        ...astSources.get(ast),
        form: 'amd',
        exoticRequireStrings: exotic,
    }),
    'extract-dependencies': ([fileName, cruise, resolveOpts, transpile]) => ({
        fileName: rootRelative(fileName),
        cruiseOptions: cruiseOptions(cruise).options,
        resolveOptions: resolveOptions(resolveOpts),
        transpileOptions: plain(transpile),
    }),
    resolve: ([module, baseDir, fileDir, resolveOpts, transpile]) => ({
        module: plain(module),
        baseDir: rootRelative(baseDir),
        fileDir: rootRelative(fileDir),
        resolveOptions: resolveOptions(resolveOpts),
        transpileOptions: plain(transpile),
    }),
    'determine-dependency-types': ([
        dependency,
        moduleName,
        manifest,
        fileDir,
        resolveOpts,
        baseDir,
        transpile,
    ]) => ({
        transpileOptions: plain(transpile),
        dependency: plain(dependency),
        moduleName,
        manifest: plain(manifest),
        fileDir: fileDir === undefined ? null : rootRelative(fileDir),
        resolveOptions: resolveOpts === undefined ? null : resolveOptions(resolveOpts),
        baseDir: baseDir === undefined ? null : rootRelative(baseDir),
    }),
    extract: ([files, cruise, resolveOpts, tsConfig]) => ({
        files: files.map(rootRelative),
        cruiseOptions: cruiseOptions(cruise).options,
        resolveOptions: resolveOptions(resolveOpts),
        tsConfig: plain(tsConfig),
    }),
    'gather-initial-sources': ([files, cruise]) => ({
        files: files.map(rootRelative),
        cruiseOptions: cruiseOptions(cruise).options,
    }),
    'extract-stats': ([fileName, options, transpile]) => ({
        fileName,
        cruiseOptions: cruiseOptions(options).options,
        transpileOptions: plain(transpile),
    }),
    'stats-acorn': ([options, fileName, transpile]) => ({
        parser: 'acorn',
        fileName,
        cruiseOptions: cruiseOptions(options).options,
        transpileOptions: plain(transpile),
    }),
    'stats-tsc': ([options, fileName, transpile]) => ({
        parser: 'tsc',
        fileName,
        cruiseOptions: cruiseOptions(options).options,
        transpileOptions: plain(transpile),
    }),
};

/** Walkers that append to a dependency array rather than returning one. */
const APPENDING = new Set(['walk-acorn-cjs', 'walk-acorn-es6', 'walk-acorn-amd']);

function isWalk(name) {
    return name.startsWith('walk-');
}

/** Replaces the checkout root with `<root>` everywhere in a recorded call. */
function portable(call) {
    return JSON.parse(JSON.stringify(call).split(root).join('<root>'));
}

export function surface(name, fn) {
    return function recordedSurface(...args) {
        const outermost = depth === 0 && calls !== null;
        // A walker called on an AST no recorded source produced (a hand-built AST) is not replayable.
        const replayable = !isWalk(name) || astSources.has(args[0]);
        const appended = APPENDING.has(name) ? args[1] : null;
        const before = appended ? appended.length : 0;
        const cwd = rootRelative(process.cwd());
        depth += 1;
        let result;
        try {
            result = fn.apply(this, args);
        } catch (error) {
            if (outermost && replayable) {
                calls.push(
                    portable({
                        surface: name,
                        cwd,
                        input: INPUTS[name](args),
                        throws: String(error?.message ?? error),
                    }),
                );
            }
            throw error;
        } finally {
            depth -= 1;
        }
        if (outermost && replayable) {
            const expected = appended ? appended.slice(before) : result;
            calls.push(
                portable({
                    surface: name,
                    cwd,
                    input: INPUTS[name](args),
                    expected: plain(expected),
                }),
            );
        }
        return result;
    };
}
