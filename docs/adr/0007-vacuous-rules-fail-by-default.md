# ADR-0007: A rule whose selection is empty fails by default

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § Why](../artifacts/design.md#why) (liveness), [§ The name](../artifacts/design.md#the-name), [§ The five stages](../artifacts/design.md#the-five-stages) (stage 4), [§ What agents actually do with rules](../artifacts/design.md#what-agents-actually-do-with-rules)
- **Constrains:** [architecture.md § The rule engine](../architecture.md#the-rule-engine), [ADR-0008](0008-exit-code-contract.md)
- **Implemented by:** [Wave 1 plan](../plans/pending/0001-wave-1-typescript-parity.md)

## Context

Four rules in the reference monorepo matched zero files for months and read as standing fences. ArchUnitNET fails an empty selection by default; dependency-cruiser does not, and heavy users bolt the check on. The product's name is this check.

## Decision

- Every rule of every family (dependency, element, slice, diagram, ratchet) whose `from`, `module` or `select` side matches nothing is **vacuous**, is listed in `summary.vacuousRules[]`, and makes the run exit with code 2 ([ADR-0008](0008-exit-code-contract.md)).
- `allowEmpty: true` on a rule, or `WithoutRequiringPositiveResults` semantics in compatibility mode, turns the check off for that rule.
- In dependency-cruiser compatibility mode the check is still on, because it is a run-trust condition rather than a rule violation; `--no-liveness` disables it globally for a repository that has its own guard.
- `rules --json` reports `fromMatches` and `toMatches` so a guard can see the liveness of every rule without running the check.

## Consequences

- A rule an agent writes that matches nothing cannot merge green.
- Conformance gate 1 must run the dependency-cruiser specs with liveness disabled, since those specs do not expect it; the harness passes `--no-liveness` and this is documented in `conformance/README.md`.

## Alternatives considered

- **Warn only.** Rejected: the monorepo's four dead rules were warned about by nothing and noticed by no one.
