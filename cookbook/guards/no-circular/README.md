# no-circular

No import cycles. A cycle closed only through a type-only import does not count, because it disappears when TypeScript is compiled.

- Plan: [plan 0005, Step 1](../../../docs/plans/pending/0005-guard-catalogue.md#step-1-fixture-layout-the-runner-and-the-first-recipe-5a)
- Source: the guard cookbook's cycles recipe ([docs/artifacts/guard-cookbook.html](../../../docs/artifacts/guard-cookbook.html)), corrected as below
- Requirement: [FR-RULE-01](../../../docs/prd.md#fr-rule-01)

## What the graph holds

Six modules from a small TypeScript tree, extracted by `rb-extract-ts` with `tsPreCompilationDeps: true`. Each edge's `circular` is set to `false` and its `cycle` is removed, so the fixture proves the engine finds the cycles itself.

| Modules | Edges | Expected |
| --- | --- | --- |
| `src/order/order.ts`, `pricing.ts`, `discount.ts` | a three-module cycle of ordinary imports | one violation, on the edge that closes the cycle, with the cycle's path |
| `src/user/user.ts`, `profile.ts` | `user.ts` imports `profile.ts`; `profile.ts` imports `user.ts` with `import type` | none: the cycle only exists through a type-only edge |
| `src/main.ts` | imports both entry modules | none |

In a real run, the `type-only` dependency type comes from `rb-extract-ts` with `tsPreCompilationDeps: true`. Without that option, a type-only import is elided before extraction, and the cycle is not in the graph at all.

## The correction to the cookbook

The cookbook writes `to: { circular: true, dependencyTypesNot: [type-only] }`. That filters only the edge being reported, not the cycle it belongs to. The `user.ts` to `profile.ts` edge is an ordinary import on a cycle, so it fires, even though the cycle closes only through the type-only edge back. dependency-cruiser 18.2.0 reports it the same way.

`viaOnly: { dependencyTypesNot: [type-only] }` requires every edge of the cycle to be other than type-only, which is what the fix text promises. The cookbook export is dated and is not edited ([docs/artifacts/README.md](../../../docs/artifacts/README.md)); this fixture is the corrected recipe.

## The examples

`rulebearing test` builds a one-edge graph per example, so the forbidden example is a module that imports itself: a cycle of one.
