# ADR-0071: The rule library ships from this repository, at the release version

- **Status:** Accepted (2026-10-10, by the owner: "let's revise the plan to keep everything in a single repo")
- **Date:** 2026-10-10
- **Amends:** [FR-REACH-04](../prd.md#fr-reach-04) ("`rulebearing-rules` MUST be a public repository ... versioned with a changelog"), and [plan 0003](../plans/pending/0003-wave-3-operations-surface-inner-loop.md) Step 23 and its "Where framework presets live" decision
- **Derives from:** [design § The developer relations hat](../artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo) ("A rule improves in one place; the presets live there"); [ADR-0020](0020-single-name-across-registries.md); [ADR-0068](0068-nuget-publishes-by-trusted-publishing.md), [ADR-0069](0069-npm-publishes-by-trusted-publishing.md)
- **Constrains:** `presets/frameworks/`, `.github/workflows/release.yml`, `scripts/sync-rule-library.sh`, [docs/release.md](../release.md), the `benbahrenburg/rulebearing-rules` repository
- **Requirements:** [FR-REACH-04](../prd.md#fr-reach-04), [FR-DIST-01](../prd.md#fr-dist-01)

## Context

FR-REACH-04 asks for a public rule library, `rulebearing-rules`, published to npm, NuGet and PyPI, that any repository can `extends`, versioned with a changelog. Plan 0003 built it as a second repository, `benbahrenburg/rulebearing-rules`, created on 2026-10-10. That repository holds:

- a copy of the five framework presets and `recommended`;
- CI;
- its own release workflow, which publishes all three packages at the library's own version.

The binary already bundles the same presets as `rulebearing:<name>`. `scripts/sync-rule-library.sh` keeps the two copies in step.

The second repository has never published. It needs its own credentials or trust settings on all three registries, and none of its three package names exists yet. This repository already publishes to all three registries through trust settings and secrets that the `v0.1.1` and `v0.1.2` releases proved ([ADR-0068](0068-nuget-publishes-by-trusted-publishing.md), [ADR-0069](0069-npm-publishes-by-trusted-publishing.md)).

A separate repository earns its cost only when a preset needs to change between binary releases. So far every preset change has been made here and released with the binary.

## Decision

**The rule library lives in this repository and ships with each release, at the release version.**

- **One source.** `presets/frameworks/` is the source of the framework presets and `recommended`. The binary bundles them as `rulebearing:<name>`, as it does now.
- **Built and published by `release.yml`.** The release packs the same files as three packages, with the names unchanged: `rulebearing-rules` on npm, `Rulebearing.Rules` on NuGet and `rulebearing-rules` on PyPI. They are published from the same `vX.Y.Z` tag as the binary and the wrappers, after the same install checks, so `extends: rulebearing-rules/<name>` resolves from all three package layouts exactly as it does today.
- **Versioned with the binary.** The library's version is the workspace version. Its changelog is the release notes: each release lists the presets it changed.
- **One set of credentials.** Publishing uses this repository's trust settings:
  - **NuGet:** the existing trusted-publishing policy's glob, `Rulebearing*`, already covers `Rulebearing.Rules`.
  - **PyPI:** the account-wide `PYPI_TOKEN` covers the new project.
  - **npm:** npm cannot set trust on a package that does not exist, so the first `rulebearing-rules` publish is done once from the maintainer's session, with 2FA. It then gets the same `npm trust github` setting as the other eight packages.
- **No second copy.** `scripts/sync-rule-library.sh` is removed, because there is nothing to sync.
- **The separate repository is archived.** `benbahrenburg/rulebearing-rules` is archived once the first release from here has published the library. Its README points to this repository. It never published, so no registry history moves.

## Consequences

- A preset fix ships with the next binary release. Nothing can be published faster than that, which is acceptable while presets change only alongside the binary.
- A team pins the rule version through its package manager, as FR-REACH-04 intends. The version is now shared with the binary, so upgrading the rules and upgrading Rulebearing move together.
- One release, one set of trust settings, one CI. The library's `rulebearing test` over every preset runs in this repository's CI.
- The PRD's "public repository" now means this public repository. FR-REACH-04's text is amended to say so and cites this ADR.

## Alternatives considered

- **Keep the second repository.** It would version the rules separately. Rejected for now: it adds a repository, a release workflow and three registry setups for a benefit nobody uses yet. Revisit if presets start changing faster than the binary.
- **Keep the repository but defer its publishing.** Rejected: it leaves two copies of the presets to keep in step, with nothing gained until it publishes.
