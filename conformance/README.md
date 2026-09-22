# Conformance

The upstream tools' test suites are Rulebearing's specification ([ADR-0009](../docs/adr/0009-conformance-suites-as-specification.md)). Both gates are required checks from the first pull request, and both ratchet: they may only tighten. The two coverage tabs in [docs/artifacts](../docs/artifacts/) are the ledger; a row is not marked **Parity** until the pinned suite says so.

| Gate | Upstream | Pinned | What runs today | CI job |
| --- | --- | --- | --- | --- |
| 1 | [dependency-cruiser](https://github.com/sverweij/dependency-cruiser) (MIT) | [18.2.0](dependency-cruiser/PIN) | Layer 1: the recorded `test/extract` cases replayed against the Rust extractor. Layer 2: upstream's `test/validate` and `test/graph-utl` specs, unmodified, with the unit under test forwarded to `rulebearing validate` | `conformance-gate-1` |
| 2 | [ArchUnitNET](https://github.com/TNG/ArchUnitNET) (Apache-2.0) | [0.13.4](archunitnet/PIN) | The committed `TestAssembly` fixture verified (hashes, portable PDB, notice) and `ported.json` validated; the reader's end-to-end test over it lives in `rb-extract-dotnet` | `conformance-gate-2` |

The ratchets are checked by `scripts/ratchets.sh` in the `ratchets` job against the base branch:

| Ratchet | File | Direction |
| --- | --- | --- |
| Layer 2 exclusions | [excluded.json](excluded.json) | may only shrink |
| Layer 1 threshold | [dependency-cruiser/threshold.json](dependency-cruiser/threshold.json) | may only rise (0.95 from sub-wave 0C, 1.0 from wave 1) |
| Ported ArchUnitNET tests | [archunitnet/ported.json](archunitnet/ported.json) | `ported` may only rise |

A ratchet compares against a base recorded at the same upstream `pin`. A pin bump re-vendors and re-records, and the pull request that bumps it must show the new figures.

Liveness ([ADR-0007](../docs/adr/0007-vacuous-rules-fail-by-default.md)) is disabled with `--no-liveness` when running dependency-cruiser's specs, because those specs do not expect it. Every other default is the tool's own.

## Gate 1, layer by layer

**Layer 1, extraction.** [`harness/export-expectations.mjs`](dependency-cruiser/harness/export-expectations.mjs) runs upstream's `test/extract` suite unmodified under a module loader hook that records each call to an extraction surface: the walkers for acorn, tsc and swc, `extractDependencies`, `resolve`, `determineDependencyTypes`, `extract`, `gatherInitialSources` and the statistics functions. It records the input and the value the passing test received. That value is by definition what 18.2.0 expects, so the recording is the specification. The Rust test `crates/rb-extract-ts/tests/extract_fixtures.rs` replays every case, prints `layer1: passed=<n> total=<t> ratio=<r>` and a timing line, writes the classified diff to `target/conformance/layer1.md`, and fails below the threshold.

`fixtures/extract/INDEX.json` accounts for all 480 upstream tests. 295 call a surface and give the 296 recorded cases. The other 185 are listed under `notLayer1` with the reason, by spec:

- they assert a transpiler's output (Babel, TypeScript, Svelte, Vue, CoffeeScript, LiveScript), which is generated JavaScript rather than a graph;
- they assert the shape of acorn's AST;
- they feed a hand-built AST object;
- or they test an internal helper whose effect reaches the cruise result only through the recorded surfaces.

This split is what makes the denominator auditable.

**Layer 2, rules.** [`harness/run-layer-2.mjs`](dependency-cruiser/harness/run-layer-2.mjs) runs the 34 specs with [`layer2-hooks.mjs`](dependency-cruiser/harness/layer2-hooks.mjs) replacing each `#validate` and `#graph-utl` import made by a spec with [`shim.mjs`](dependency-cruiser/harness/shim.mjs). Every function, method and curried application is forwarded to the binary over the JSON protocol documented at the top of `shim.mjs`; the original module is consulted only for shape. Until `validate` exists (wave 1) every spec fails and is listed in `excluded.json` with reason `wave-1`. In gate mode, a failure in a spec that is not listed fails the job, and a listed spec that passes is reported so the list can shrink. `--record` rewrites the list.

**Layers 3 to 5** (report fixtures byte-compared, schema validation, oracle zero-diff and the mutation branch) arrive with the reporters in [wave 1](../docs/plans/pending/0001-wave-1-typescript-parity.md). The two 18.2.0 schemas layer 4 needs are already vendored under `dependency-cruiser/fixtures/schemas/`.

## Running it locally

```sh
conformance/dependency-cruiser/scripts/vendor.sh          # once, or after a PIN bump: fixtures and expectations
conformance/dependency-cruiser/run.sh                     # layers 1 and 2 (clones upstream into dependency-cruiser/upstream/)
cargo test -p rb-extract-ts --test extract_fixtures -- --nocapture   # layer 1 alone
RB_UPDATE_LAYER1_OPEN=1 cargo test -p rb-extract-ts --test extract_fixtures   # rewrite layer1-open.json
node conformance/dependency-cruiser/harness/run-layer-2.mjs <checkout> --record   # shrink excluded.json
conformance/archunitnet/scripts/build-test-assembly.sh    # once; rebuild only on a PIN bump
scripts/gate2-check.sh && scripts/ratchets.sh
```

## Layout

```
conformance/
├── excluded.json                    # layer 2 exclusions, reason and plan each; may only shrink
├── dependency-cruiser/
│   ├── PIN, LICENSE                 # 18.2.0; upstream's MIT licence
│   ├── threshold.json               # layer 1 minimum ratio; may only rise
│   ├── run.sh                       # the conformance-gate-1 job
│   ├── scripts/vendor.sh            # fetches the tag, vendors inputs, records expectations
│   ├── harness/                     # the recorder, the layer 2 shim and runner (Node)
│   └── fixtures/
│       ├── extract/                 # test/extract inputs, INDEX.json, expectations/*.json
│       ├── report/, report-json/    # test/report verbatim; its cruise results as JSON (rb-model round trip)
│       └── schemas/                 # the 18.2.0 cruise-result and configuration schemas
└── archunitnet/
    ├── PIN, ported.json             # 0.13.4; the port counter
    ├── scripts/build-test-assembly.sh
    └── fixtures/                    # TestAssembly.dll and .pdb, SHA256SUMS, LICENSE, NOTICE, README.md
```
