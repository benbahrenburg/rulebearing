# ADR-0025: Continuous integration runs least-privileged, pinned and bounded, and the dependency supply chain is closed

- **Status:** Accepted
- **Date:** 2026-09-21
- **Derives from:** [ADR-0019](0019-mit-licence.md) (licence policy), [ADR-0010](0010-crate-layout-and-extractor-boundary.md) (feature-gated extractors), [ADR-0002](0002-rust-as-implementation-language.md) (`rust-version`), [design § Rules an agent writes, held to the same bar](../artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar) (hermetic runs), [architecture § Security posture](../architecture.md#security-posture)
- **Constrains:** `.github/workflows/*.yml`, `deny.toml`, `.github/dependabot.yml`, `.githooks/`, [CLAUDE.md](../../CLAUDE.md)
- **Implemented by:** [Wave 0 plan](../plans/pending/0000-wave-0-spike.md), sub-wave 0A
- **Requirements:** [NFR-QUAL-02](../prd.md#nfr-qual-02), [NFR-SEC-01](../prd.md#nfr-sec-01), [NFR-COMPAT-01](../prd.md#nfr-compat-01)

## Context

The project ships a binary that other people will run in their pipelines, built by workflows that run on every push. Three things were unhardened: the workflows themselves (default write token, floating action tags, no timeouts), the dependency sources (licences and advisories were checked, registries were not), and two promises that nothing verified, namely the declared minimum Rust version and the claim that a narrower build never links the other extractors.

## Decision

**1. Workflows are least-privileged, pinned and bounded.**

| Control | Setting |
| --- | --- |
| Token | `permissions: contents: read` at the top of every workflow; the release workflow raises it for the jobs that publish, and nowhere else |
| Actions | Pinned to a commit SHA with the tag in a trailing comment; bumped by Dependabot, never by editing the tag |
| Time | `timeout-minutes` on every job, sized to the job |
| Duplication | A `concurrency` group per workflow and ref, cancelling superseded runs |
| Shell | Workflows are linted by `actionlint` and every committed script by `shellcheck` |

**2. The dependency supply chain is closed.** `cargo deny check licenses advisories bans sources` runs as a required check. `[sources]` denies unknown registries and all git dependencies, allowing only crates.io. A git or private-registry dependency needs an ADR first. Dependabot watches Cargo, npm and the Actions themselves, weekly and grouped; conformance pins are deliberate and excluded, because the upstream version is the specification ([ADR-0009](0009-conformance-suites-as-specification.md)).

**3. Both untested promises are now tested.** A `msrv` job builds the workspace on the exact toolchain named by `rust-version`, and fails if the two disagree. A `features` job runs `cargo hack check --feature-powerset` over `rb-cli`, so every combination of `extract-ts`, `extract-dotnet` and `extract-python` is built rather than one.

**4. Spelling and prose mechanics are checked.** `typos` runs over the repository, excluding the verbatim export in `docs/artifacts/` and vendored fixtures, with project vocabulary in `.typos.toml`. This is deliberately narrow: it catches a misspelled identifier or heading, and it is not a prose linter ([ADR-0023](0023-documentation-link-and-lint-gates.md) explains why there is none).

**5. Hooks are a convenience, never the gate.** `.githooks/pre-commit` runs formatting and the link check; `.githooks/pre-push` runs `cargo lint` and the test suite. They are opt-in with `git config core.hooksPath .githooks` and skippable with `--no-verify`, which is exactly why every one of those checks is also required in CI.

## Consequences

- A pull request cannot silently gain a write token, an unpinned action or an unbounded job, because the file that would grant it is reviewed and linted.
- Bumping an action is a Dependabot pull request with a SHA diff, which is a visible supply-chain event rather than an invisible one.
- The MSRV becomes a maintained promise: raising it is a deliberate edit to `Cargo.toml` and to the job that checks it.
- The feature powerset job grows with the feature set. If it becomes slow, it moves to the nightly workflow rather than being deleted.

## Alternatives considered

- **`cargo vet` or `cargo crev` for dependency review.** Rejected for now: heavyweight for a handful of direct dependencies. Revisit if the dependency tree grows or the project gains contributors.
- **Release provenance, signing and an SBOM.** Deferred to the first published release, where they belong; wave 0 publishes placeholders only ([ADR-0020](0020-single-name-across-registries.md)).
- **Requiring the hooks.** Not possible and not desirable. Hooks can always be skipped, so treating them as a gate would create a false sense of enforcement.
