# init fixtures

What `rulebearing init --dry-run` proposes for each TypeScript test bed in [the manifest](../manifest.yaml), committed so that a change in discovery is a reviewable diff ([plan 0001, Step 16](../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-16-init-1f)). Regenerate with [`run.sh`](run.sh) over checkouts at the manifest SHAs. Each file names the SHA it came from. The baseline entries are replaced by their count per rule: they are the test bed's findings, not init's discovery.

| Test bed | Fixture | Notes |
| --- | --- | --- |
| sverweij/dependency-cruiser | [sverweij__dependency-cruiser.yaml](sverweij__dependency-cruiser.yaml) | |
| langfuse/langfuse (`web/`) | [langfuse__langfuse.yaml](langfuse__langfuse.yaml) | The checkout installs `web`'s dependencies only (`pnpm install --filter web...`), so `@langfuse/shared`, `uuid` and `storybook` do not resolve. dependency-cruiser 18.2.0 leaves the same 1,571 imports unresolved on that tree. |
| infinitered/ignite | [infinitered__ignite.yaml](infinitered__ignite.yaml) | |
| microsoft/FluidFramework (`packages/dds/tree`) | none | Layer 5's sparse checkout has only this package. Its `tsconfig.json` extends `common/build/build-common/tsconfig.node20.json`, which is outside the checkout, so `init` exits 2 and names the missing file. A full clone does not have this problem. |

The scale beds the plan names (n8n, grafana, kibana) are the first thing its cut list drops ([plan 0001 § 3](../../docs/plans/pending/0001-wave-1-typescript-parity.md#3-wave-based-delivery-plan)). They join when the nightly run keeps full checkouts of them.
