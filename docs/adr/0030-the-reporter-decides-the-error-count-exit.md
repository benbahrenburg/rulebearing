# ADR-0030: The reporter decides whether the error count is the exit code, as in dependency-cruiser

- **Status:** Accepted
- **Date:** 2026-09-23
- **Derives from:** [design § Three pipelines](../artifacts/design.md#three-pipelines) (`cruise --output-type json > .graph/cruise.json`, then `fmt --exit-code --output-type err`), [design § The run and its consumers](../artifacts/design.md#the-run-and-its-consumers) (the gate reads the error count through `depcruise-fmt --exit-code --output-type err`), conformance gate 1 layer 5 on dependency-cruiser's own repository
- **Constrains:** `crates/rb-cli` (`cruise`, `fmt`, the help text), `crates/rb-report` (the table of gating reporters)
- **Supersedes:** the clause of [ADR-0008](0008-exit-code-contract.md) that every reporter shares one exit-code function, and its statement that dependency-cruiser always exits with the error count. The codes 2 and 3 and the cap at 255 stand.
- **Implemented by:** [Wave 1 plan](../plans/pending/0001-wave-1-typescript-parity.md), Steps 13 and 18
- **Requirements:** [FR-CORE-06](../prd.md#fr-core-06), [NFR-CONF-01](../prd.md#nfr-conf-01)

## Context

[ADR-0008](0008-exit-code-contract.md) says that dependency-cruiser exits with the number of error-severity violations and that every reporter shares one exit-code function. The first half is true only for some reporters. In dependency-cruiser 18.2.0 each reporter returns its own `exitCode`, and the command line exits with it (`src/cli/index.mjs`). `err`, `err-long`, `null`, `teamcity` and `azure-devops` return `summary.error`. `json`, `csv`, `text`, `dot`, `markdown`, `mermaid`, `d2` and `baseline` return 0. `depcruise-fmt --exit-code` exits with the same reporter's code.

The design's first pipeline depends on this. `rulebearing cruise ... --output-type json > .graph/cruise.json` is one CI step, and `rulebearing fmt --exit-code --output-type err .graph/cruise.json` is the next. If the JSON step exits with the error count, the pipeline stops before the gate step runs and before any ratchet reads the file. Layer 5 made the difference visible on dependency-cruiser's own repository: both tools found the same 7 errors, and dependency-cruiser exited 0 where Rulebearing exited 7.

## Decision

- The violation part of the exit code is the reporter's. A gating reporter exits with the error count: `err`, `err-long`, `null`, `teamcity`, `azure-devops`, and Rulebearing's `github-annotations` and `agent`. Every other reporter exits 0 when the run can be trusted. `rb-report` keeps the list as one table, tested against every output type.
- Everything this ADR's gating reporters count counts together: error-severity violations, expired rules and known-violation entries, and exceeded ratchets ([ADR-0029](0029-ratchets-enforced-by-cruise-and-reported-in-the-summary.md)), capped at 255 as before.
- Codes 2 and 3 do not depend on the reporter. An empty cruise, a vacuous rule, an unreadable ratchet budget or an unsupported file exits 2, and an invalid configuration exits 3, whatever the output type. Removing the silent empty cruise is the reason ADR-0008 exists, and a `json` step that exits 0 on zero modules would bring it back.
- `fmt` without `--exit-code` exits 0, as `depcruise-fmt` does. With it, `fmt` applies the same rule to the reporter it was given.

## Consequences

- The design's pipelines run as written: the JSON step always succeeds when the run is trustworthy, and the `err` step is the gate.
- `cruise --output-type json` with violations now exits 0. A pipeline that relied on the old behaviour must run a gating reporter or `fmt --exit-code`, as it would with dependency-cruiser.
- The help text's exit-code table names the gating reporters.

## Alternatives considered

- **Keep ADR-0008 as written.** Rejected: it breaks the design's reference pipeline and every dependency-cruiser pipeline that saves JSON in one step and gates in another, and a drop-in cannot do that.
- **Make 2 and 3 reporter-dependent too.** Rejected: that brings back the silent empty cruise for anyone who saves JSON.
