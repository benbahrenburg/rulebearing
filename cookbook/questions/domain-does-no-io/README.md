# The domain does no I/O

A domain that reads the database or calls the network cannot be tested without them, and its rules end up spread between the domain and the storage code. import-linter writes "these modules may not import those" as a `forbidden` contract. In Rulebearing it is a `forbidden` rule with `reachable: true`, so that a chain through another module counts as well as a direct import, as it does in import-linter unless the contract sets `allow_indirect_imports`.

- Plan: [plan 0004, Step 5](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables) (the import-linter contract kinds)
- Source: [design § import-linter contracts](../../../docs/artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them)
- Decision: [ADR-0013](../../../docs/adr/0013-ruff-parser-for-python.md) (the import-linter mapping)
- Requirement: [FR-REACH-01](../../../docs/prd.md#fr-reach-01)

## The rule

Nothing under `shop/domain/` may reach `shop/db/` or `shop/http/`. `shop/app.py`, which wires the domain to storage and the network, is not under `shop/domain/` and may import all three.

<!-- cookbook:rule -->

The file is [rulebearing.yaml](rulebearing.yaml). Its `examples` are the edges `rulebearing test` checks the rule against. Without `reachable: true` the rule would check direct imports only, which is import-linter's `allow_indirect_imports = True`.

## The same rule for dependency-cruiser

`rulebearing config convert rulebearing.yaml --to dependency-cruiser` writes the rule in dependency-cruiser's format and names what it dropped: the `languages.python` block, the `$schema`, the `fix` and the `examples`.

<!-- cookbook:converted -->

The files are [converted/.dependency-cruiser.json](converted/.dependency-cruiser.json) and [converted/dropped.txt](converted/dropped.txt).

## The failing edge

In [repo/](repo/), `shop/domain/invoice.py` has a helper that loads an invoice's lines from the database before totalling them:

<!-- cookbook:violations -->

## The fix

Load in the caller and pass the values in. Delete `total_of` from the domain, and let `shop/app.py` do the loading:

```python
# shop/app.py
from shop.db import session
from shop.domain import invoice
from shop.http import client


def run(invoice_id: str) -> None:
    total = invoice.total(session.load_lines(invoice_id))
    session.save("invoice", total)
    client.post("/invoices", total)
```

## From an import-linter contract

The same question as import-linter's `forbidden` contract, in [repo/.importlinter](repo/.importlinter):

<!-- cookbook:import-linter -->

`rulebearing import import-linter` translates it into one `reachable` rule over both forbidden modules, with the `graph.chainsThrough` that keeps chains inside the root package ([ADR-0038](../../../docs/adr/0038-a-rule-narrows-the-graph-it-sees.md)):

<!-- cookbook:imported -->

Run over the same repository, the imported rule finds the same edge as the rule above:

<!-- cookbook:imported-violations -->

The files are [imported/rulebearing.yaml](imported/rulebearing.yaml) and [imported/expected.json](imported/expected.json).

## Try it

The playground holds this page's repository and rule. **Run** cruises the files in this browser, as `rulebearing cruise` does in `repo/`; paste the imported rule into the configuration and run it, or pick another output type.

<!-- cookbook:playground -->
