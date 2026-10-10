# ADR-0069: npm publishes by trusted publishing, with a token only for a package's first publish

- **Status:** Accepted (2026-10-10, by the owner: "yes", to making the npm workflow change on a branch)
- **Date:** 2026-10-10
- **Derives from:** [ADR-0025](0025-ci-and-supply-chain-hardening.md) (the release workflow raises the token only for the jobs that publish; actions pinned); [ADR-0068](0068-nuget-publishes-by-trusted-publishing.md) (the same move for NuGet); [ADR-0020](0020-single-name-across-registries.md) (the unscoped package names)
- **Constrains:** `.github/workflows/release.yml` (`npm-credentials`, `npm-publish`), [docs/release.md](../release.md)
- **Implemented by:** [plan 0001, Step 19](../plans/implemented/0001-wave-1-typescript-parity.md#step-19-npm-package-github-action-release-1g) (the npm package and its release)
- **Requirements:** [FR-DIST-01](../prd.md#fr-dist-01), [NFR-SEC-01](../prd.md#nfr-sec-01)

## Context

`npm-publish` published all eight npm packages with `NPM_TOKEN`, a token stored as a repository secret. npm limits a write token that bypasses 2FA to 90 days. Since 2026-07-31 such a token can no longer change trust settings or manage tokens, and from January 2027 it loses direct publishing: it can only stage a publish for a maintainer to approve with 2FA ([npm changelog, 2026-07-31](https://github.blog/changelog/2026-07-31-restricting-npm-bypass-2fa-granular-access-tokens/)). The `v0.1.0` tag's publish failed with `EOTP` because the token in `NPM_TOKEN` did not bypass 2FA at all ([plan 0001, 1G](../plans/implemented/0001-wave-1-typescript-parity.md#3-wave-based-delivery-plan)).

npm's trusted publishing replaces the token. A package's trust setting names a GitHub repository and workflow file, and npm 11.5.1 or later, running in that workflow, exchanges the job's OIDC token for a publish token for that package alone. The CLI manages the setting: `npm trust github`, `npm trust list`, `npm trust revoke`.

A trust setting can only be made for a package that already exists, and npm keeps one per package. Only `rulebearing` exists. Its trust setting was made on 2026-10-10: repository `benbahrenburg/rulebearing`, file `release.yml`, no environment. The six `rulebearing-cli-<platform>` packages and `eslint-plugin-rulebearing` are created by the first release. CLAUDE.md rules out publishing placeholders to create them earlier.

## Decision

**`npm-publish` installs npm 11.21.0 and keeps both credentials.** npm uses the OIDC exchange for every package with a matching trust setting. It falls back to `NODE_AUTH_TOKEN`, set from `NPM_TOKEN`, for a package with none, which is a package the release creates. `--provenance` stays and signs with the same OIDC token.

**After the first release, all eight packages get the trust setting, and the secret is deleted.** The maintainer runs `npm trust github` for the seven new names, as the loop in [docs/release.md § The npm wrapper](../release.md#the-npm-wrapper-from-wave-1) shows. Then `NPM_TOKEN` is deleted and the token revoked. From then on the workflow publishes with no stored npm credential. The fallback stays in the workflow for a package added later, whose first publish needs a token created for that package alone.

**A dry run proves the trust setting.** On a manual run, `npm-credentials` republishes the `rulebearing` 0.0.1 tarball with no token configured and provenance off. The registry answers "previously published" only if npm exchanged the OIDC token. Without a matching trust setting npm has no credential and fails with `ENEEDAUTH`, so the job fails and nothing new is published. When `NPM_TOKEN` is set, the job also checks it with `npm whoami`.

## Consequences

- Once the secret is deleted, there is no npm token to rotate or leak. Publishing is possible only from `release.yml` in this repository.
- `npm-credentials` holds `id-token: write`, as `npm-publish` already did. Both are publish jobs, limited to a manual run or a release tag (ADR-0025).
- The first release still depends on `NPM_TOKEN`, and that token must bypass 2FA, or the publish fails with `EOTP` as `v0.1.0`'s did. Such a token publishes only until January 2027. A release after that cannot create the seven packages from CI. They would be published once from the maintainer's own session, with 2FA, or staged by CI for the maintainer to approve, and then given trust settings.
- The dry run cannot prove that the token covers names that do not exist yet. Only the first release does.
- Renaming `release.yml` or moving the repository breaks publishing until every package's trust setting is replaced.

## Alternatives considered

- **Keep `NPM_TOKEN` for everything.** Rejected: 90-day rotation, and npm is restricting tokens that bypass 2FA for publishing.
- **Publish placeholders for the seven names first, then trust them.** Rejected: CLAUDE.md allows placeholders only in the wave 0E reservation task.
- **Require a GitHub environment in the trust setting.** Deferred, as for NuGet: the release already runs only from a `vX.Y.Z` tag after every install check passes.
