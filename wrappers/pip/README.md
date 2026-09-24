# rulebearing

One architecture rule set for TypeScript, .NET and Python: deterministic guardrails an agent can check its own changes against. This package puts the `rulebearing` binary on a Python project's path. Source, documentation and releases: https://github.com/benbahrenburg/rulebearing

```sh
pip install rulebearing
rulebearing --version
rulebearing cruise --config rulebearing.yaml -T err src
```

The command-line surface is the binary's (`rulebearing --help`); the exit codes are those of [ADR-0008](../../docs/adr/0008-exit-code-contract.md). To run each rule as a pytest test, install [pytest-rulebearing](../../adapters/python/pytest-rulebearing/README.md).

## How the package works

The package is a launcher and nothing else ([architecture § Distribution](../../docs/architecture.md#distribution), [FR-DIST-01](../../docs/prd.md#fr-dist-01), [Wave 2, Step 14](../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#214-step-14-test-adapters-and-wrappers-2h)). There is one wheel per release target, each carrying that target's binary at `rulebearing/bin/rulebearing` (`.exe` on Windows), so pip installs the one that matches the machine. There is no source distribution: a build from source is `cargo build --release -p rb-cli`.

| Wheel platform tag | Release target | Installed on |
| --- | --- | --- |
| `manylinux_2_N_x86_64` | `x86_64-unknown-linux-gnu` | Linux x64 with glibc 2.N or newer, N read from the binary |
| `manylinux_2_N_aarch64` | `aarch64-unknown-linux-gnu` | Linux arm64 with glibc 2.N or newer |
| `musllinux_1_1_x86_64` | `x86_64-unknown-linux-musl` (static) | Linux x64 with musl (Alpine) |
| `macosx_11_0_arm64` | `aarch64-apple-darwin` | macOS 11 or newer, arm64 |
| `macosx_10_12_x86_64` | `x86_64-apple-darwin` | macOS 10.12 or newer, x64 |
| `win_amd64` | `x86_64-pc-windows-msvc` | Windows x64 |

The console script `rulebearing` (and `python -m rulebearing`) runs the binary with every argument unchanged. On Linux and macOS the launcher process becomes the binary, so its exit code and signals are the binary's; on Windows it waits and returns the binary's exit code. It never re-implements a subcommand.

| Environment variable | Effect |
| --- | --- |
| `RULEBEARING_BINARY` | Path to a `rulebearing` binary to run instead of the bundled one. A path that does not exist is exit 2 with one line naming it. |

## Building a wheel

[`hatch_build.py`](hatch_build.py) is the hatchling hook that stamps the release version and bundles the binary; the environment says which:

| Variable | Meaning |
| --- | --- |
| `RULEBEARING_WHEEL_BINARY` | the binary to bundle; required, since a wheel without one would install a command that cannot run |
| `RULEBEARING_WHEEL_TARGET` | the Rust target triple of that binary; default the host's |
| `RULEBEARING_WHEEL_GLIBC` | for a `*-linux-gnu` target, the newest glibc symbol version the binary needs (`2.34`), which names the `manylinux` tag; default the host's glibc |
| `RULEBEARING_VERSION` | the version; default the `[workspace.package]` version of the root `Cargo.toml`. A semantic pre-release (`0.2.0-rc.1`) is written as PEP 440 (`0.2.0rc1`) |

```sh
cargo build --release -p rb-cli
pip install build hatchling packaging
RULEBEARING_WHEEL_BINARY=target/release/rulebearing python -m build --wheel --no-isolation wrappers/pip
```

The release workflow builds the six wheels from the release archives and publishes them ([docs/release.md](../../docs/release.md#wrappers-from-waves-1-and-2)).

## Developing this package

Linted and type-checked by the root configuration through `cargo xtask lint` ([ADR-0023](../../docs/adr/0023-documentation-link-and-lint-gates.md)); the tests hold the 70% line floor of [ADR-0018](../../docs/adr/0018-test-coverage-threshold.md). `tests/test_wheel_smoke.py` builds a wheel with the locally built binary, installs it into a new virtual environment and runs `rulebearing --version`.

```sh
pytest wrappers/pip --cov=wrappers/pip        # from the repository root; needs target/release/rulebearing
```
