# ADR-0024: Test quality is gated by mutation testing, property tests and snapshots, not by coverage alone

- **Status:** Accepted
- **Date:** 2026-09-21
- **Derives from:** [ADR-0018](0018-test-coverage-threshold.md) (the 70% floor), [ADR-0009](0009-conformance-suites-as-specification.md) (the suites are the specification), [design § What Rust does not solve](../artifacts/design.md#what-rust-does-not-solve) (one maintainer, the suites as reviewer), [design § Rules an agent writes, held to the same bar](../artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar)
- **Constrains:** [architecture.md § Verification strategy](../architecture.md#verification-strategy), [CLAUDE.md](../../CLAUDE.md), `.github/workflows/ci.yml`, `mutants.toml`
- **Implemented by:** [Wave 0 plan](../plans/pending/0000-wave-0-spike.md), sub-wave 0A
- **Requirements:** [NFR-QUAL-01](../prd.md#nfr-qual-01), [NFR-QUAL-02](../prd.md#nfr-qual-02)

## Context

The 70% line-coverage floor answers "did this line run under test". It does not answer "would any test notice if this line were wrong". For a tool whose correctness claim is byte-level agreement with two upstream suites, and whose maintainer is one person reviewing largely agent-authored changes, the second question is the one that matters. A test that executes a function and asserts nothing passes the floor and protects nothing.

Three specific risks:

1. **A hashing or matching function is subtly wrong for inputs nobody wrote a fixture for.** The violation id, the anchor slug and the path normaliser are pure functions over large input spaces.
2. **An output format drifts.** The reporters, the JSON document and the command-line help are contracts other people's scripts depend on, and a diff is the only honest check.
3. **A test asserts on the wrong thing.** Coverage cannot see it; a surviving mutant can.

## Decision

Four gates, in addition to the coverage floor.

**1. Mutation testing on the contract crates.** `cargo mutants` runs in CI over `rb-model`, `rb-rules` and `xtask`: the crates that hold the graph document, the rule engine and the gates themselves. A surviving mutant fails the job. Scope and exclusions live in `.cargo/mutants.toml`; build scripts, the argument dispatch and `Display` implementations are excluded because mutating them reports noise rather than missing assertions. The extractors join when they exist, one crate at a time, each in the wave plan that lands it. This is a ratchet in spirit: the excluded list may shrink, never grow, without an ADR.

Two exclusions are of a different kind and carry their reasoning in the file. An **equivalent mutant** is one no input can distinguish, so no test can kill it; the link scanner's cursor arithmetic has two. A mutant that is detected only by hanging is excluded as well, because a timeout is a slow and flaky signal even though it is not a silent pass. Every other surviving mutant is a missing assertion, and the fix is a test.

The baseline when this was introduced: 128 mutants over the three crates, 25 surviving. Closing them took six tests and found one real weakness, a path normaliser that silently clamped a link climbing above the repository root instead of reporting it. The gate starts at zero survivors.

**2. Property tests where the input space is adversarial.** Pure functions over open input spaces carry `proptest` invariants beside their example tests: the anchor slug emits only anchor characters and is idempotent; path normalisation never leaves an interior `.` or `..`; the violation id is deterministic and fixed-width. A property test states what must be true for every input, which is what a fixture cannot do.

**3. Snapshots for anything a third party parses.** The command-line help is a committed snapshot, regenerated only with `RB_UPDATE_SNAPSHOTS=1`, because flag parity with dependency-cruiser is a promise. When the reporters land, their fixtures are byte-compared against dependency-cruiser's own `test/report` directory, which is conformance gate 1 layer 3 and needs no separate mechanism.

**4. Determinism is asserted, not assumed.** A test builds the same graph from two insertion orders, normalises both and compares the serialised bytes. Two runs on the same inputs must agree byte for byte, which is what lets a cached graph be re-reported and what makes an agent's local run and CI comparable.

Doc examples on public items are executable and run in CI (`cargo test --doc`), so the documentation cannot drift from the API it describes.

## Consequences

- The mutation job is the slowest required check. It is scoped to three small crates for that reason, with a 30-minute ceiling, and it will need re-scoping as the extractors grow: the plan that adds a crate decides whether it joins.
- `proptest` is a development dependency only, MIT or Apache-2.0, so the licence policy is unaffected.
- A failing mutant is not automatically a bug in the code. It is a missing assertion, and the fix is a test, not an exclusion.
- Regenerating a snapshot is a reviewable diff. Regenerating one to make a check pass, without explaining what changed, is the failure mode this gate exists to expose.

## Alternatives considered

- **Raising the coverage floor to 90%.** Rejected: it buys exercised lines rather than asserted behaviour, and it pushes people toward tests written for the metric.
- **Branch coverage.** Rejected for now: mutation testing subsumes most of its value on these crates, and [ADR-0018](0018-test-coverage-threshold.md) fixes lines as the measure.
- **Mutation testing across the whole workspace.** Rejected: minutes per pull request for crates that are thin wrappers around other people's parsers.
- **`insta` for snapshots.** Not needed yet. One committed text file and an update flag cover the current surface; revisit when the reporters arrive.
