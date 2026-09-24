# ADR-0037: `baseline` has three modes of Rulebearing's own; dependency-cruiser has none

- **Status:** Proposed
- **Date:** 2026-09-24
- **Derives from:** [ADR-0015](0015-stable-violation-id.md) (the stable violation id), [ADR-0031](0031-a-saved-result-carries-what-the-exit-code-counts.md) (`summary.expired[]`), [ADR-0009](0009-conformance-suites-as-specification.md)
- **Constrains:** `crates/rb-cli/src/cmd/baseline.rs`, `crates/rb-rules/src/known.rs`, `crates/rb-report/src/baseline.rs`
- **Implemented by:** [Wave 2 plan](../plans/pending/0002-wave-2-dotnet-python-element-rules.md), [Step 10](../plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
- **Requirements:** [FR-RULE-09](../prd.md#fr-rule-09)

## Context

Plan 0002 Step 10 asks for `rulebearing baseline [--baseline-mode full | shrink-only | format]` "with dependency-cruiser's three modes". dependency-cruiser 18.2.0 has no modes: `depcruise-baseline` runs `depcruise -c -T baseline -f .dependency-cruiser-known-violations.json` and nothing else. The coverage tab's row and FR-RULE-09 describe a behaviour upstream does not have, so the modes need a definition of their own. The one upstream behaviour to keep is the `baseline` reporter's output, which gate 1 layer 3 byte-compares against `test/report/baseline`.

## Decision

- **`full`** (the default) writes every current violation as a `knownViolations` entry keyed by its stable id. An entry already in the file keeps its `expires`, `owner` and `reason`. With no lifecycle fields, the output is byte for byte dependency-cruiser's.
- **`shrink-only`** reads the file (or, without one, `options.knownViolations`), writes it back less the entries no current violation matches, prints each entry it removed, and never adds one. It exits with the number of removed entries, so a fixed finding fails CI until its entry leaves the baseline: import-linter's unmatched-ignore alerting ([design § import-linter contracts](../artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them)). New findings are warned about, not added.
- **`format`** rewrites the file sorted by rule, `from`, `to` and `id`, without cruising, and exits 0.
- `--expires`, `--owner` and `--reason` fill those fields on each written entry that lacks them. An entry past its `expires` stops applying the day after and fails the run through `summary.expired[]`.
- `--ignore-known [file]` replaces `options.knownViolations`, as dependency-cruiser does. `--no-ignore-known` applies none, the configuration's included; the last of the two wins.

## Consequences

- The coverage tab row for the baseline is Parity+: the reporter and `--ignore-known` match upstream, the modes and the lifecycle fields are additions.
- `rulebearing import import-linter` points `ignore_imports` users at `baseline --baseline-mode shrink-only`.

## Alternatives considered

- **No modes, as upstream.** Rejected: the plan's shrink-only behaviour is the import-linter alerting the design promises, and `format` keeps a hand-edited baseline reviewable.
- **`shrink-only` exiting 1 on any removal.** Rejected: the count names how many entries went, as the error count does for violations.
