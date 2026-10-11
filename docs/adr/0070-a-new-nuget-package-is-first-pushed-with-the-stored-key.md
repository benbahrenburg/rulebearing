# ADR-0070: A new NuGet package ID is first pushed with the stored key

- **Status:** Accepted (2026-10-10, by the owner: "do A")
- **Date:** 2026-10-10
- **Amends:** [ADR-0068](0068-nuget-publishes-by-trusted-publishing.md). Its decision that `NUGET_API_KEY` is deleted after the first release through the policy is withdrawn; the rest stands.
- **Derives from:** [ADR-0025](0025-ci-and-supply-chain-hardening.md); release run [38085491053](https://github.com/benbahrenburg/rulebearing/actions/runs/38085491053) (`v0.1.1`)
- **Constrains:** `.github/workflows/release.yml` (`nuget-publish`), [docs/release.md](../release.md)
- **Implemented by:** [plan 0002, Step 14](../plans/implemented/0002-wave-2-dotnet-python-element-rules.md#214-step-14-test-adapters-and-wrappers-2h)
- **Requirements:** [FR-DIST-01](../prd.md#fr-dist-01)

## Context

ADR-0068 assumed that the trusted-publishing policy, with the scope "push new packages and package versions" and the glob `Rulebearing*`, would let the release create the package IDs that did not exist yet. It did not.

In the `v0.1.1` release, `nuget-publish` exchanged its OIDC token and pushed `Rulebearing` 0.1.1. nuget.org then refused `Rulebearing.Analyzer` with 403 ("does not have permission to access the specified package"). A rerun with the same policy, which shows the scope and glob above and is Active, failed the same way.

nuget.org's documentation says a policy "will apply to all packages owned by the selected owner". A package ID that does not exist has no owner, so the policy's key cannot create it. The seven `Rulebearing.TestAdapter*` packages and `Rulebearing.Analyzer` were therefore not published.

## Decision

**`nuget-publish` tries the trusted-publishing key for every package, and falls back to `NUGET_API_KEY` for a package that key cannot push.** A push that fails with 403 is retried with the `NUGET_API_KEY` secret. That key has the scope "push new packages and package versions" and the glob `Rulebearing*`. The job writes a notice naming the package. A failure that is not a 403, or a 403 with the secret unset, fails the job and names the package.

**`NUGET_API_KEY` stays as a repository secret.** It is used only for a package's first push. Every later version of that package goes through the policy, because by then the package has an owner.

## Consequences

- After a package's first release, its versions are pushed without a stored credential. The stored key is read only when a release adds a NuGet package, such as `Rulebearing.Rules` from plan 0003, 3G, which another repository publishes.
- The key expires within a year and must be renewed by hand. A missing or expired key fails only a release that adds a package, and the error says so.
- The release that adds this fallback is `v0.1.2`. Rerunning `v0.1.1` would run the workflow as it was at that tag, without the fallback.

## Alternatives considered

- **Push each new package once from the maintainer's machine with a short-lived key.** Rejected by the owner: a second key on the day the policy was made, and a manual step for every future package.
- **Remove trusted publishing and push everything with `NUGET_API_KEY`.** Rejected: the policy works for versions of existing packages, which is every push after the first.
