# Packages do not import each other

Orders, billing and shipping are owned and changed separately, so none of them may depend on another; what they share lives in a fourth package they all may use. import-linter writes this as an `independence` contract. In Rulebearing it is one `forbidden` rule with a capture, and `reachable: true` so that a chain through a shared module counts too, as it does in import-linter.

- Plan: [plan 0004, Step 5](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables) (the import-linter contract kinds)
- Source: [design § import-linter contracts](../../../docs/artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them)
- Decision: [ADR-0013](../../../docs/adr/0013-ruff-parser-for-python.md) (the import-linter mapping)
- Rule language: [rules.md § Captures](../../../docs/rules.md#captures)
- Requirement: [FR-REACH-01](../../../docs/prd.md#fr-reach-01)

## The rule

`from.path` captures which of the three packages the importer is in, and `to.pathNot` lets it reach only its own. `shop/common/` is in none of the three, so every package may use it.

<!-- cookbook:rule -->

The file is [rulebearing.yaml](rulebearing.yaml). Its `examples` are the edges `rulebearing test` checks the rule against.

## The same rule for dependency-cruiser

`rulebearing config convert rulebearing.yaml --to dependency-cruiser` writes the rule in dependency-cruiser's format and names what it dropped: the `languages.python` block, the `$schema`, the `fix` and the `examples`.

<!-- cookbook:converted -->

The files are [converted/.dependency-cruiser.json](converted/.dependency-cruiser.json) and [converted/dropped.txt](converted/dropped.txt).

## The failing edge

In [repo/](repo/), placing an order issues its invoice: `shop/orders/place.py` imports `shop.billing.invoice`.

<!-- cookbook:violations -->

## The fix

Let the caller that needs both, outside the three packages, call each in turn, and keep `place` to taking the order:

```python
# shop/orders/place.py
from shop.common import ids


def place(total_cents: int) -> str:
    return ids.new_id("order")


# shop/checkout.py
from shop.billing import invoice
from shop.orders import place


def checkout(total_cents: int) -> str:
    order_id = place.place(total_cents)
    invoice.issue(order_id, total_cents)
    return order_id
```

## From an import-linter contract

The same question as import-linter's `independence` contract, in [repo/.importlinter](repo/.importlinter):

<!-- cookbook:import-linter -->

`rulebearing import import-linter` translates it into one `reachable` rule per ordered pair of packages, each with the `graph.chainsThrough` that keeps chains inside the root package ([ADR-0038](../../../docs/adr/0038-a-rule-narrows-the-graph-it-sees.md)):

<!-- cookbook:imported -->

Run over the same repository, the imported rules find the same edge as the rule above:

<!-- cookbook:imported-violations -->

The files are [imported/rulebearing.yaml](imported/rulebearing.yaml) and [imported/expected.json](imported/expected.json).

## Try it

The playground holds this page's repository and rule. **Run** cruises the files in this browser, as `rulebearing cruise` does in `repo/`; paste the imported rules into the configuration and run them, or pick another output type.

<!-- cookbook:playground -->
