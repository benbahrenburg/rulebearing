# Layers only import downward

A layered package works when each layer uses the ones below it and none above it: the web layer calls the services, the services use the domain, and the domain knows neither. import-linter writes this as a `layers` contract. In Rulebearing it is one `forbidden` rule per layer, with `reachable: true` so that a chain through other modules counts as well as a direct import, as it does in import-linter.

- Plan: [plan 0004, Step 5](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables) (the import-linter contract kinds)
- Source: [design § import-linter contracts](../../../docs/artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them)
- Decision: [ADR-0013](../../../docs/adr/0013-ruff-parser-for-python.md) (the import-linter mapping)
- Requirement: [FR-REACH-01](../../../docs/prd.md#fr-reach-01)

## The rule

The lowest layer, `shop/domain/`, may reach neither layer above it; the middle layer, `shop/services/`, may not reach `shop/web/`. Nothing stops a higher layer importing a lower one, and modules inside one layer import each other freely.

<!-- cookbook:rule -->

The file is [rulebearing.yaml](rulebearing.yaml). Its `examples` are the edges `rulebearing test` checks each rule against.

## The same rule for dependency-cruiser

`rulebearing config convert rulebearing.yaml --to dependency-cruiser` writes the rules in dependency-cruiser's format and names what it dropped: the `languages.python` block, since dependency-cruiser reads TypeScript and JavaScript only, the `$schema`, and each rule's `fix` and `examples`.

<!-- cookbook:converted -->

The files are [converted/.dependency-cruiser.json](converted/.dependency-cruiser.json) and [converted/dropped.txt](converted/dropped.txt).

## The failing edge

In [repo/](repo/), `shop/domain/pricing.py` imports `shop.web.formatting` to format a price. That one import makes three findings: the direct one, and two chains, because `shop/domain/order.py` and `shop/services/orders.py` reach the web layer through `pricing.py`. The `Through` column is the chain:

<!-- cookbook:violations -->

## The fix

Formatting belongs to the web layer. Delete `label` from `shop/domain/pricing.py`, and format where the page is built:

```python
# shop/web/views.py
from shop.services import orders
from shop.web import formatting


def show_order(order_id: str) -> str:
    return formatting.money(orders.total(order_id))
```

All three findings go with the one import, because the chains ran through it.

## From an import-linter contract

The same question as import-linter's `layers` contract, in [repo/.importlinter](repo/.importlinter):

<!-- cookbook:import-linter -->

`rulebearing import import-linter` translates it into one rule per lower-to-higher pair of layers, each with `reachable: true`, and a `graph.chainsThrough` that keeps the chains inside the root package as import-linter's graph does ([ADR-0038](../../../docs/adr/0038-a-rule-narrows-the-graph-it-sees.md)):

<!-- cookbook:imported -->

Run over the same repository, the imported rules find the same edges as the rules above:

<!-- cookbook:imported-violations -->

The files are [imported/rulebearing.yaml](imported/rulebearing.yaml) and [imported/expected.json](imported/expected.json).

## Try it

The playground holds this page's repository and rule. **Run** cruises the files in this browser, as `rulebearing cruise` does in `repo/`; paste the imported rules into the configuration and run them, or pick another output type.

<!-- cookbook:playground -->
