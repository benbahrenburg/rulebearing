# ADR-0023: Documentation links are checked on every compile, and every language has a configured linter behind one entry point

- **Status:** Accepted
- **Date:** 2026-09-21
- **Derives from:** [ADR-0001](0001-record-architecture-decisions.md) ("link everything"), [ADR-0018](0018-test-coverage-threshold.md), [design § What Rust does not solve](../artifacts/design.md#what-rust-does-not-solve) (the suites, not the maintainer, are the reviewer), [design § Docs derived from the rules](../artifacts/design.md#docs-derived-from-the-rules-never-written-beside-them)
- **Constrains:** [architecture.md § Verification strategy](../architecture.md#verification-strategy), [§ Repository layout](../architecture.md#repository-layout), [CLAUDE.md](../../CLAUDE.md), `.github/workflows/ci.yml`
- **Implemented by:** [Wave 0 plan](../plans/pending/0000-wave-0-spike.md), sub-wave 0A
- **Requirements:** [NFR-DOC-01](../prd.md#nfr-doc-01), [NFR-QUAL-02](../prd.md#nfr-qual-02)

## Context

Two rules were written down but only weakly enforced.

**Linked references.** [ADR-0001](0001-record-architecture-decisions.md) requires every source file, plan and ADR to link the architecture section, the ADRs, the plan and the requirement it serves. A shell script checked that the file at the end of a link existed. It did not check anchors, did not run on Windows, and ran only in its own CI job, so a rename could sit broken in a working tree through any number of builds. This repository has more than 1,800 relative links across five plans, 23 decision records, a PRD and the crate doc comments; at that size a link is a dependency, and an unchecked dependency rots.

**Linters.** The working agreement names conventions for Rust, TypeScript, C# and Python, but only Rust had configuration on disk. The other three would have been enforced by memory at review time, which is the failure mode the whole project exists to remove for architecture rules.

## Decision

**1. The documentation link check is a compile-time gate.** It lives in `xtask/src/doclinks.rs` and runs from three places: `crates/rb-model/build.rs` (every crate depends on `rb-model`, so every `cargo build`, `cargo test` and `cargo clippy` runs it once), `cargo xtask lint`, and the `docs-links` CI job. It checks that a relative target exists and, when the link carries an anchor, that the target holds a heading with that slug, using GitHub's slug rules. It skips absolute URLs, fenced code blocks (examples, not references), the `file/<tab-id>` tab links inside the verbatim export in `docs/artifacts/`, and, in Rust sources, everything that is not a `//!` or `///` line. `RB_SKIP_DOC_LINK_CHECK=1` bypasses it locally; nothing in CI sets it.

**2. One lint entry point covers all four languages.** `cargo xtask lint` runs, in order: the link check, `cargo fmt --check`, `cargo clippy -D warnings`, eslint and prettier, ruff (lint and format) and mypy, and `dotnet format --verify-no-changes`. `--fix` applies what each linter can fix. A language whose tree does not exist yet reports **not applicable** rather than passing, so the first file of that language brings its linter with it. A linter that is not installed reports **skipped** locally and **fails** under `--strict`, which is what CI passes.

**3. Each language's configuration is on disk, at the repository root.**

| Language | Tools | Configuration |
| --- | --- | --- |
| Rust | rustfmt, clippy (pedantic, `unwrap`/`expect`/`panic` denied), `cargo deny` | `rustfmt.toml`, `[workspace.lints]` in `Cargo.toml`, `deny.toml` |
| TypeScript, JavaScript | eslint (flat config, `typescript-eslint` strict and stylistic type-checked), prettier | `eslint.config.mjs`, `.prettierrc.json`, `.prettierignore`, `tsconfig.base.json`, `package.json` |
| Python | ruff (`select = ["ALL"]` with four documented ignores), ruff format, mypy strict | `pyproject.toml` |
| C# | .NET analyzers at `latest-recommended`, warnings as errors, `dotnet format`, style severities | `Directory.Build.props`, `.editorconfig` |
| Documentation | the link check above | `xtask/` |

**4. `xtask` is a workspace member outside `crates/`.** It carries no dependencies beyond the standard library, so compiling it as a build dependency of `rb-model` costs almost nothing and adds no licence surface. It is not an `rb-*` crate and is not part of the product; [ADR-0010](0010-crate-layout-and-extractor-boundary.md)'s crate list is unchanged, in the same way `wrappers/`, `adapters/` and `frontends/` sit outside it. It carries the same 70% coverage floor as every other crate ([ADR-0018](0018-test-coverage-threshold.md)).

## Consequences

- A broken link fails `cargo build`, not only a CI job, so it is found in the editor rather than in review. The cost is one directory scan per compile, guarded by `rerun-if-changed` on the documentation trees.
- The check is Rust, so it runs identically on the Linux, macOS and Windows test jobs. `scripts/check-links.sh` stays as a one-line wrapper.
- Adding a language means adding its linter to `cargo xtask lint` and its configuration file, not adding a step to a reviewer's memory.
- A document may not be moved without fixing the links into it. That is the intent: the plans, the PRD and the ADRs are addressed by path throughout.
- The `not applicable` state is load-bearing. It must turn into `pass` the day the first TypeScript, Python or C# file lands, and the wave plan that lands it says so.

## Alternatives considered

- **A Markdown link checker from npm (`markdown-link-check`, `lychee`).** Rejected: it would put Node or a second binary in front of `cargo build`, none of them reads Rust doc comments, and the repository's own promise is a single static binary with no runtime.
- **Leaving the check in CI only.** Rejected: the gap between writing a link and learning it is broken is the whole problem, and CI feedback arrives after the pull request is open.
- **A git pre-commit hook.** Kept as an option for later; hooks are opt-in per clone and are skipped with `--no-verify`, so they cannot be the gate.
- **Linting Markdown prose (`markdownlint`).** Not adopted. The style rules in CLAUDE.md (no em-dashes, tables for parallel facts) are editorial rather than mechanical, and a prose linter over 6,000 lines of committed documentation would produce noise without catching the failure that matters, which is a broken reference.
