# ADR-0018: 70% line coverage is a required check on every crate and wrapper

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** the user's project brief (70% test coverage); [design.md § What Rust does not solve](../artifacts/design.md#what-rust-does-not-solve) (the suites as reviewer)
- **Constrains:** [architecture.md § Verification strategy](../architecture.md#verification-strategy), [CLAUDE.md § Testing and coverage](../../CLAUDE.md#testing-and-coverage), `.github/workflows/ci.yml`
- **Implemented by:** [Wave 0 plan](../plans/pending/0000-wave-0-spike.md)

## Context

The conformance suites ([ADR-0009](0009-conformance-suites-as-specification.md)) prove parity with the upstream tools but do not cover the native format, the new reporters, the agent commands or the wrappers. A single-maintainer project needs a floor that a contributor's pull request cannot lower.

## Decision

- **Rust:** `cargo llvm-cov --workspace --fail-under-lines 70` is a required CI check. Coverage is measured per workspace and reported per crate; a crate below 70% fails the check even when the workspace is above it. Conformance fixtures count toward coverage when they run under `cargo test`, and they do (gate 1 layer 1 and gate 2 are `#[test]`s over committed fixtures).
- **TypeScript** (`wrappers/npm`, `adapters/vitest`, `eslint-plugin-rulebearing`): vitest with `coverage.thresholds.lines = 70`.
- **C#** (`Rulebearing.TestAdapter`, `Rulebearing.Analyzer`, the fallback extractor if invoked): coverlet with a 70% line threshold in the test project.
- **Python** (`wrappers/pip`, `pytest-rulebearing`): `pytest --cov --cov-fail-under=70`.
- The threshold is a floor, not a target: the extractors and `rb-rules` are expected to sit well above it because the fixtures drive them.
- Generated code (stdlib snapshots, schema output) is excluded by path in the coverage configuration and the exclusions are listed in `CLAUDE.md`.

## Consequences

- A pull request that adds a reporter without a fixture fails CI.
- The wave 0 plan installs `cargo-llvm-cov` in CI and sets the gate before any feature code lands.

## Alternatives considered

- **A higher threshold.** Not chosen by the brief; the conformance suites already push the core crates higher.
- **Branch coverage.** Not required; line coverage is what the brief asks for and what all four toolchains report consistently.
