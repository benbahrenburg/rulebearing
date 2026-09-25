# ADR-0015: Every violation carries a stable id and a line and column

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § Precision an agent can act on](../artifacts/design.md#precision-an-agent-can-act-on), [§ Reporters](../artifacts/design.md#reporters) (`sarif` fingerprints), [§ What agents actually do with rules](../artifacts/design.md#what-agents-actually-do-with-rules)
- **Constrains:** [architecture.md § The graph document](../architecture.md#the-graph-document), [§ Outputs and CI contract](../architecture.md#outputs-and-ci-contract)
- **Implemented by:** [Wave 1 plan](../plans/pending/0001-wave-1-typescript-parity.md)

## Context

dependency-cruiser and ArchUnitNET stop at the file or the type. An agent given a line edits the right import on the first try; a reviewer saying "fix RB-4f2a" needs the reference to mean the same thing across runs.

## Decision

- Every edge carries `line` and `column` from the AST span, the PDB sequence point or the Python node.
- Every violation carries an `id`: `RB-` followed by the first eight hex characters of a SHA-256 over the rule name, the `from` path, the `to` path and the `dependencyKind`, in that order, separated by `\n`. Line numbers are excluded so the id survives an unrelated edit above the import.
- The id is the SARIF `partialFingerprints` value, the `knownViolations` baseline key, and the reference in the `agent`, `junit`, `trx` and pull-request outputs.
- Every report carries a receipt: `summary.inspected` with counts of files, assemblies and modules per language.

## Consequences

- Baselines written by `baseline` survive line churn and fail loudly when the rule or the edge changes.
- The hashing function lives in `rb-model` and is covered by a fixed-vector test, since a change would invalidate every baseline in the wild.

## Alternatives considered

- **Include the line in the hash.** Rejected: every unrelated edit would churn the baseline.
- **A sequential id.** Rejected: not stable across runs or machines.
