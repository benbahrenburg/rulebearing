# Features must not know each other

A feature folder is something a team can build, test, hand over or delete on its own. That stops being true the day one feature imports another, because from then on the two ship together. The answer is one rule with a capture: a module under `src/features/<x>/` may import anything except a module under another `src/features/<y>/`.

- Plan: [plan 0004, Step 5](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables)
- Source: the first of the design's three cookbook questions, [design § The developer relations hat](../../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo)
- Rule language: [rules.md § Captures](../../../docs/rules.md#captures)
- Requirement: [FR-REACH-01](../../../docs/prd.md#fr-reach-01)

## The rule

`from.path` captures the feature's folder name as `$1`, and `to.pathNot` uses it, so an import inside the same feature is allowed and an import of any other feature is not. Shared code lives in `src/shared/`, and the app shell `src/main.ts` may import every feature, because neither is under `src/features/`. A new feature folder is covered the day it is created; nothing lists the features by name.

<!-- cookbook:rule -->

The file is [rulebearing.yaml](rulebearing.yaml). Its `examples` are the edges `rulebearing test` checks the rule against.

## The same rule for dependency-cruiser

`rulebearing config convert rulebearing.yaml --to dependency-cruiser` writes the rule in dependency-cruiser's format, which dependency-cruiser runs as it is, and names what it dropped: the `$schema`, the `fix` and the `examples`, which dependency-cruiser has no field for.

<!-- cookbook:converted -->

The files are [converted/.dependency-cruiser.json](converted/.dependency-cruiser.json) and [converted/dropped.txt](converted/dropped.txt).

## The failing edge

The repository in [repo/](repo/) has three features and a shared folder. `src/features/cart/cart.ts` calls `findProduct` from the catalog feature, so `rulebearing cruise` reports:

<!-- cookbook:violations -->

## The fix

The cart needs a product's price, not the catalog. Let the cart take the product as an argument, and let the app shell, which may import both features, look it up:

```ts
// src/features/cart/cart.ts
export function addToCart(lines: readonly Line[], product: Line): Line[] {
  return [...lines, product];
}

// src/main.ts
const product = findProduct('sku-1');
console.log(charge(total(addToCart([], { sku: product.sku, price: product.price }))));
```

When two features need the same code rather than each other's results, move that code into `src/shared/`.

## Try it

The playground holds this page's repository and rule. **Run** cruises the files in this browser, as `rulebearing cruise` does in `repo/`; change the rule and run it again, or pick another output type.

<!-- cookbook:playground -->
