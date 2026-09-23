# ADR-0029: Ratchets are enforced by `cruise` and reported in `summary.ratchets[]`

- **Status:** Accepted
- **Date:** 2026-09-23
- **Derives from:** [design § The run and its consumers](../artifacts/design.md#the-run-and-its-consumers) (the ratchet guard "fails hard on a missing JSON and on an implausible zero"), [design § The native format](../artifacts/design.md#the-native-format) (`ratchets`, "a budget that may only fall, as config"), [ADR-0004](0004-graph-document-is-cruise-result-superset.md), [ADR-0007](0007-vacuous-rules-fail-by-default.md), [ADR-0008](0008-exit-code-contract.md)
- **Constrains:** `crates/rb-model` (`Summary`), `crates/rb-cli` (`cruise`, `fmt`), `crates/rb-report` (`--strict-schema`), `schema/v1.json`
- **Extends:** the summary additions listed in [ADR-0004](0004-graph-document-is-cruise-result-superset.md)
- **Implemented by:** [Wave 1 plan](../plans/pending/0001-wave-1-typescript-parity.md), Steps 7 and 15
- **Requirements:** [FR-RULE-06](../prd.md#fr-rule-06), [FR-CORE-04](../prd.md#fr-core-04)

## Context

`rules.ratchets` is configuration: a name, a `from` and `to` pair, and a budget file `{ "ceiling": n }`. `count` applies one ratchet by hand. The design says a hand-written ratchet script "becomes one line of config", which only holds if the gate that runs every rule also runs every ratchet. A ratchet is not a dependency rule, and dependency-cruiser's `summary.violations[].type` has no value for it, so a breach cannot be written as a violation without breaking the `cruise-result` schema. [ADR-0004](0004-graph-document-is-cruise-result-superset.md) lists the summary additions exhaustively (`inspected`, `vacuousRules[]`), so a new one needs a decision.

## Decision

- `cruise` evaluates every ratchet after the rules and writes the result to an additive `summary.ratchets[]`, one entry per ratchet in configuration order: `name`, `budget` (the file), `count` (matching direct edges), `ceiling` (absent when the budget cannot be read) and `status`, one of `held`, `exceeded` or `no-budget`.
- An `exceeded` ratchet counts as one error in the exit code, beside the error-severity violations, under the cap of [ADR-0008](0008-exit-code-contract.md). `summary.error` stays the count of error-severity violations, as dependency-cruiser defines it.
- A `no-budget` ratchet makes the run untrustworthy (exit 2), because a guard with no ceiling checks nothing. So does a ratchet whose `from` matches no module when liveness is on; it is listed in `summary.vacuousRules[]` with `side: "from"`, as [ADR-0007](0007-vacuous-rules-fail-by-default.md) does for rules.
- Each `exceeded` or `no-budget` ratchet is named on stderr with its count, its ceiling, its budget file and its `fix`.
- `fmt` reads `summary.ratchets[]` from a saved result and applies the same exit rule, so a gate that re-reports a saved cruise agrees with the cruise.
- `--strict-schema` removes `summary.ratchets`, as it removes the other additions.

## Consequences

- A repository's ratchet scripts retire into configuration, and one `cruise` is the whole gate.
- `summary`, `impact` and `count` read the same budget files and report the same numbers.
- The graph document schema gains one optional summary field; a consumer written against dependency-cruiser's schema sees nothing new unless it reads the field.

## Alternatives considered

- **Leave ratchets to `count`, one CI step per ratchet.** Rejected: that is the hand-written script the design retires, and a ratchet missing from CI would go unnoticed.
- **Write a breach as a violation with a new `type`.** Rejected: dependency-cruiser's schema enumerates the types, and layer 4 validates every result against it.
- **Add breaches to `summary.error`.** Rejected: `fmt --exit-code` on a dependency-cruiser result, and every consumer that reads `summary.error`, would see a number dependency-cruiser never produces.
