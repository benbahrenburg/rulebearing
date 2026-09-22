# Wrappers

Thin shells over the one static binary ([architecture § Distribution](../docs/architecture.md#distribution), [ADR-0002](../docs/adr/0002-rust-as-implementation-language.md), [ADR-0020](../docs/adr/0020-single-name-across-registries.md)). Each wrapper carries the platform binary and nothing else, has its own test suite with a 70% line-coverage floor ([ADR-0018](../docs/adr/0018-test-coverage-threshold.md)), and is published from [release.yml](../.github/workflows/release.yml) with the same version as the binary.

| Directory | Registry | Package | Lands in |
| --- | --- | --- | --- |
| [npm/](npm/) | npm | `rulebearing`, platform binaries under `optionalDependencies`; also exposes the `rulebearing/vitest` reporter entry point | [Wave 1, 1G](../docs/plans/pending/0001-wave-1-typescript-parity.md) |
| [nuget/](nuget/) | NuGet | `Rulebearing` as a `dotnet tool`, binary under `runtimes/` | [Wave 2, 2H](../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md) |
| [pip/](pip/) | PyPI | `rulebearing` wheel per platform | [Wave 2, 2H](../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md) |

Until those plans land, each directory holds the `0.0.1` name reservation: a README and the packaging metadata, no functionality. [`crates/rulebearing/`](crates/rulebearing/README.md) is the crates.io reservation. [`publish-placeholders.sh`](publish-placeholders.sh) publishes all four on one day ([Wave 0, 0E](../docs/plans/pending/0000-wave-0-spike.md); the procedure is in [docs/release.md](../docs/release.md)).
