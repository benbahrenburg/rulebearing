# ADR-0053: `x-dot-webpage` draws its graph with GraphViz' `dot`, as dependency-cruiser does

- **Status:** Proposed
- **Date:** 2026-09-27
- **Derives from:** [dependency-cruiser coverage § Output types](../artifacts/dependency-cruiser-18.2.0-coverage.md#output-types) (row `x-dot-webpage`, Parity, wave 3), [design § Reporters](../artifacts/design.md#reporters), [architecture § Security posture](../architecture.md#security-posture), [ADR-0009](0009-conformance-suites-as-specification.md) (the upstream specs are the specification), [ADR-0010](0010-crate-layout-and-extractor-boundary.md) (what each crate may do)
- **Constrains:** `crates/rb-report/src/dot_webpage.rs`, `crates/rb-cli/src/graphviz.rs`, `crates/rb-cli/src/protocol.rs`, `conformance/dependency-cruiser/harness/dot-webpage-forward.mjs`
- **Implemented by:** [Wave 3 plan](../plans/pending/0003-wave-3-operations-surface-inner-loop.md), [Step 6](../plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
- **Requirements:** [FR-OUT-01](../prd.md#fr-out-01), [NFR-SEC-01](../prd.md#nfr-sec-01), [NFR-CONF-01](../prd.md#nfr-conf-01)

## Context

dependency-cruiser 18.2.0's `x-dot-webpage` reporter (`src/report/dot-webpage/dot-module.mjs`) renders the module-level `dot` program, runs `dot -V` to check that GraphViz is installed, pipes the program through `dot -Tsvg`, and wraps the SVG in an HTML page. Without GraphViz it throws "GraphViz dot, which is required for the 'x-dot-webpage' reporter doesn't seem to be available on this system". The SVG is GraphViz' layout; there is no way to produce it byte for byte without GraphViz.

The architecture's security posture says "no code execution outside the sandbox" and names Node (`--sidecar node`, `--config-via-node`) as the only spawned runtime. The binary already starts `git` (`--affected`, `diff --base`, `attest`, the cache key) and `gh` (`adopt`), which are tools, not code it evaluates. `dot` is the same kind of process: a fixed program with fixed arguments, fed a text the binary wrote.

The plan lists `x-dot-webpage` in wave 3 with the exit criterion "all twenty-one dependency-cruiser output types byte-compared".

## Decision

- `--output-type x-dot-webpage` (on `cruise` and `fmt`) runs `dot -V` and then `dot -Tsvg` from `PATH`, as upstream does, with the program on stdin, and nothing else. No other output type, and no other command, starts GraphViz.
- `rb-report` decides and does not spawn: the reporter asks a `Graphviz` for the two calls and decides from the answers exactly as upstream does (the availability check on `-V`'s status and stderr, `-Tsvg`'s error or exit code). `rb-cli` supplies the runner that starts the process.
- A missing or failing `dot` exits 2 with upstream's message, because the run could not produce its report; it is not a configuration error.
- The conformance protocol answers the two calls from the `spawnFunction` option upstream's own specs pass to the reporter, so `test/report/dot-webpage/dot-module.spec.mjs` runs unmodified; the harness records what the spec's function answers and the binary decides.

## Consequences

- `x-dot-webpage` is byte for byte upstream's on a machine with the same GraphViz, and fails as upstream fails on one without it.
- The security posture gains one named process: GraphViz' `dot`, started only for this output type. The receipt does not record it, because the report is the evidence that it ran.
- CI's layer 3 job needs GraphViz installed for the oracle comparison of `x-dot-webpage`; without it upstream's reporter throws too, and the comparison counts those as not renderable upstream.

## Alternatives considered

- **Leave `x-dot-webpage` out.** Rejected: the coverage tab promises Parity, and the wave's exit criterion counts it.
- **Embed the `dot` program in the page and render it in the browser (viz.js).** Rejected: the page would differ from upstream's, and would load a script from the network or carry a large vendored one.
- **Lay out the graph in Rust.** Rejected: GraphViz' layout is the output; a different layout is a different page.
