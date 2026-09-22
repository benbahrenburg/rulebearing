// The #validate and #graph-utl replacement for conformance gate 1 layer 2: each export of the
// original module is forwarded to `rulebearing validate`, so dependency-cruiser's own specs run
// unmodified against Rulebearing's engine.
//
// Plan: docs/plans/pending/0000-wave-0-spike.md, Step 5 item 3. Design: docs/artifacts/design.md
// § Conformance gate 1 (layer 2). Loaded through layer2-hooks.mjs by run-layer-2.mjs.
//
// Protocol (what wave 1's `validate` subcommand implements): the binary is started as
//   rulebearing validate --rules - --module - --no-liveness
// with one JSON request on stdin:
//   { "module": "#validate/index.mjs", "export": "validateDependency", "path": [], "calls": [[...args]] }
// `path` names a function inside an object export (`matchModuleRule.match` is export "default",
// path ["match"]); `calls` holds one argument list per application, so a curried call
// `match(module)(rule)` sends two. A method of a class export adds the constructor arguments:
//   { "module": "...", "export": "default", "constructorArgs": [...], "path": ["findTransitiveDependencies"], "calls": [[...]] }
// The binary answers on stdout with { "result": <value> } and exit code 0. Any other exit code is
// a failed call and its stderr is the error message. Liveness is off because the upstream specs
// do not expect it (ADR-0007). Every call is stateless: a class instance is replayed from its
// constructor arguments.
//
// The original module is consulted for shape only. When one application of an original function
// returns a function, the forwarder keeps collecting argument lists instead of calling out; the
// value a spec asserts on always comes from the binary.
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const binaryName = process.platform === 'win32' ? 'rulebearing.exe' : 'rulebearing';

/** The binary under test: $RULEBEARING_BIN, else the release build in this repository. */
export function binary() {
    const configured = process.env.RULEBEARING_BIN;
    if (configured) {
        return configured;
    }
    const release = join(repository, 'target', 'release', binaryName);
    return existsSync(release) ? release : join(repository, 'target', 'debug', binaryName);
}

/** Replaces regular expressions with their source so a request is plain JSON. */
function toJson(value) {
    return JSON.stringify(value, (_key, inner) => (inner instanceof RegExp ? inner.source : inner));
}

/** Sends one request to the binary and returns its result, or throws with its message. */
export function call(request) {
    const run = spawnSync(
        binary(),
        ['validate', '--rules', '-', '--module', '-', '--no-liveness'],
        { input: toJson(request), encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 },
    );
    if (run.error) {
        throw new Error(`rulebearing validate could not start: ${run.error.message}`);
    }
    if (run.status !== 0) {
        throw new Error(`rulebearing validate exited ${run.status}: ${run.stderr.trim()}`);
    }
    return JSON.parse(run.stdout).result;
}

function isClass(value) {
    return (
        typeof value === 'function' && /^class[\s{]/u.test(Function.prototype.toString.call(value))
    );
}

/** A forwarded function: follows a curried chain, then calls the binary with every stage. */
function forwardFunction(request, original, calls = []) {
    return (...args) => {
        const chain = [...calls, args];
        const shape = original(...args);
        if (typeof shape === 'function') {
            return forwardFunction(request, shape, chain);
        }
        return call({ ...request, calls: chain });
    };
}

/** Forwards every function reachable from `value`, keeping data as data. */
function forwardValue(request, value) {
    if (isClass(value)) {
        // A constructor function rather than a class: `new` on it returns the proxy.
        return function Forwarded(...constructorArgs) {
            const instance = new value(...constructorArgs);
            return new Proxy(instance, {
                get(_target, key) {
                    const member = instance[key];
                    if (typeof key !== 'string' || typeof member !== 'function') {
                        return member;
                    }
                    return forwardFunction(
                        { ...request, constructorArgs, path: [...request.path, key] },
                        member.bind(instance),
                    );
                },
            });
        };
    }
    if (typeof value === 'function') {
        return forwardFunction(request, value);
    }
    if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
        return Object.fromEntries(
            Object.entries(value).map(([key, inner]) => [
                key,
                forwardValue({ ...request, path: [...request.path, key] }, inner),
            ]),
        );
    }
    // A constant is data, not behaviour; the spec reads it as it is.
    return value;
}

/** The forwarding replacement for export `name` of `module`. */
export function forward(module, name, original) {
    return forwardValue({ module, export: name, path: [] }, original);
}
