# ADR-0009: dependency-cruiser's and ArchUnitNET's test suites are the specification

- **Status:** Accepted
- **Date:** 2026-09-20
- **Derives from:** [design.md § Conformance gate 1](../artifacts/design.md#conformance-gate-1-dependency-cruisers-tests-validate-rulebearing), [§ Conformance gate 2](../artifacts/design.md#conformance-gate-2-archunitnets-test-assemblies-validate-the-element-rules), [§ Test beds](../artifacts/design.md#test-beds-open-source-repositories-to-validate-against), [§ Specification coverage](../artifacts/design.md#specification-coverage)
- **Constrains:** [architecture.md § Verification strategy](../architecture.md#verification-strategy)
- **Implemented by:** every wave plan; the harness lands in [Wave 0](../plans/pending/0000-wave-0-spike.md)

## Context

"Superset" is a claim that must be measured. dependency-cruiser (MIT) ships 546 extraction fixtures, 25 validation specs, graph-utility specs, one directory per reporter, and whole-cruise expectations. ArchUnitNET (Apache 2.0) ships `TestAssembly` and `ArchUnitNETTests`, whose fluent assertions specify every predicate and condition. Both are pinned to versions.

## Decision

Two conformance gates are required checks from the first pull request and both ratchet.

**Gate 1, dependency-cruiser 18.2.0**, in five layers: (1) every `test/extract` fixture byte-compared through `rb-extract-ts`; (2) the original `test/validate` and `test/graph-utl` specs run unmodified under a Node harness with the `#validate` and `#graph-utl` imports remapped to a shim that calls `rulebearing validate --rules - --module -`; (3) every `test/report/<reporter>` fixture byte-compared through `rulebearing fmt --from dependency-cruiser`, version string normalised; (4) every emitted `json` validates against the pinned `cruise-result` schema with extensions stripped, and every accepted config validates against the `configuration` schema; (5) zero-diff on the TypeScript oracle repositories at pinned commits, plus a mutation branch with twelve deliberate violations both tools must report.

**Gate 2, ArchUnitNET 0.13.4**: `TestAssembly` built with a portable PDB and committed as a fixture; each test in `ArchUnitNETTests/Fluent/Syntax/Elements/**` ported to a data-driven case with the same selection and expected pass or fail set; NetArchTest's test project treated the same way; the .NET oracle repositories' tests, imported by `import archunit`, must agree with `dotnet test`.

**Ratchets:** `conformance/excluded.json` (with a reason per entry) may only shrink; the unported-test count may only fall. The two coverage tabs in `docs/artifacts/` are the ledger: a row may not say Parity until the pinned suite says so.

**Test beds:** oracle, greenfield and scale repositories are cloned at pinned SHAs in a nightly run that publishes a zero-diff table, the `init` fixtures and a timing table; a timing regression over 20% fails the nightly run.

## Consequences

- The maintainer is not the reviewer of record; the suites are. A contributor who changes an extractor knows within minutes whether it still agrees with the upstream tool.
- Bumping a pinned upstream version re-runs the newer specs automatically, because the specs are not copied.
- Licences: MIT fixtures are vendored freely; Apache 2.0 fixtures carry their notice file ([ADR-0019](0019-mit-licence.md)).

## Alternatives considered

- **Hand-written tests only.** Rejected: they would encode the maintainer's reading of the upstream tools rather than the tools themselves.
