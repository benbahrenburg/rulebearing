# Releasing

How Rulebearing reaches its four registries and GitHub releases. Decisions: [ADR-0020](adr/0020-single-name-across-registries.md) (one name everywhere, reserved on one day), [ADR-0019](adr/0019-mit-licence.md) (MIT), [ADR-0025](adr/0025-ci-and-supply-chain-hardening.md) (pinned, least-privileged workflows). Plan: [wave 0, Step 11](plans/pending/0000-wave-0-spike.md#step-11-name-reservation-and-release-plumbing-0e).

## The 0.0.1 name reservation (wave 0, done once)

Four placeholder packages hold the name `rulebearing` (`Rulebearing` on NuGet). Each carries a README saying what the name is reserved for and where the project lives, and nothing else. They are published by hand from the maintainer's own session, on one day, in this order, because a lookup proves a name is free now, not that it stays free.

| Order | Registry | Package source | Credential in the session |
| --- | --- | --- | --- |
| 1 | crates.io | [`wrappers/crates/rulebearing/`](../wrappers/crates/rulebearing/README.md) (outside the workspace; every workspace crate is `rb-*`) | `cargo login` |
| 2 | npm | [`wrappers/npm/`](../wrappers/npm/README.md) | `npm login` |
| 3 | PyPI | [`wrappers/pip/`](../wrappers/pip/README.md) (now the real package; the script no longer builds it) | `TWINE_USERNAME=__token__`, `TWINE_PASSWORD` |
| 4 | NuGet | [`wrappers/nuget/`](../wrappers/nuget/README.md) | `NUGET_API_KEY` |

```sh
wrappers/publish-placeholders.sh --dry-run   # packages all four and runs each registry's dry run; needs no credentials
wrappers/publish-placeholders.sh             # publishes, in the order above; checks the @rulebearing npm scope
```

Then, the same day:

1. Create the GitHub organisation `rulebearing` (the web interface only; the API cannot create one).
2. Open each registry page, confirm it shows 0.0.1 and the reservation README, and record the four URLs and the `@rulebearing` scope result in the plan's 0E status table.

If a registry rejects the name, stop: [ADR-0020](adr/0020-single-name-across-registries.md) names the reserves (`plumbrule`, then `trussworthy`), and switching is a new ADR, not a rename in place.

Registry tokens stay in the maintainer's keychain in wave 0. None is stored in GitHub until wave 1's release workflow publishes the npm wrapper; wave 2 adds `PYPI_TOKEN` and `NUGET_API_KEY` ([below](#pypi-from-wave-2)).

## Binaries (from wave 0)

[`.github/workflows/release.yml`](../.github/workflows/release.yml) builds `rulebearing` for the six targets of [architecture § Distribution](architecture.md#distribution): Linux x86-64 (glibc and musl) and ARM64, macOS ARM64 and x86-64, Windows x86-64.

| Trigger | What happens |
| --- | --- |
| Manual run with `dry_run` on | builds every target, uploads the archives as a workflow artefact, releases nothing |
| Tag `vX.Y.Z-rc.N` | the same dry run, from a tag |
| Tag `vX.Y.Z` | builds every target and attaches the archives and `SHA256SUMS` to a GitHub release |

The workspace version in `Cargo.toml` is the release version; tag the commit that carries it.

## Wrappers (from waves 1 and 2)

The npm wrapper (wave 1) and the NuGet and PyPI wrappers (wave 2) replace the placeholders with packages that carry the platform binary and nothing else ([wrappers/README.md](../wrappers/README.md)). Their publishing steps are in `release.yml`, so one tag releases the binary and every wrapper at one version ([FR-DIST-01](prd.md#fr-dist-01), [plan 0002 Step 14](plans/pending/0002-wave-2-dotnet-python-element-rules.md#214-step-14-test-adapters-and-wrappers-2h)). A `version` job reads the workspace version once, checks that a tag carries it, and hands it to every packing job.

| Registry | Packages | Pack | Install check (macOS arm64, Linux x64, Windows x64) | Publish (a `vX.Y.Z` tag only) | Credential |
| --- | --- | --- | --- | --- | --- |
| npm | `rulebearing` (with `rulebearing/vitest`), six `rulebearing-cli-<platform>` | `npm-pack` | `npm-install-check` | `npm-publish`, with provenance | `NPM_TOKEN`; the job alone holds `id-token: write` |
| PyPI | six `rulebearing` wheels, `pytest-rulebearing` | `pypi-build` | `pypi-install-check` | `pypi-publish`, with twine | `PYPI_TOKEN`; `contents: read` |
| NuGet | `Rulebearing` (dotnet tool), and what `wrappers/nuget/pack.sh` packs | `nuget-pack` | `nuget-install-check` | `nuget-publish`, with `dotnet nuget push` | `NUGET_API_KEY`; `contents: read` |

Each publish job needs its install check and the GitHub `release` job, so nothing is published that did not install and run, and nothing is published before the release exists. A dry run packs and install-checks all three registries and publishes nothing. Each publish step skips a version already on the registry (`npm view`, `twine --skip-existing`, `--skip-duplicate`), so rerunning a partly published release finishes it.

## The npm wrapper (from wave 1)

[`wrappers/npm/`](../wrappers/npm/README.md) is the `rulebearing` package: a launcher that runs the binary from one of six unscoped platform packages, `rulebearing-cli-<platform>`, listed under `optionalDependencies` ([plan 0001 Step 19](plans/pending/0001-wave-1-typescript-parity.md#step-19-npm-package-github-action-release-1g), [ADR-0020](adr/0020-single-name-across-registries.md); the `@rulebearing` scope is not held). The committed `package.json` carries the workspace version; the release version is stamped at staging time and the source tree is never edited.

[`release.yml`](../.github/workflows/release.yml) adds three jobs after `binaries`:

| Job | Runs on | What it does |
| --- | --- | --- |
| `npm-pack` | every trigger | checks that a tag carries the workspace version, builds the launcher, runs `wrappers/npm/scripts/stage.mjs` over the six archives, builds and stages [`eslint-plugin-rulebearing`](../frontends/eslint-plugin-rulebearing/README.md) with the version stamped into it and its `rulebearing` dependency ([plan 0002 Step 13](plans/pending/0002-wave-2-dotnet-python-element-rules.md#213-step-13-worktree-aware-cache-and-the-eslint-plugin-2g)), and packs the eight packages with `npm pack` into the `npm-packages` artefact |
| `npm-install-check` | every trigger | on macOS arm64, Linux x64 and Windows x64, installs `rulebearing` and the host's platform package from those tarballs into an empty project and asserts that `npx rulebearing --version` prints `rulebearing <version>` |
| `npm-publish` | a `vX.Y.Z` tag only, after `release` and the install check | `npm publish --provenance --access public` for the six platform packages, then `rulebearing`, then `eslint-plugin-rulebearing`, from the same tarballs |

A dry run (a manual run or an `-rc` tag) therefore packs and install-checks exactly what a release would publish, and publishes nothing. `npm-publish` alone holds `id-token: write`, which provenance needs; every other job keeps `contents: read` ([ADR-0025](adr/0025-ci-and-supply-chain-hardening.md)). It reads the `NPM_TOKEN` repository secret, an npm automation token with publish rights on the eight package names; the platform names and `eslint-plugin-rulebearing` are created by their first publish.

To stage and install the packages by hand on one machine, without publishing:

```sh
cargo build --release -p rb-cli
mkdir -p /tmp/rb/dist /tmp/rb/content && cp target/release/rulebearing LICENSE README.md /tmp/rb/content/
tar -czf /tmp/rb/dist/rulebearing-aarch64-apple-darwin.tar.gz -C /tmp/rb/content .   # the host's target
(cd wrappers/npm && npm run stage -- /tmp/rb/dist 0.0.1 /tmp/rb/staged --partial)
npm pack /tmp/rb/staged/rulebearing /tmp/rb/staged/rulebearing-cli-darwin-arm64 --pack-destination /tmp/rb
(mkdir -p /tmp/rb/project && cd /tmp/rb/project && npm init -y && npm install /tmp/rb/*.tgz && npx rulebearing --version)
```

The install check also imports `rulebearing/vitest` from the installed package, with `vitest@5` as its peer, and checks that `defineArchitectureTests` and `RulebearingReporter` are exported ([adapters/vitest](../adapters/vitest/README.md)).

## PyPI (from wave 2)

[`wrappers/pip/`](../wrappers/pip/README.md) is the `rulebearing` project: one wheel per release target, each with that target's binary in the package data and a console script `rulebearing`. [`adapters/python/pytest-rulebearing/`](../adapters/python/pytest-rulebearing/README.md) is the `pytest-rulebearing` project, a pure-Python wheel that depends on exactly `rulebearing==<version>`. Both stamp the release version at build time through a hatchling hook; the committed metadata carries none, and a semantic pre-release is written as PEP 440 (`0.2.0-rc.1` is `0.2.0rc1`).

| Job | What it does |
| --- | --- |
| `pypi-build` | runs [`wrappers/pip/scripts/build-wheels.sh`](../wrappers/pip/scripts/build-wheels.sh) over the six archives with pinned `build`, `hatchling`, `packaging` and `twine`; asserts six `rulebearing` wheels; `twine check --strict`; uploads the `pypi-packages` artefact |
| `pypi-install-check` | installs `rulebearing` from the built wheels only (`--no-index`) and `pytest-rulebearing` from them (pytest from PyPI) into a new venv, asserts `rulebearing --version` prints `rulebearing <version>`, then runs `pytest` with `rulebearing = true` over [adapters/fixture](../adapters/fixture/README.md) and asserts three failures, two passes and the liveness reason |
| `pypi-publish` | `twine upload --skip-existing`, the `rulebearing` wheels first, then `pytest-rulebearing` |

The `manylinux` tag of a `*-linux-gnu` wheel is the newest `GLIBC_2.N` symbol version the binary needs, read with `readelf`, so a wheel never claims a glibc older than the binary requires. The musl build is static and is tagged `musllinux_1_1_x86_64`; the macOS tags are Rust's default deployment targets (11.0 for arm64, 10.12 for x64).

`PYPI_TOKEN` is a PyPI API token scoped to the `rulebearing` and `pytest-rulebearing` projects. Trusted publishing would need `id-token: write` on `pypi-publish` and a third-party action pinned by commit; it can replace the token in a later change without touching the other jobs.

To build and install the wheels by hand on one machine, without publishing (the archive from [the npm example above](#the-npm-wrapper-from-wave-1)):

```sh
pip install build==1.6.1 hatchling==1.32.4 packaging==26.3
PYTHON=python wrappers/pip/scripts/build-wheels.sh /tmp/rb/dist 0.1.0 /tmp/rb/pypi --partial
python -m venv /tmp/rb/venv && /tmp/rb/venv/bin/pip install --no-index --find-links /tmp/rb/pypi rulebearing
/tmp/rb/venv/bin/rulebearing --version
```

`pytest wrappers/pip --cov=wrappers/pip` does the same for the host through `tests/test_wheel_smoke.py`.

## NuGet (from wave 2)

The `Rulebearing` dotnet tool and the `Rulebearing.TestAdapter` packages are built under [`wrappers/nuget/`](../wrappers/nuget/README.md) and `adapters/dotnet/` ([plan 0002 Step 14](plans/pending/0002-wave-2-dotnet-python-element-rules.md#214-step-14-test-adapters-and-wrappers-2h)). `release.yml` depends on one entry point there, and on nothing else in the .NET tree:

```sh
wrappers/nuget/pack.sh <dist-dir> <version> <out-dir>
```

| Contract | |
| --- | --- |
| Input | `<dist-dir>` holds the six `rulebearing-<target>.tar.gz` archives the `binaries` job builds; `<version>` is the workspace version |
| Output | every `.nupkg` to publish, in `<out-dir>`, at `<version>`: `Rulebearing.<version>.nupkg`, the dotnet tool with the binaries under `runtimes/<rid>/native/`, and the seven `Rulebearing.TestAdapter` packages (the core and its xUnit, xUnit v3, NUnit, MSTest v2, MSTest v4 and TUnit packages) |
| Environment | the .NET 10 SDK on `ubuntu-latest`; no network beyond NuGet restore; no credentials |
| Check | `dotnet tool install Rulebearing --version <version> --tool-path <dir> --add-source <out-dir>`, then `<dir>/rulebearing --version` prints `rulebearing <version>` on macOS arm64, Linux x64 and Windows x64 |

`nuget-pack` fails with a named error when `pack.sh` is absent or writes no `Rulebearing.<version>.nupkg`. `nuget-publish` pushes every package in the artefact with `--skip-duplicate`. `NUGET_API_KEY` is a nuget.org API key with push rights, scoped to the `Rulebearing` package-id prefix.
