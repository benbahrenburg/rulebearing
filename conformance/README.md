# Conformance

The upstream tools' test suites are Rulebearing's specification ([ADR-0009](../docs/adr/0009-conformance-suites-as-specification.md)). Both gates are required checks from the first pull request, and both ratchet: they may only tighten. The two coverage tabs in [docs/artifacts](../docs/artifacts/) are the ledger; a row is not marked **Parity** until the pinned suite says so.

| Gate | Upstream | Pinned | What runs today | CI job |
| --- | --- | --- | --- | --- |
| 1 | [dependency-cruiser](https://github.com/sverweij/dependency-cruiser) (MIT) | [18.2.0](dependency-cruiser/PIN) | Layer 1: 296 of 296 recorded `test/extract` cases. Layer 2: 34 of 34 `test/validate` and `test/graph-utl` specs, unmodified. Layer 3: the wave 1 and wave 2 `test/report` specs, byte for byte, and an oracle comparison of every wave 2 reporter with upstream's. Layer 4: output and configurations against the upstream schemas. Layer 5: zero difference on three oracle repositories and the mutation branch | `conformance-gate-1`, `conformance-gate-1-layer-5` |
| 2 | [ArchUnitNET](https://github.com/TNG/ArchUnitNET) (Apache-2.0) | [0.13.4](archunitnet/PIN) | The committed fixtures verified (hashes, portable PDBs, notice); every case under `archunitnet/ported/` reproduces upstream over the committed graphs (`cargo test -p rb-rules --test gate2`, 1567 of 1605 upstream cases; the rest are listed in `archunitnet/unported.json` with reasons), and the graphs are current with the fixtures | `conformance-gate-2`, `gate2-ratchet` |
| 2 | [NetArchTest](https://github.com/BenMorris/NetArchTest) (MIT) | [1.3.2](netarchtest/PIN) | Its own unit tests: 326 cases, each NetArchTest's verdict over the committed `NetArchTest.TestStructure` fixtures, reproduced by the element engine (`crates/rb-rules/tests/gate2_netarchtest.rs`); fixtures and counts verified by `scripts/gate2-netarchtest-check.sh` ([netarchtest/README.md](netarchtest/README.md)) | `conformance-gate-2` |

The ratchets are checked by `scripts/ratchets.sh` in the `ratchets` job against the base branch:

| Ratchet | File | Direction |
| --- | --- | --- |
| Layer 2 exclusions | [excluded.json](excluded.json) | may only shrink |
| Layer 1 threshold | [dependency-cruiser/threshold.json](dependency-cruiser/threshold.json) | may only rise (0.95 from sub-wave 0C, 1.0 from wave 1) |
| Ported ArchUnitNET tests | [archunitnet/ported.json](archunitnet/ported.json) | `ported` may only rise |
| Unported ArchUnitNET tests | [archunitnet/unported.json](archunitnet/unported.json) | may only shrink, and holds no `not-yet` entry once plan 0002 is implemented (`scripts/gate2-ratchet.sh`, job `gate2-ratchet`) |
| Ported NetArchTest tests | [netarchtest/ported.json](netarchtest/ported.json) | `ported` may only rise |
| Unported NetArchTest tests | [netarchtest/unported.json](netarchtest/unported.json) | may only shrink, and holds no `not-yet` entry once plan 0002 is implemented (`scripts/gate2-ratchet.sh`) |

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

**Layer 2, rules.** [`harness/run-layer-2.mjs`](dependency-cruiser/harness/run-layer-2.mjs) runs the 34 specs with [`layer2-hooks.mjs`](dependency-cruiser/harness/layer2-hooks.mjs) replacing each `#validate` and `#graph-utl` import made by a spec with [`shim.mjs`](dependency-cruiser/harness/shim.mjs). Every function, method and curried application is forwarded to the binary over the JSON protocol documented at the top of `shim.mjs`; the original module is consulted only for shape. All 34 specs pass, and `excluded.json` lists none. In gate mode, a failure in a spec that is not listed fails the job, and a listed spec that passes is reported so the list can shrink. `--record` rewrites the list.

**Layer 3, reporters.** [`harness/run-layer-3.mjs`](dependency-cruiser/harness/run-layer-3.mjs) runs upstream's `test/report` specs unmodified for the wave 1 reporters (`err`, `err-long`, `text`, `csv`, `teamcity`, `azure-devops`, `null`) and the wave 2 ones (`dot`, `ddot`, `archi` / `cdot`, `flat` / `fdot`, `mermaid`, `d2`, `metrics`, `err-html`), with [`layer3-hooks.mjs`](dependency-cruiser/harness/layer3-hooks.mjs) forwarding each reporter to `rulebearing report` and each reporter-internal module a unit spec imports (`dot/theming`, `dot/module-utl`, `error-html/utl`) to `rulebearing validate`. The specs compare output byte for byte, so a pass is a byte-compare pass. Because the `err-html` and `metrics` specs match with regular expressions, the runner then renders every cruise result mock under `test/report` with each wave 2 reporter and several option sets through both upstream's implementation and Rulebearing's, and fails on any byte of difference (the `err-html` run date aside) ([plan 0002, Step 10](../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)).

**Layer 4, schemas.** [`crates/rb-cli/tests/layer4.rs`](../crates/rb-cli/tests/layer4.rs), run alone by [`scripts/run-layer-4.sh`](dependency-cruiser/scripts/run-layer-4.sh), validates against the vendored 18.2.0 schemas in `dependency-cruiser/fixtures/schemas/`. It checks every upstream result the pinned schema accepts, re-reported with `fmt --strict-schema`; a fresh `cruise --strict-schema`; layer 5's results when present; and every bundled preset and fetched oracle configuration at the stage dependency-cruiser validates it (`extends` merged, rules not yet normalised).

**Layer 5, oracle zero-diff and the mutation branch** ([plan 0001, Step 18](../docs/plans/pending/0001-wave-1-typescript-parity.md#step-18-gate-1-layer-5-the-mutation-branch-oracle-zero-diff-1g)). [`scripts/run-layer-5.sh`](dependency-cruiser/scripts/run-layer-5.sh) clones `sverweij/dependency-cruiser`, `langfuse/langfuse` and `microsoft/FluidFramework` at their [manifest](../testbeds/manifest.yaml) SHAs, runs dependency-cruiser and `rulebearing cruise` with each repository's own configuration and roots (the script records which, and why), and [`harness/zero-diff.mjs`](dependency-cruiser/harness/zero-diff.mjs) compares every module, every dependency field and every violation after sorting. A difference fails the job unless [divergences.md](divergences.md) records it with a reason. `--mutations` applies [the mutation branch](dependency-cruiser/mutations/README.md) to dependency-cruiser's repository and requires both tools to report exactly its twelve violations, one per rule shape. The job, `conformance-gate-1-layer-5`, runs on every push to `main` and nightly, not on pull requests, because it clones and installs three repositories.

## Running it locally

```sh
conformance/dependency-cruiser/scripts/vendor.sh          # once, or after a PIN bump: fixtures and expectations
conformance/dependency-cruiser/run.sh                     # layers 1 to 4 (clones upstream into dependency-cruiser/upstream/)
conformance/dependency-cruiser/scripts/run-layer-4.sh     # layer 4 alone
cargo test -p rb-extract-ts --test extract_fixtures -- --nocapture   # layer 1 alone
RB_UPDATE_LAYER1_OPEN=1 cargo test -p rb-extract-ts --test extract_fixtures   # rewrite layer1-open.json
node conformance/dependency-cruiser/harness/run-layer-2.mjs <checkout> --record   # shrink excluded.json
conformance/dependency-cruiser/scripts/run-layer-5.sh --all        # layer 5: the three oracles
conformance/dependency-cruiser/scripts/run-layer-5.sh --mutations  # layer 5: the mutation branch
conformance/archunitnet/scripts/build-test-assembly.sh    # once; rebuild only on a PIN bump
conformance/netarchtest/scripts/build-test-assemblies.sh  # once; rebuild only on a PIN bump
scripts/gate2-check.sh && scripts/ratchets.sh && scripts/gate2-ratchet.sh
cargo test -p rb-rules --test gate2 -- --nocapture       # gate 2: every ported case
RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-extract-dotnet --test gate2_graphs   # regenerate the graphs
python3 conformance/archunitnet/tools/port.py            # regenerate ported/ and the counts
```

## Layout

```
conformance/
├── excluded.json                    # layer 2 exclusions, reason and plan each; may only shrink
├── divergences.md                   # layer 5 differences accepted, each with its reason
├── dependency-cruiser/
│   ├── PIN, LICENSE                 # 18.2.0; upstream's MIT licence
│   ├── threshold.json               # layer 1 minimum ratio; may only rise
│   ├── run.sh                       # the conformance-gate-1 job
│   ├── scripts/vendor.sh            # fetches the tag, vendors inputs, records expectations
│   ├── scripts/run-layer-4.sh       # the schema checks, crates/rb-cli/tests/layer4.rs
│   ├── scripts/run-layer-5.sh       # the oracle zero-diff and the mutation branch
│   ├── mutations/                   # the mutation branch as a patch, and its expected violations
│   ├── harness/                     # the recorder, the layer 2 and 3 shims and runners, zero-diff (Node)
│   └── fixtures/
│       ├── extract/                 # test/extract inputs, INDEX.json, expectations/*.json
│       ├── report/, report-json/    # test/report verbatim; its cruise results as JSON (rb-model round trip)
│       └── schemas/                 # the 18.2.0 cruise-result and configuration schemas
├── archunitnet/
│   ├── PIN, ported.json             # 0.13.4; the port counter
│   ├── scripts/build-test-assembly.sh
│   └── fixtures/                    # TestAssembly.dll and .pdb, SHA256SUMS, LICENSE, NOTICE, README.md
└── netarchtest/
    ├── PIN, ported.json, unported.json  # 1.3.2; the port counter; every test with no case, and why
    ├── scripts/build-test-assemblies.sh
    ├── tools/Port/                  # runs each upstream chain with NetArchTest and writes ported/
    ├── ported/, graphs/             # the cases; the fixtures' graphs
    └── fixtures/                    # NetArchTest.TestStructure and CrossAssemblyTest.{A,B}, SHA256SUMS, LICENSE, README.md
```
