# Releasing

How Rulebearing reaches its four registries and GitHub releases. Decisions: [ADR-0020](adr/0020-single-name-across-registries.md) (one name everywhere, reserved on one day), [ADR-0019](adr/0019-mit-licence.md) (MIT), [ADR-0025](adr/0025-ci-and-supply-chain-hardening.md) (pinned, least-privileged workflows). Plan: [wave 0, Step 11](plans/pending/0000-wave-0-spike.md#step-11-name-reservation-and-release-plumbing-0e).

## The 0.0.1 name reservation (wave 0, done once)

Four placeholder packages hold the name `rulebearing` (`Rulebearing` on NuGet). Each carries a README saying what the name is reserved for and where the project lives, and nothing else. They are published by hand from the maintainer's own session, on one day, in this order, because a lookup proves a name is free now, not that it stays free.

| Order | Registry | Package source | Credential in the session |
| --- | --- | --- | --- |
| 1 | crates.io | [`wrappers/crates/rulebearing/`](../wrappers/crates/rulebearing/README.md) (outside the workspace; every workspace crate is `rb-*`) | `cargo login` |
| 2 | npm | [`wrappers/npm/`](../wrappers/npm/README.md) | `npm login` |
| 3 | PyPI | [`wrappers/pip/`](../wrappers/pip/README.md) | `TWINE_USERNAME=__token__`, `TWINE_PASSWORD` |
| 4 | NuGet | [`wrappers/nuget/`](../wrappers/nuget/README.md) | `NUGET_API_KEY` |

```sh
wrappers/publish-placeholders.sh --dry-run   # packages all four and runs each registry's dry run; needs no credentials
wrappers/publish-placeholders.sh             # publishes, in the order above; checks the @rulebearing npm scope
```

Then, the same day:

1. Create the GitHub organisation `rulebearing` (the web interface only; the API cannot create one).
2. Open each registry page, confirm it shows 0.0.1 and the reservation README, and record the four URLs and the `@rulebearing` scope result in the plan's 0E status table.

If a registry rejects the name, stop: [ADR-0020](adr/0020-single-name-across-registries.md) names the reserves (`plumbrule`, then `trussworthy`), and switching is a new ADR, not a rename in place.

Registry tokens stay in the maintainer's keychain in wave 0. None is stored in GitHub until wave 1's release workflow publishes the npm wrapper.

## Binaries (from wave 0)

[`.github/workflows/release.yml`](../.github/workflows/release.yml) builds `rulebearing` for the six targets of [architecture § Distribution](architecture.md#distribution): Linux x86-64 (glibc and musl) and ARM64, macOS ARM64 and x86-64, Windows x86-64.

| Trigger | What happens |
| --- | --- |
| Manual run with `dry_run` on | builds every target, uploads the archives as a workflow artefact, releases nothing |
| Tag `vX.Y.Z-rc.N` | the same dry run, from a tag |
| Tag `vX.Y.Z` | builds every target and attaches the archives and `SHA256SUMS` to a GitHub release |

The workspace version in `Cargo.toml` is the release version; tag the commit that carries it.

## Wrappers (from waves 1 and 2)

The npm wrapper (wave 1) and the NuGet and PyPI wrappers (wave 2) replace the placeholders with packages that carry the platform binary and nothing else ([wrappers/README.md](../wrappers/README.md)). Their publishing steps join `release.yml` with the plans that build them, so one tag releases the binary and every wrapper at one version.
