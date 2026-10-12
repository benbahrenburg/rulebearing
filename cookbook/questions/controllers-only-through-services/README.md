# Controllers only through services

A controller turns a request into a call and a result into a reply. When it reads storage itself, it skips whatever the service applies on the way (validation, permissions, caching), and every change to the storage layer reaches the HTTP layer. The answer is a rule that lets a controller import other controllers, the services and the shared helpers, and nothing else under `src/`.

- Plan: [plan 0004, Step 5](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables)
- Source: the second of the design's three cookbook questions, [design § The developer relations hat](../../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo)
- Rule language: [rules.md § The four families](../../../docs/rules.md#the-four-families)
- Requirement: [FR-REACH-01](../../../docs/prd.md#fr-reach-01)

## The rule

The rule names what a controller may import (`pathNot`) rather than what it may not, so a new storage folder, a queue client or an HTTP client added next year is covered without editing the rule. Packages from `node_modules` are not under `src/` and stay allowed.

<!-- cookbook:rule -->

The file is [rulebearing.yaml](rulebearing.yaml). Its `examples` are the edges `rulebearing test` checks the rule against.

## The same rule for dependency-cruiser

`rulebearing config convert rulebearing.yaml --to dependency-cruiser` writes the rule in dependency-cruiser's format and names what it dropped: the `$schema`, the `fix` and the `examples`.

<!-- cookbook:converted -->

The files are [converted/.dependency-cruiser.json](converted/.dependency-cruiser.json) and [converted/dropped.txt](converted/dropped.txt).

## The failing edge

In [repo/](repo/), `src/controllers/order-controller.ts` goes through `src/services/order-service.ts`, but `src/controllers/report-controller.ts` sums the orders straight from the repository:

<!-- cookbook:violations -->

## The fix

The service already has the function the report controller computes by hand. Call it:

```ts
// src/controllers/report-controller.ts
import { ok } from '../shared/http';
import type { Reply } from '../shared/http';
import { revenueCents } from '../services/order-service';

export function getRevenue(): Reply {
  return ok({ cents: revenueCents() });
}
```

When no service function fits, add one to `src/services/` that wraps the repository call, so the next controller finds it there.

## Try it

The playground holds this page's repository and rule. **Run** cruises the files in this browser, as `rulebearing cruise` does in `repo/`; change the rule and run it again, or pick another output type.

<!-- cookbook:playground -->
