# ADR-0031: A saved result carries everything the exit code counts

- **Status:** Accepted
- **Date:** 2026-09-23
- **Derives from:** [ADR-0030](0030-the-reporter-decides-the-error-count-exit.md) (`fmt --exit-code` applies the same rule as `cruise`), [design § Three pipelines](../artifacts/design.md#three-pipelines), the wave 1 review
- **Constrains:** `crates/rb-model` (`summary.expired[]`), `crates/rb-rules` (fills it), `crates/rb-report` (`--strict-schema` strips it), `crates/rb-cli` (`fmt --exit-code`)
- **Implemented by:** [Wave 1 plan](../plans/pending/0001-wave-1-typescript-parity.md), Step 13
- **Requirements:** [FR-CORE-06](../prd.md#fr-core-06)

## Context

[ADR-0030](0030-the-reporter-decides-the-error-count-exit.md) made `cruise --output-type json` exit 0 on a trustworthy run and moved the gate to `fmt --exit-code --output-type err` over the saved file. That is sound only if the saved file holds everything `cruise` counts. Two things were missing:

- **Expired entries.** A rule or known-violation entry past its `expires` date is one error ([docs/rules.md](../rules.md#baselines-and-ratchets)). The engine kept the list in memory and `cruise` printed it to stderr, but it never reached the document. `cruise -T json` then `fmt --exit-code -T err` passed a run that `cruise -T err` failed.
- **Vacuous rules.** They were in `summary.vacuousRules`, but `fmt` did not read them, so a saved run that `cruise` exited 2 on was re-reported with the error count.

## Decision

- `summary.expired[]` is an additive field: one entry per expired rule or known violation, with `name`, `expires` (`YYYY-MM-DD`) and `kind` (`rule` or `knownViolation`). It is absent when nothing has expired, and `--strict-schema` removes it with the other additions ([ADR-0004](0004-graph-document-is-cruise-result-superset.md)).
- `fmt --exit-code` computes the code from the saved result alone. A non-empty `summary.vacuousRules` or a ratchet with `no-budget` gives 2. Otherwise a gating reporter's code is `summary.error` plus the expired entries plus the exceeded ratchets, the same sum `cruise` uses.

## Consequences

- The design's two-step pipeline fails exactly when a one-step `cruise -T err` would.
- A result written by dependency-cruiser has no `expired` or `vacuousRules`, so `fmt` treats it as before.

## Alternatives considered

- **Count expired entries into `summary.error`.** Rejected: `summary.error` is dependency-cruiser's count of error-severity violations, and changing what it means breaks the "nothing is dropped" promise for anyone who reads the field.
- **Leave the two-step pipeline blind to expiry.** Rejected: an entry whose date passes would stop failing CI as soon as a repository adopts the documented pipeline.
