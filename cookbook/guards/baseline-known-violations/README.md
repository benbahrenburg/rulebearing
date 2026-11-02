# baseline-known-violations

Accept today's findings, each with a deadline. Both entry shapes are here: a stable `id` with `expires` and `owner`, and dependency-cruiser's own shape.

- Plan: [plan 0005, Step 4](../../../docs/plans/pending/0005-guard-catalogue.md#step-4-the-lifecycle-recipes-and-their-negative-cases-5e)
- Source: the guard cookbook's budgets and exceptions section ([docs/artifacts/guard-cookbook.html](../../../docs/artifacts/guard-cookbook.html))
- Requirement: [FR-RULE-09](../../../docs/prd.md#fr-rule-09)
- Decisions: [ADR-0015](../../../docs/adr/0015-stable-violation-id.md), [ADR-0030](../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md), [ADR-0031](../../../docs/adr/0031-a-saved-result-carries-what-the-exit-code-counts.md)

## What the graph holds

Two pages each import server code, so the rule has two findings, and each matches one baseline entry. Both are reported at severity `ignore` and the gate exits 0. A third edge, a page importing `lib/format.ts`, does not fire.

In a real run the edges come from `rb-extract-ts`, and `rulebearing adopt` or `baseline` writes the entries.

## Negative cases

| Case | What differs | Exit | Why |
| --- | --- | --- | --- |
| `expired` | the id entry expired on 2020-01-01 | 2 | a count, not "untrustworthy": the finding is back at `error` (1) and the expired entry is one more (1) ([ADR-0030](../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md)). `expected.json` holds both, and the output names `RB-40dba9c6` |
| `unrelated-edit` | the graph has one more module and edge, away from both findings | 0 | the id is unchanged, so the entry still matches; `expected.json` asserts both ids and severities ([ADR-0015](../../../docs/adr/0015-stable-violation-id.md)) |

The expired case's exit code equals the plan's "exit 2", but only because this recipe's sum is 2. With no finding left behind, an expired entry alone would exit 1, as [`expiring-exception-rule`](../expiring-exception-rule/README.md) records.

## Differences from the cookbook

The cookbook's second entry has `from`, `to` and `rule` only. An entry in dependency-cruiser's shape is matched with dependency-cruiser's `isSameViolation`, which compares `type` too, so without `type: dependency` that entry matches nothing and the finding stays an error. This recipe carries `type`. dependency-cruiser's `depcruise-baseline` writes it as well.
