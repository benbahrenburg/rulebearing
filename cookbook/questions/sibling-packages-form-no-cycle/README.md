# Sibling packages form no cycle

Two packages that import each other can only be understood, tested and released together, even when no single module imports itself back. import-linter writes this as an `acyclic_siblings` contract. In Rulebearing's dependency rules it is a `circular` rule with `scope: folder`, which looks for cycles between folders rather than between modules.

- Plan: [plan 0004, Step 5](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables) (the import-linter contract kinds)
- Source: [design § import-linter contracts](../../../docs/artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them)
- Decisions: [ADR-0013](../../../docs/adr/0013-ruff-parser-for-python.md) (the import-linter mapping), [ADR-0034](../../../docs/adr/0034-slices-group-types-or-modules-and-segments.md) (the slice form the import writes)
- Rule language: [rules.md § The four families](../../../docs/rules.md#the-four-families) (`scope: folder`)
- Requirement: [FR-REACH-01](../../../docs/prd.md#fr-reach-01)

## The rule

Every folder under `shop/` is held to it, so a new package is covered the day it is created.

<!-- cookbook:rule -->

The file is [rulebearing.yaml](rulebearing.yaml). A cycle between two folders takes at least two edges, and `rulebearing test` builds its graph from one edge per example, so this rule's `examples` show only what stays allowed; the forbidden side is the cycle in the repository below, which the page's `expected.json` holds the binary to.

## The same rule for dependency-cruiser

`rulebearing config convert rulebearing.yaml --to dependency-cruiser` writes the rule in dependency-cruiser's format, where `scope: folder` means the same, and names what it dropped: the `languages.python` block, the `$schema`, the `fix` and the `examples`.

<!-- cookbook:converted -->

The files are [converted/.dependency-cruiser.json](converted/.dependency-cruiser.json) and [converted/dropped.txt](converted/dropped.txt).

## The failing edge

In [repo/](repo/), an order issues its invoice (`shop/orders/order.py` imports `shop.billing.invoice`), and the dunning run asks whether an order is still open (`shop/billing/dunning.py` imports `shop.orders.status`). No module is on a cycle, but the two packages are, and the rule reports each folder edge of it:

<!-- cookbook:violations -->

## The fix

Break the cycle at its weakest edge. Dunning needs one fact about the order, so let the caller pass it in, and `shop/billing/` no longer imports `shop/orders/`:

```python
# shop/billing/dunning.py
def remind(order_id: str, *, is_open: bool) -> str | None:
    return f"reminder for {order_id}" if is_open else None
```

When the borrowed code is more than one fact, move it into the package that needs it, or into a new package both import.

## From an import-linter contract

The same question as import-linter's `acyclic_siblings` contract, in [repo/.importlinter](repo/.importlinter):

<!-- cookbook:import-linter -->

`rulebearing import import-linter` translates it into slice rules: one per package at or below each ancestor, each slicing that package's children with `segments: 1` and requiring `beFreeOfCycles` ([ADR-0034](../../../docs/adr/0034-slices-group-types-or-modules-and-segments.md)). Slice rules have no dependency-cruiser form, which is why the rule above is the folder-scoped one:

<!-- cookbook:imported -->

Run over the same repository, the imported rules find the same cycle. A slice finding names the slices, `billing` and `orders`, and lists the module edges that close the cycle under `Through`:

<!-- cookbook:imported-violations -->

The files are [imported/rulebearing.yaml](imported/rulebearing.yaml) and [imported/expected.json](imported/expected.json).

## Try it

The playground holds this page's repository and rule. **Run** cruises the files in this browser, as `rulebearing cruise` does in `repo/`; paste the imported rules into the configuration and run them, or pick another output type.

<!-- cookbook:playground -->
