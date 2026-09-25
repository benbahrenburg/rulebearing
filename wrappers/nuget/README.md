# Rulebearing

One architecture rule set for TypeScript, .NET and Python: deterministic guardrails an agent can check its own changes against. This package is the `rulebearing` binary as a `dotnet tool`. Source, documentation and releases: https://github.com/benbahrenburg/rulebearing

```sh
dotnet tool install -g Rulebearing
rulebearing --version
rulebearing cruise --config rulebearing.yaml --output-type err
```

To run the rules as tests in an xUnit, NUnit, MSTest or TUnit project, add the matching `Rulebearing.TestAdapter.*` package ([adapters/dotnet](../../adapters/dotnet/README.md)); it finds this tool on `PATH`. The command-line surface is the binary's (`rulebearing --help`); the exit codes are those of [ADR-0008](../../docs/adr/0008-exit-code-contract.md).

## How the package works

The package is a launcher and nothing else ([architecture § Distribution](../../docs/architecture.md#distribution), [FR-DIST-01](../../docs/prd.md#fr-dist-01), [plan 0002 Step 14](../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#214-step-14-test-adapters-and-wrappers-2h)). It carries the binary for each release target of [release.yml](../../.github/workflows/release.yml) under `runtimes/<rid>/native/`, beside a small `net8.0` launcher that runs on any later runtime (`RollForward` `Major`).

| Runtime identifier | Release target |
| --- | --- |
| `linux-x64` | `x86_64-unknown-linux-gnu` |
| `linux-musl-x64` | `x86_64-unknown-linux-musl` |
| `linux-arm64` | `aarch64-unknown-linux-gnu` |
| `osx-arm64` | `aarch64-apple-darwin` |
| `osx-x64` | `x86_64-apple-darwin` |
| `win-x64` | `x86_64-pc-windows-msvc` |

The launcher takes the identifier from the operating system and process architecture, and musl from the runtime's own identifier (a musl build of .NET reports `linux-musl-*`). It marks the binary executable (NuGet does not keep file modes), runs it with every argument unchanged and the console inherited, and exits with the binary's exit code. It never re-implements a subcommand. With no binary for the machine it prints one line naming the identifier and the fix, and exits 2.

| Environment variable | Effect |
| --- | --- |
| `RULEBEARING_BINARY` | Path to a `rulebearing` binary to run instead of the packaged one. A path that does not exist is exit 2. |

## Developing this package

The launcher is `src/`, its tests are `tests/Rulebearing.Tool.Tests` at the 70% line floor of [ADR-0018](../../docs/adr/0018-test-coverage-threshold.md), and the package version is the workspace version in `Cargo.toml` unless `-p:Version` says otherwise ([eng/Rulebearing.Packages.props](../../eng/Rulebearing.Packages.props)).

```sh
wrappers/nuget/pack.sh <dist-dir> <version> <out-dir>   # the release: the tool from the six archives, and the test adapters
wrappers/nuget/pack.sh --archives <dist-dir> --version <version> --out <dir> [--partial]
wrappers/nuget/pack.sh --binary target/release/rulebearing --rid osx-arm64 --version <version> --out <dir>
wrappers/nuget/smoke.sh                                  # pack for this machine, install into a temp tool path, run --version
dotnet test wrappers/nuget/tests/Rulebearing.Tool.Tests
```

`pack.sh` refuses a tool package without all six binaries unless it is given `--partial` or a single `--binary`; its three-argument form is the entry point `release.yml` calls, and packs every package to publish. The release flow is in [docs/release.md](../../docs/release.md). This path held the `0.0.1` name reservation, published on 2026-09-22 ([ADR-0020](../../docs/adr/0020-single-name-across-registries.md)); this package replaces it.
