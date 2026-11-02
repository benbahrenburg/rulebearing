# expiring-exception-rule

A temporary rule with an owner and a date. Until the date it is an ordinary rule; the day after, the run fails until someone deletes or renews it.

- Plan: [plan 0005, Step 4](../../../docs/plans/pending/0005-guard-catalogue.md#step-4-the-lifecycle-recipes-and-their-negative-cases-5e)
- Source: the guard cookbook's budgets and exceptions section ([docs/artifacts/guard-cookbook.html](../../../docs/artifacts/guard-cookbook.html))
- Requirement: [FR-CFG-07](../../../docs/prd.md#fr-cfg-07)
- Decisions: [ADR-0030](../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md), [ADR-0031](../../../docs/adr/0031-a-saved-result-carries-what-the-exit-code-counts.md)

## What the graph holds

`packages/orders` imports `packages/http`, so the rule's `from` has a match and it fires on nothing. `packages/legacy-http` imports its own `client.ts`, so `to.path` has a match and `config lint` does not call the rule dead; `pathNot` keeps that internal edge legal. The date is 2099-12-31 so the fixture does not depend on today. In a real run the edges come from `rb-extract-ts`.

## Negative case

| Case | What differs | Exit | Why |
| --- | --- | --- | --- |
| `expired` | `expires: 2020-01-01` | 1 | an expired rule is one error from a gating reporter ([ADR-0030](../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md)); `summary.expired[]` names it ([ADR-0031](../../../docs/adr/0031-a-saved-result-carries-what-the-exit-code-counts.md)) |

## The exit code disagrees with the plan and the PRD

[FR-CFG-07](../../../docs/prd.md#fr-cfg-07)'s acceptance and plan 0005's Step 4 say an expired rule exits 2. [ADR-0030](../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md) and [ADR-0031](../../../docs/adr/0031-a-saved-result-carries-what-the-exit-code-counts.md), both accepted, count it as one error, and the engine does what they say. The design says only that "the run fails the day after". This fixture records what the accepted decisions specify and the plan's status table carries the open question.

## Differences from the cookbook

The cookbook writes `expires: 2026-12-31` on the rule that also has the exception list; here the two are separate recipes, and the list is [`defines-exception-list`](../defines-exception-list/README.md).
