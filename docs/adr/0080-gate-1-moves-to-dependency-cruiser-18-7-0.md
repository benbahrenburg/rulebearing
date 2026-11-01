# ADR-0080: Gate 1 moves to dependency-cruiser 18.7.0

- **Status:** Accepted (2026-11-01, by the owner, who asked for plan 0011 to be implemented)
- **Date:** 2026-11-01
- **Supersedes:** the pin in [ADR-0009](0009-conformance-suites-as-specification.md) ("Gate 1, dependency-cruiser 18.2.0"). The principle stands: the pinned upstream suite is the specification, and the gates ratchet.
- **Amends:** the list of what a gating reporter counts in [ADR-0030](0030-the-reporter-decides-the-error-count-exit.md) and the sum `fmt --exit-code` computes in [ADR-0031](0031-a-saved-result-carries-what-the-exit-code-counts.md); [ADR-0053](0053-x-dot-webpage-draws-with-graphviz-dot.md) (the output type's name); [ADR-0037](0037-baseline-modes.md) (the modes reach `cruise`)
- **Derives from:** [design § Conformance gate 1](../artifacts/design.md#conformance-gate-1-dependency-cruisers-tests-validate-rulebearing), [design § Specification coverage](../artifacts/design.md#specification-coverage), [design § Exit codes](../artifacts/design.md#exit-codes), [design § Reporters](../artifacts/design.md#reporters)
- **Constrains:** `conformance/dependency-cruiser/` (the pin, the harness, layer 5), `conformance/excluded.json`, `conformance/ledger/`, `crates/rb-model` (the summary), `crates/rb-config` (the new options), `crates/rb-rules` (`summarize`), `crates/rb-report` (the gating table, `err`, `err-html`, `markdown`), `crates/rb-cli` (the flags, the exit-code function, the help snapshot), `crates/rb-extract-ts`, `schema/`, `wrappers/npm` (the vendored types), `crates/rb-node`, `crates/rb-wasm`
- **Implemented by:** plan 0011, dependency-cruiser 18.7.0 (local, [ADR-0039](0039-plans-and-design-kept-local.md))
- **Requirements:** [NFR-CONF-01](../prd.md#nfr-conf-01), [FR-CORE-06](../prd.md#fr-core-06), [FR-CFG-01](../prd.md#fr-cfg-01), [FR-RULE-09](../prd.md#fr-rule-09), [FR-EXT-TS-01](../prd.md#fr-ext-ts-01), [FR-OUT-01](../prd.md#fr-out-01), [FR-DIST-02](../prd.md#fr-dist-02)

## Context

Gate 1 is pinned to dependency-cruiser 18.2.0. Upstream has since released 18.3.0, 18.3.1, 18.4.0, 18.5.0, 18.6.0 and 18.7.0. A user who runs the current dependency-cruiser gets behaviour Rulebearing does not have, and the promise is "superset, precisely; nothing is dropped". The releases add, in summary:

| Version | Change that reaches Rulebearing's output or contract |
| --- | --- |
| 18.3.0 | `--baseline [file]` and `--baseline-mode full\|shrink-only` on the main command; `options.ignoreKnown`; `x-dot-webpage` renamed `dot-webpage`, the old name kept; swc reads tsx, jsx and type-only imports |
| 18.4.0 | `summary.baselineSize`, `summary.baselineStale` and a mandatory `summary.advisedExitCode`; `options.baseline.staleEntriesSeverity`; `--baseline-mode format`; stale-entry lines in `err`, `err-html` and `markdown`; reordered help text |
| 18.5.0 | `options.detectImportsInAmbientModules`; the markdown reporter's stale-baseline table and its five reporter options |
| 18.6.0 | Svelte components with top-level `await` |
| 18.7.0 | only manifest keys that end in `ependencies` classify a dependency; swc covers every TypeScript extension and type-only exports; more precise compiler warnings in `summary.environment` |

The supported transpiler ranges do not change: `@babel/core` stays `>=7 <8` and `typescript` `>=2 <7` (`src/meta.cjs`).

Two of these changes touch the exit-code contract. Upstream computes `advisedExitCode` in `src/analyze/summarize/index.mjs` as `summary.error`, plus `summary.baselineStale` when `options.baseline.staleEntriesSeverity` is `"error"` (the default is `"warn"`). Its `null` reporter exits with `advisedExitCode`, falling back to `summary.error`. `err`, `err-long`, `azure-devops` and `teamcity` do the same, so a stale entry in the baseline can now fail a run upstream.

## Decision

- **The pin moves to 18.7.0.** `conformance/dependency-cruiser/PIN` reads `18.7.0`, the harness and layer 5 run the 18.7.0 suite and binaries, and `conformance/excluded.json` records `"pin": "18.7.0"`. The ratchet base is recorded again at the new pin, because a list keyed to 18.2.0 says nothing about specs that did not exist then. The pin moves last, in the change that makes every new spec pass, so the base starts at the count the 18.2.0 list ends at (zero today) and does not grow.
- **The 18.2.0 records stay.** `conformance/ledger/dependency-cruiser-18.2.0.json` and the 18.2.0 coverage tab are kept. A new ledger file and a new tab are added for 18.7.0, the tab exported from the design document as [ADR-0001](0001-record-architecture-decisions.md) requires, never written by hand.
- **`summary.advisedExitCode` keeps upstream's meaning.** `rb-rules` writes it on every summary with upstream's formula and nothing else: `error`, plus `baselineStale` when `staleEntriesSeverity` is `error`. It is not capped. `summary.baselineSize` and `summary.baselineStale` are written when known violations are in use, as upstream does. Rulebearing's own counted items (expired entries, exceeded ratchets) stay out of these fields, for the reason [ADR-0031](0031-a-saved-result-carries-what-the-exit-code-counts.md) kept them out of `summary.error`.
- **The gating count changes in one place.** A gating reporter ([ADR-0030](0030-the-reporter-decides-the-error-count-exit.md)) exits with `advisedExitCode`, or `summary.error` on a result that has no `advisedExitCode` (one written by dependency-cruiser before 18.4.0), plus the expired entries and the exceeded ratchets. The cap at 255, `--exit-code-mode strict`, and the precedence of codes 2 and 3 do not change. `fmt --exit-code` computes the same sum from the saved result.
- **The contract table is unchanged in shape; its vectors grow.** The meanings of 0, 1 to 255, 2 and 3 in [ADR-0008](0008-exit-code-contract.md) stand. The fixed-vector test of the exit-code function gains rows for stale entries under each `staleEntriesSeverity` value (`error` counts them, `warn`, `info` and `ignore` do not, absent means `warn`), for a result without `advisedExitCode`, and for stale entries together with expired entries and ratchets. No existing row changes its expected code.
- **`dot-webpage` is the name and `x-dot-webpage` stays an alias.** Both are accepted by `cruise`, `fmt` and the npm API, as upstream's enum lists both. [ADR-0053](0053-x-dot-webpage-draws-with-graphviz-dot.md)'s decision about GraphViz applies to both names unchanged.
- **The baseline modes reach `cruise`.** `cruise --baseline [file] --baseline-mode full|shrink-only|format` behaves as upstream's main command does, writing the `baseline` output to the file. `rulebearing baseline` keeps [ADR-0037](0037-baseline-modes.md)'s modes, lifecycle fields and its `shrink-only` exit code (the number of removed entries). Where upstream's `cruise --baseline-mode` and ADR-0037 disagree, `cruise` follows upstream and the `baseline` subcommand keeps ADR-0037, and the plan records each difference with its upstream test.
- **Every platform takes the change together.** The binary, `rb-node`'s `cruise()` and `format()`, `rb-wasm` and the playground, the npm types (vendored again from the 18.7.0 package), and the PyPI and NuGet wrappers, which carry the binary, ship the new fields and options in the same release. The test adapters and front-ends read only additive JSON fields, and each gets a test that a result with the new summary fields passes through.

## Consequences

- Every committed cruise-result fixture and golden that has a summary gains `advisedExitCode`. That is a wide but mechanical diff, and each regenerated golden is explained in its pull request.
- A team that sets `staleEntriesSeverity: error` sees a gating reporter fail on a stale baseline entry, exactly as dependency-cruiser fails. A team that does not set it sees no change in exit codes.
- The help snapshot changes to upstream's new order and wording. That is a reviewable diff, explained where it is regenerated.
- `options.baseline` stops being a key that is accepted and recorded without effect. `mode` and `staleEntriesSeverity` now act.
- The Babel 8 Dependabot skip stays valid, because the transpiler ranges do not move.
- The next upstream release follows the same route: a plan, then a new ADR that moves the pin.

## Alternatives considered

- **Stay on 18.2.0.** Rejected: users of the current dependency-cruiser would lose features on switching, which breaks the superset promise.
- **Put expired entries and ratchets into `advisedExitCode`.** Rejected: the field is dependency-cruiser's, and a consumer that reads it would get a different number from each tool for the same repository.
- **Move the pin first and exclude the failing new specs.** Rejected: it grows the ratchet list, which only shrinks.
- **Edit ADR-0009 in place.** Rejected: an accepted ADR is not edited, and a reversal is a new ADR.
