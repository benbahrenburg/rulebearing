# ADR-0059: Gates run by tier; a pull request mutates the lines it changes, a push to `main` mutates the whole scope

- **Status:** Proposed
- **Date:** 2026-10-01
- **Derives from:** [ADR-0024](0024-test-quality-gates.md) (mutation testing on the contract crates), [ADR-0009](0009-conformance-suites-as-specification.md) (the conformance gates), [ADR-0018](0018-test-coverage-threshold.md) (the coverage floor), [ADR-0058](0058-the-edit-compile-cycle-rebuilds-only-what-changed.md) (what one edit rebuilds), [architecture § Verification strategy](../architecture.md#verification-strategy)
- **Supersedes:** nothing. It refines when [ADR-0024](0024-test-quality-gates.md)'s mutation gate mutates which lines; its scope (`rb-model`, `rb-rules`, `xtask`), its exclusion policy and its rule that a survivor is fixed with a test stand.
- **Constrains:** `.github/workflows/ci.yml` (`mutants-shard`), `scripts/mutants-diff.sh` (new), [CLAUDE.md](../../CLAUDE.md) § Testing and coverage
- **Implemented by:** the change that adds this record
- **Requirements:** [NFR-QUAL-01](../prd.md#nfr-qual-01), [NFR-QUAL-02](../prd.md#nfr-qual-02)

## Context

A wave is delivered as steps, each on its own branch, often several in parallel worktrees, each reviewed and fixed before it merges. Every gate in [CLAUDE.md](../../CLAUDE.md) is run on every step, and again after every review fix: the workspace suite, both conformance gates, coverage per crate, and mutation testing over the whole of `rb-model`, `rb-rules` and `xtask`.

Plan 0003's status line for sub-wave 3D, 2026-10-01, records what that costs on one machine. A mutation run over 185 mutants was stopped unfinished at 69. The `rb-cli` suite, started alongside it, hit its 25-minute limit without reporting. The guard's latency was measured at a load average of 8 to 10 and had to be marked as awaiting an unloaded run. The gates are not wrong; they ran at the wrong grain and on top of each other.

The mutants that bear on a change are the ones on lines it changes. `cargo mutants --in-diff` mutates exactly those. The last commit on that branch (`8e43549`, `rb-rules`) yields 8 mutants in-diff against 185 for the whole scope. What `--in-diff` cannot see is a change that weakens a test of unchanged code, for example a deleted assertion, which leaves a mutant of an unchanged line surviving.

## Decision

**1. Mutation testing by diff on a pull request, whole scope on `main`.** The `mutants-shard` job runs `scripts/mutants-diff.sh origin/<base>` on a pull request: the same three packages and exclusions, mutating only the lines changed since the merge base, in the same eight shards. A push to `main` runs the whole scope as before. A pull request that changes `.cargo/mutants.toml` runs the whole scope, because it changes what the gate covers. A survivor in either run fails the job, and the fix is a test ([ADR-0024](0024-test-quality-gates.md)).

**2. Three tiers say which gates a piece of work runs, and when.**

| Tier | When | Gates |
| --- | --- | --- |
| Inner loop | after each edit | `cargo check -p <crate>`, `cargo test -p <crate>`, `cargo clippy -p <crate> --all-targets -- -D warnings` |
| Step merge | once per step branch, after its review fixes | `cargo lint`, `cargo test --workspace --all-features`, `scripts/mutants-diff.sh <wave branch>`, gate 1 layers 1 to 4 and gate 2 when the step touches an extractor, a reporter or the engine |
| Sub-wave close | once, before the sub-wave is marked `Done` | everything in [CLAUDE.md](../../CLAUDE.md)'s definition of done: coverage per crate, both conformance gates in full with the ratchets, layer 5, the oracles, `cargo mutants` over the whole scope, `cargo deny`, and any timing the plan names |

A review fix re-runs the inner loop, and the step-merge tier once at the end, not after each finding.

**3. A timing is never measured while another gate runs.** A plan's performance target is measured on a machine running nothing else of this repository's, and a mutation run is never started beside the workspace suite. Both share the machine's cores, and a timing taken under load is not evidence.

## Consequences

- A pull request's mutation job costs in proportion to the change, not to the three crates.
- A pull request that weakens a test of unchanged code can pass its mutation job and fail `main`'s. `main` then goes red on the merge, and the fix is a test in the next change. This is the price of the per-diff run, and the whole-scope run on every push to `main` bounds it to one merge.
- The `mutants-shard` checkout fetches the full history, to find the merge base.
- The tiers are a working rule, not a CI change: CI runs every required check on every pull request, as before. They decide what an agent or a developer runs locally before it gets there.
- A step that runs only the inner loop and the step-merge tier is not `Done`. The status vocabulary in [docs/plans/README.md](../plans/README.md) is unchanged.

## Alternatives considered

- **Whole-scope mutation testing on every pull request, as now.** Rejected: its cost grows with the crates rather than with the change, and it is already sharded eight ways.
- **Mutation testing only nightly.** Rejected: a survivor would reach `main` from any pull request, not only from one that weakens a test, and the author would have moved on.
- **A whole-scope run on a pull request whenever it touches a test file.** Rejected for now: most changes touch a test file, so it would rarely differ from the whole-scope run. Revisit if `main`'s whole-scope run goes red more than occasionally.
