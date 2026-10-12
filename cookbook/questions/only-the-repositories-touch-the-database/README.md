# Only the repositories touch the database

When every query goes through a repository, a schema change is made in one place and the queries can be read in one folder. import-linter writes "only these modules may import that one" as a `protected` contract. In Rulebearing it is a `forbidden` rule whose `from.pathNot` names the modules that may import the protected one.

- Plan: [plan 0004, Step 5](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables) (the import-linter contract kinds)
- Source: [design § import-linter contracts](../../../docs/artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them)
- Decision: [ADR-0013](../../../docs/adr/0013-ruff-parser-for-python.md) (the import-linter mapping)
- Requirement: [FR-REACH-01](../../../docs/prd.md#fr-reach-01)

## The rule

Only `shop/repositories/`, and `shop/db/` itself, may import anything under `shop/db/`. The check is on direct imports, as import-linter's `protected` contract checks them: the web layer reaching the database through a repository is the point of the rule, not a breach of it.

<!-- cookbook:rule -->

The file is [rulebearing.yaml](rulebearing.yaml). Its `examples` are the edges `rulebearing test` checks the rule against.

## The same rule for dependency-cruiser

`rulebearing config convert rulebearing.yaml --to dependency-cruiser` writes the rule in dependency-cruiser's format and names what it dropped: the `languages.python` block, the `$schema`, the `fix` and the `examples`.

<!-- cookbook:converted -->

The files are [converted/.dependency-cruiser.json](converted/.dependency-cruiser.json) and [converted/dropped.txt](converted/dropped.txt).

## The failing edge

In [repo/](repo/), `shop/web/views.py` shows an order through the repository, but counts the orders with a query of its own:

<!-- cookbook:violations -->

## The fix

Put the query in the repository and call it:

```python
# shop/repositories/orders.py
def count_orders() -> int:
    return len(session.query("select id, total from orders"))


# shop/web/views.py
from shop.repositories import orders


def count_orders() -> int:
    return orders.count_orders()
```

## From an import-linter contract

The same question as import-linter's `protected` contract, in [repo/.importlinter](repo/.importlinter):

<!-- cookbook:import-linter -->

`rulebearing import import-linter` translates it into `allowed` rules, as the design's table maps it: one naming the importers the contract allows, one for importers outside the root package, and, because dependency-cruiser's `allowed` rules are one allow-list for the whole file, one that allows every import of a module no protected contract names. An import no `allowed` rule covers is reported as `not-in-allowed`, at `allowedSeverity: error`:

<!-- cookbook:imported -->

Run over the same repository, the imported rules find the same edge as the rule above:

<!-- cookbook:imported-violations -->

The files are [imported/rulebearing.yaml](imported/rulebearing.yaml) and [imported/expected.json](imported/expected.json).

## Try it

The playground holds this page's repository and rule. **Run** cruises the files in this browser, as `rulebearing cruise` does in `repo/`; paste the imported rules into the configuration and run them, or pick another output type.

<!-- cookbook:playground -->
