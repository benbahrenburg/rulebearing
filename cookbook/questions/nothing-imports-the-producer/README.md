# Nothing imports the producer

The event producer is a process of its own: it drains the outbox and publishes to the broker. Code that imports it publishes from the request path instead, so a request now fails when the broker is down, and an event can be published for a change that is later rolled back. The answer is a rule that no module outside `src/producer/` imports anything inside it.

- Plan: [plan 0004, Step 5](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables)
- Source: the third of the design's three cookbook questions, [design § The developer relations hat](../../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo)
- Rule language: [rules.md § The four families](../../../docs/rules.md#the-four-families)
- Requirement: [FR-REACH-01](../../../docs/prd.md#fr-reach-01)

## The rule

`from.pathNot` leaves the producer's own modules out, so `src/producer/main.ts` may still start `src/producer/producer.ts`. Everything else, including modules added later, is held to it.

<!-- cookbook:rule -->

The file is [rulebearing.yaml](rulebearing.yaml). Its `examples` are the edges `rulebearing test` checks the rule against.

## The same rule for dependency-cruiser

`rulebearing config convert rulebearing.yaml --to dependency-cruiser` writes the rule in dependency-cruiser's format and names what it dropped: the `$schema`, the `fix` and the `examples`.

<!-- cookbook:converted -->

The files are [converted/.dependency-cruiser.json](converted/.dependency-cruiser.json) and [converted/dropped.txt](converted/dropped.txt).

## The failing edge

In [repo/](repo/), `src/orders/place-order.ts` writes its event to the outbox, as it should. `src/orders/cancel-order.ts` writes to the outbox and then publishes straight away through the producer:

<!-- cookbook:violations -->

## The fix

Write the event to the outbox and stop there; the producer process publishes it on its next pass:

```ts
// src/orders/cancel-order.ts
import { enqueue } from '../outbox/outbox';

export function cancelOrder(id: string): void {
  enqueue({ topic: 'order-cancelled', payload: JSON.stringify({ id }) });
}
```

If an event must be published sooner, shorten the producer's interval; do not publish from the request.

## Try it

The playground holds this page's repository and rule. **Run** cruises the files in this browser, as `rulebearing cruise` does in `repo/`; change the rule and run it again, or pick another output type.

<!-- cookbook:playground -->
