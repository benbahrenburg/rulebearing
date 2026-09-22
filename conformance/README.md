# Conformance

The upstream tools' test suites are Rulebearing's specification ([ADR-0009](../docs/adr/0009-conformance-suites-as-specification.md)). Both gates are required checks in [ci.yml](../.github/workflows/ci.yml) from the first pull request, and both ratchet: [excluded.json](excluded.json) may only shrink, and the unported-test count under `archunitnet/` may only fall. The two coverage tabs in [docs/artifacts](../docs/artifacts/) are the ledger a row cannot mark **Parity** until the pinned suite says so.

| Gate | Upstream | Pinned | Layers | Plan |
| --- | --- | --- | --- | --- |
| 1 | [dependency-cruiser](https://github.com/sverweij/dependency-cruiser) (MIT) | v18.2.0 | extract fixtures byte-compared; `test/validate` and `test/graph-utl` specs run unmodified through a shim; report fixtures byte-compared; schema validation; oracle zero-diff plus a mutation branch | [Wave 0, 0B](../docs/plans/pending/0000-wave-0-spike.md) then every wave |
| 2 | [ArchUnitNET](https://github.com/TNG/ArchUnitNET) (Apache-2.0), [NetArchTest](https://github.com/BenMorris/NetArchTest) (MIT) | 0.13.4, 1.3.2 | `TestAssembly` built with a portable PDB and committed; each fluent test ported to a data-driven case; oracle agreement with `dotnet test` | [Wave 0, 0B](../docs/plans/pending/0000-wave-0-spike.md), [Wave 2, 2C](../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md) |

Liveness ([ADR-0007](../docs/adr/0007-vacuous-rules-fail-by-default.md)) is disabled with `--no-liveness` when running the dependency-cruiser specs, because those specs do not expect it. Every other default is the tool's own.

Layout:

```
conformance/
├── excluded.json              # specs routed through the sidecar, with a reason each; may only shrink
├── check-ratchet.sh           # fails a pull request that grows excluded.json or the unported count
├── dependency-cruiser/
│   ├── run.sh                 # installs dependency-cruiser@18.2.0 and runs the five layers
│   ├── shim/                  # #validate and #graph-utl remapped to `rulebearing validate`
│   ├── fixtures/              # vendored test/extract and test/report expectations (MIT)
│   └── oracles/               # pinned commits and their diff results
└── archunitnet/
    ├── run.sh                 # builds TestAssembly with a portable PDB, runs the ported cases
    ├── NOTICE                 # Apache-2.0 notice for the vendored fixtures
    ├── TestAssembly/          # the committed fixture assembly and PDB
    └── ported/                # one data-driven case per upstream test; unported.txt is the ratchet
```
