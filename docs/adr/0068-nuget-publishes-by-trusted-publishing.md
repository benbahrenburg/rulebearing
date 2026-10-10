# ADR-0068: NuGet publishes by trusted publishing, not a stored API key

- **Status:** Accepted (2026-10-10, by the owner: "finish all of the none npm work and commit")
- **Date:** 2026-10-10
- **Derives from:** [ADR-0025](0025-ci-and-supply-chain-hardening.md) ("the release workflow raises [the token] for the jobs that publish, and nowhere else"; actions pinned to a commit SHA); [docs/release.md § NuGet](../release.md#nuget-from-wave-2), which named trusted publishing as the replacement for the key
- **Constrains:** `.github/workflows/release.yml` (`nuget-credentials`, `nuget-publish`), [docs/release.md](../release.md)
- **Implemented by:** [plan 0002, Step 14](../plans/pending/0002-wave-2-dotnet-python-element-rules.md#214-step-14-test-adapters-and-wrappers-2h) (the NuGet wrapper's publishing)
- **Requirements:** [FR-DIST-01](../prd.md#fr-dist-01), [NFR-SEC-01](../prd.md#nfr-sec-01)

## Context

The release workflow pushed every NuGet package with `NUGET_API_KEY`, a nuget.org API key stored as a repository secret. A key like that lasts up to a year, works from anywhere, and has to be rotated by hand before it expires. nuget.org now offers trusted publishing: the owner creates a policy naming a GitHub repository and workflow file, and a run of that workflow exchanges its GitHub OIDC token for an API key that lasts one hour. The exchange is done by the `NuGet/login` action.

The owner created the policy on nuget.org on 2026-10-10:

| Field | Value |
| --- | --- |
| Package owner | `bencoding` |
| Repository | `benbahrenburg/rulebearing` |
| Workflow file | `release.yml` |
| Environment | none |
| Scope | push new packages and package versions, glob `Rulebearing*` |

## Decision

**`nuget-publish` gets its key from the policy, not from a secret.** The job gains `id-token: write` beside `contents: read`, runs `NuGet/login` pinned to commit `8d196754b4036150537f80ac539e15c2f1028841` (v1.2.0) with `user: ${{ secrets.NUGET_USER }}`, and passes the action's `NUGET_API_KEY` output to `dotnet nuget push`. Nothing else in the job changes.

**`nuget-credentials` proves the same path.** A manual run, which is always a dry run, makes the same exchange and pushes the `Rulebearing` 0.0.1 package already on nuget.org with `--skip-duplicate`. A missing or mismatched policy, or a wrong `NUGET_USER`, fails the exchange; a key scoped away from `Rulebearing` fails the push before nuget.org reports the duplicate. So the dry run checks the path a release takes, and publishes nothing.

**The `NUGET_API_KEY` secret is deleted after the first release that publishes through the policy.** Until then it stays, unread, as the way back if the policy has to be withdrawn.

## Consequences

- No long-lived NuGet credential is stored in GitHub once the secret is deleted. A leaked key is useless within the hour.
- Two more jobs hold `id-token: write`. Both are NuGet jobs that push, and both are limited to `workflow_dispatch` or a release tag, which is what ADR-0025 allows.
- The workflow depends on one more third-party action. It is pinned by SHA and Dependabot bumps it like the others.
- `NUGET_USER` must hold the nuget.org account name `bencoding`, not an email address. The dry run fails if it does not.
- Renaming `release.yml`, moving the repository, or adding an environment to `nuget-publish` breaks the exchange until the policy is edited to match.
- PyPI still publishes with `PYPI_TOKEN`. Moving it to trusted publishing is a separate decision.

## Alternatives considered

- **Keep the stored key.** It works, and the dry run on 2026-10-10 proved it. Rejected because it needs manual rotation and is valid from any machine for up to a year.
- **Bind the policy to a GitHub environment.** An environment could add required reviewers before a publish. Deferred: the release already runs only from a pushed `vX.Y.Z` tag after every install check passes, and an environment would also have to admit tags.
- **Write the exchange in a shell step instead of using `NuGet/login`.** Rejected: it would reimplement the token-service protocol that the registry's own action already handles.
