# ADR-0020: One name, `rulebearing`, on every registry

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § The name](../artifacts/design.md#the-name), [§ Open questions](../artifacts/design.md#open-questions) (Name ownership)
- **Constrains:** [architecture.md § Distribution](../architecture.md#distribution)
- **Implemented by:** [Wave 0 plan](../plans/pending/0000-wave-0-spike.md)

## Decision

- `rulebearing` on npm, PyPI and crates.io; `Rulebearing` on NuGet; the binary `rulebearing`; the config file `rulebearing.yaml`; the repository `benbahrenburg/rulebearing`; the GitHub organisation `rulebearing`.
- A placeholder `0.0.1` is published to all four registries on the same day in wave 0, and the repository and organisation are created then, because a lookup proves a name is free now, not that it stays free.
- The npm organisation `@rulebearing` is checked from a signed-in session before any scoped package is planned.
- Reserves, if the name is lost before publication: `plumbrule`, then `trussworthy`.

## Consequences

- Wave 0 has a release-pipeline task before any feature ships.
- Every crate is `rb-*` internally and only the binary and the wrappers carry the public name.
