# The mutation branch

Conformance gate 1, layer 5, second half ([plan 0001, Step 18](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-18-gate-1-layer-5-the-mutation-branch-oracle-zero-diff-1g); [NFR-CONF-01](../../../docs/prd.md#nfr-conf-01), [NFR-CONF-03](../../../docs/prd.md#nfr-conf-03)). The zero-diff half proves that both tools agree on repositories whose rules mostly pass; this half proves that both tools report a violation of every rule shape when one is there. dependency-cruiser's own repository, at the SHA [testbeds/manifest.yaml](../../../testbeds/manifest.yaml) pins, is the tree; the "branch" is held as a patch, applied at run time to a fresh checkout, so nothing from the repository is committed here.

| File | What it is |
| --- | --- |
| [mutations.patch](mutations.patch) | New files only: `.dependency-cruiser.mutations.cjs` (twelve rules, one per shape) and `src/mutation/<nn>-<shape>/` (the violating modules). Applies with `git apply` at the manifest SHA |
| [expected.json](expected.json) | The twelve violations, `{ rule, from, to }`, that each tool must report, and nothing else |

## The twelve shapes

Each rule is scoped to its own folder under `src/mutation/`, so it fires exactly on its mutation and nowhere else in the tree. The rest of `src` and `bin` is cruised with it, so every mutation sits inside the real graph.

| Rule | Shape | The mutation |
| --- | --- | --- |
| `m01-forbidden` | regular forbidden (`from.path`, `to.path`) | `01-forbidden/uses-cli.mjs` imports `src/cli/format-meta-info.mjs` |
| `m02-path-not` | `to.pathNot` | `02-path-not/uses-graph-utl.mjs` imports `src/utl/bus.mjs` (allowed) and `src/graph-utl/compare.mjs` (not) |
| `m03-fence` | a `$1` fence (`from.path` capture, `to.pathNot: "...$1/"`) | `03-fence/alpha/a.mjs` imports its sibling `03-fence/beta/b.mjs` |
| `m04-circular` | `to.circular` | `04-circular/a.mjs` and `b.mjs` import each other |
| `m05-orphan` | `from.orphan` | `05-orphan/lonely.mjs` imports nothing and nothing imports it |
| `m06-dependency-types-not` | `to.dependencyTypesNot` | `06-dependency-types-not/uses-core.mjs` imports `node:path`, a `core` edge where only `local` is allowed |
| `m07-reachable` | `to.reachable: true` in `forbidden` | `07-reachable/entry.mjs` reaches `forbidden.mjs` through `middle.mjs` |
| `m08-required` | `required` | `08-required/skips-the-bus.mjs` does not depend on `src/utl/bus.mjs` |
| `m09-dependents` | `module.numberOfDependentsLessThan` | `09-dependents/shared.mjs` has one dependent where two are required |
| `not-in-allowed` | `allowed` with `allowedSeverity` | `10-allowed/a.mjs` imports `src/utl/bus.mjs`; the folder may only import itself |
| `m11-via` | `to.via` on a cycle | `11-via/a.mjs` and `hub.mjs` form a cycle through the hub |
| `m12-could-not-resolve` | `to.couldNotResolve` | `12-could-not-resolve/imports-nothing.mjs` imports a file that does not exist |

## Running it

```sh
conformance/dependency-cruiser/scripts/run-layer-5.sh --mutations
```

The script clones dependency-cruiser's repository at the manifest SHA into the layer 5 checkout folder, applies the patch, installs its dependencies, runs dependency-cruiser at the pinned version ([PIN](../PIN)) and `rulebearing cruise --no-liveness` with `--config .dependency-cruiser.mutations.cjs src bin`, and passes both results to [zero-diff.mjs](../harness/zero-diff.mjs) with `--expect expected.json`. It fails unless each tool reports exactly the twelve expected violations and the two results agree field for field, as the oracle zero-diff requires ([divergences.md](../../divergences.md)).

## Changing it

The patch is new files only, so it keeps applying as the pinned SHA moves. To add or change a mutation, edit the files in a checkout at the manifest SHA, regenerate the patch with `git add -N . && git diff > mutations.patch`, run `--mutations`, and update `expected.json` and the table above in the same change.
