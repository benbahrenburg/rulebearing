# Framework preset fixtures

One small repository per framework preset under `<name>/repo/`, and the proposal `rulebearing init --dry-run --owner @fixture --preset <name>` writes for it, committed as `<name>/rulebearing.yaml` and byte-compared by [`framework_presets.rs`](../../framework_presets.rs) ([plan 0003, Step 11](../../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#23-steps-for-sub-wave-3c-presets-lifecycle-fields-snapshot-and-changelog)). The presets are [`presets/frameworks/`](../../../../../presets/frameworks/README.md). It is this repository's own code, under its MIT licence.

| Fixture | What it holds | What the proposal shows |
| --- | --- | --- |
| `nextjs` | an App Router tree with `app/api/`, a `dashboard` route with its own `components/` and `lib/`, and shared `components/`, `lib/` and `server/` | `src/lib/links.ts` calling the route handler, baselined under `nextjs-no-import-of-api-routes`, `nextjs-no-import-of-route-entries` (a `route.ts` is an entry) and `nextjs-shared-code-not-to-routes`; no finding for `src/components/ItemCount.tsx` (an `import type` of the handler and an import of `server/`) or for the route's colocated folders |
| `clean-architecture` | none of its own: the [`init-layers`](../init-layers/PROVENANCE.md) solution, whose built assemblies the .NET extractor reads (the source paths are the ones its PDBs record, `crates/rb-cli/tests/fixtures/init-layers/src/...`) | `Shop.Application` using `Shop.Infrastructure`, baselined under the namespace rule init proposes and the preset's path rule alike |
| `django` | one app with models, views, URLs, a form and a migration, run with `--preset python,django` | `shop/forms.py` importing a view, baselined under `django-views-only-from-urls` |
| `fastapi` | routers, services and schemas, and no repository layer | `fastapi-repositories-not-to-services-or-routers` left out with `severity: ignore`; `app/services/audit.py` importing a router, baselined |
| `vertical-slices` | two features under `src/features/` and a `src/shared/` kernel | `cart` importing `checkout`, baselined under `slices-are-independent` and init's own `features-are-independent` |

Regenerate with `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-cli --test framework_presets` and explain the diff in review.
