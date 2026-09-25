# ADR-0017: CoffeeScript and LiveScript run through a Node sidecar

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § What each extractor has to get right](../artifacts/design.md#what-each-extractor-has-to-get-right), [§ Open questions](../artifacts/design.md#open-questions) (the sidecar); [dependency-cruiser coverage § Extraction and resolution](../artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution)
- **Constrains:** [architecture.md § Extractors](../architecture.md#extractors)
- **Implemented by:** [Wave 3 plan](../plans/pending/0003-wave-3-operations-surface-inner-loop.md)

## Context

dependency-cruiser supports CoffeeScript and LiveScript through their transpilers. No Rust parser exists for either, none of the oracle repositories uses them, and nothing may be dropped from the superset.

## Decision

- `--sidecar node` spawns dependency-cruiser for `.coffee`, `.litcoffee`, `.ls`, `.cjsx` and `.csx` files and merges the resulting edges into the graph document, marked `sidecar: true`.
- Without the flag, such a file makes the run untrustworthy (exit 2, [ADR-0008](0008-exit-code-contract.md)) with a named reason, never a silent skip.
- This is the only place the tool spawns Node, and the coverage tab says so. The specs that exercise it sit in `conformance/excluded.json` until the sidecar lands in wave 3, after which that list must be empty.

## Consequences

- The superset claim holds without a native parser for two languages with no active oracle.
- The sidecar is exercised only by dependency-cruiser's own fixtures, which is documented.

## Alternatives considered

- **Drop the two languages.** Rejected: "nothing is dropped" is the promise.
- **Port the transpilers.** Rejected: cost with no user.
