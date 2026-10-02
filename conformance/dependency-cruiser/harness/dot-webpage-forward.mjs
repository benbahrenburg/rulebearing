// The replacement for `#report/dot-webpage/dot-module.mjs` in conformance gate 1 layer 3: the
// reporter is `rulebearing report --output-type x-dot-webpage`.
//
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 6. Decision:
// docs/adr/0053-x-dot-webpage-draws-with-graphviz-dot.md.
//
// Upstream's reporter takes a `spawnFunction` option, which its spec uses to stand in for
// GraphViz' `dot`. A function cannot cross the protocol, so this module asks it the two calls the
// reporter makes (`dot -V`, then `dot -Tsvg` with the module-level dot program on stdin) and
// sends the answers as `spawnFunction: { version, convert }`. The binary decides from them, as
// upstream's reporter decides from what `spawnSync` returns. Without the option, the binary runs
// the `dot` on PATH.
import { report } from './shim.mjs';

function answer(spawned) {
    const text = (value) => (value === undefined || value === null ? null : String(value));
    return {
        status: spawned?.status ?? null,
        stdout: text(spawned?.stdout),
        stderr: text(spawned?.stderr),
        error: spawned?.error ? String(spawned.error.message ?? spawned.error) : null,
    };
}

export default function dotWebpage(result, options) {
    const spawn = options?.spawnFunction;
    if (typeof spawn !== 'function') {
        return report('x-dot-webpage', result, options);
    }
    const rest = Object.fromEntries(
        Object.entries(options).filter(([key]) => key !== 'spawnFunction'),
    );
    const program = report('dot', result, rest).output;
    return report('x-dot-webpage', result, {
        ...rest,
        spawnFunction: {
            version: answer(spawn('dot', ['-V'])),
            convert: answer(spawn('dot', ['-Tsvg'], { input: program })),
        },
    });
}
