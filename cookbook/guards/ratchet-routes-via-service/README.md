# ratchet-routes-via-service

A count of edges that may only fall. Route files reach the database through the service layer, and the direct edges that remain are counted against a budget file.

- Plan: [plan 0005, Step 4](../../../docs/plans/pending/0005-guard-catalogue.md#step-4-the-lifecycle-recipes-and-their-negative-cases-5e)
- Source: the guard cookbook's budgets and exceptions section ([docs/artifacts/guard-cookbook.html](../../../docs/artifacts/guard-cookbook.html))
- Requirement: [FR-RULE-06](../../../docs/prd.md#fr-rule-06)
- Decision: [ADR-0029](../../../docs/adr/0029-ratchets-enforced-by-cruise-and-reported-in-the-summary.md)

## What the graph holds

| Edge | Counted |
| --- | --- |
| `apps/web/src/app/orders/page.tsx` to `apps/web/src/server/db.ts` | yes |
| `apps/admin/src/app/users/route.ts` to `apps/admin/src/server/db.ts` | yes |
| `apps/web/src/app/reports/page.tsx` to `apps/admin/src/server/db.ts` | no: `$1` holds it to the same app |
| `apps/web/src/app/orders/page.tsx` to `apps/web/src/lib/format.ts` | no: not server or domain code |
| `apps/web/src/server/db.ts` to `apps/web/src/domain/order.ts` | no: the source is not a route |

The budget's ceiling is 2 and the count is 2, so `summary.ratchets` says `held` with no headroom, and the gate exits 0. `summary.ratchets` carries `count` and `ceiling`; `rulebearing count --json` is what prints `headroom`.

In a real run the edges come from `rb-extract-ts`; a ratchet reads only `source` and `resolved`, so any extractor supplies them. A relative `budget` resolves against the working directory, not the configuration file.

## Negative cases

| Case | What differs | Exit | Why |
| --- | --- | --- | --- |
| `exceeded` | `budget.json` has a ceiling of 1 | 1 | an exceeded ratchet is one error ([ADR-0029](../../../docs/adr/0029-ratchets-enforced-by-cruise-and-reported-in-the-summary.md)) |
| `missing-budget` | `budget.json` is removed | 2 | the run cannot be trusted |
| `raise-refused` | a ceiling of 1, and the command is `count --write` | 1 | the ceiling only falls; the runner proves `budget.json` is byte-identical afterwards |

## Differences from the cookbook

A ratchet has no `examples` key in the schema, so `rulebearing test` has nothing to run for this recipe; the negative cases are its proof.
