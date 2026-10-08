# ADR-0058: The edit-compile cycle rebuilds only what changed: the link check moves to a crate nothing depends on, dev builds carry line tables, `sccache` is per machine

- **Status:** Accepted (2026-10-08, by the owner)
- **Date:** 2026-10-01
- **Derives from:** [ADR-0023](0023-documentation-link-and-lint-gates.md) (the link check is a compile-time gate), [ADR-0024](0024-test-quality-gates.md) (mutation testing rebuilds once per mutant), [ADR-0010](0010-crate-layout-and-extractor-boundary.md) (every crate depends on `rb-model`), [architecture § Verification strategy](../architecture.md#verification-strategy)
- **Supersedes:** the clause of [ADR-0023](0023-documentation-link-and-lint-gates.md) decision 1 that runs the check from `crates/rb-model/build.rs`, and the clause of its decision 4 that `xtask` is compiled as a build dependency of `rb-model`. The check itself, what it checks, `cargo xtask lint`, the `docs-links` job and `RB_SKIP_DOC_LINK_CHECK` stand.
- **Constrains:** `xtask/gate/` (new), `crates/rb-model/Cargo.toml`, `Cargo.toml` (`[workspace] members`, `[profile.dev]`), `.cargo/mutants.toml`, [CLAUDE.md](../../CLAUDE.md)
- **Implemented by:** the change that adds this record
- **Requirements:** [NFR-DOC-01](../prd.md#nfr-doc-01), [NFR-QUAL-02](../prd.md#nfr-qual-02)

## Context

[ADR-0023](0023-documentation-link-and-lint-gates.md) put the link check in `rb-model`'s build script so that every compile runs it: every crate depends on `rb-model`. To see doc comments change, the script asks cargo to re-run it whenever anything under `crates/`, `xtask/`, `fuzz/` or the documentation roots changes. Cargo recompiles a package whenever its build script re-runs, and recompiles every package above one it recompiled. So any edit to any source file, or to any document, recompiled `rb-model` and with it all ten crates.

Measured on 2026-10-01 on the wave 3 branch (12-core Apple silicon, warm `target/`): touching one file in `rb-cli` and running `cargo check --workspace --all-targets` re-checked all ten crates in 155 s. The check itself costs 0.2 s of CPU (`cargo check-links`: 5,216 links in 377 files).

The cost repeats wherever a file changes:

| Where | Effect |
| --- | --- |
| An agent's edit, then `check`, `clippy` or `test` | the whole workspace rebuilds, not the crate edited |
| Each mutant of [ADR-0024](0024-test-quality-gates.md)'s gate | `cargo mutants` edits a file under `crates/`, so each mutant also rebuilds `rb-model` and every crate between it and the one mutated |
| A status line in a plan, an ADR, a README | the whole workspace rebuilds on the next compile |

Two further costs fall on the parallel worktrees a wave runs in. Each has its own `target/`, built from cold: fifteen of them held about 230 GB on 2026-10-01, between 0.9 and 21 GB each. Most of that is full debug info, which is also most of what the linker writes for each of the 94 integration-test binaries.

## Decision

**1. The link check runs from `xtask/gate/build.rs`, in a crate named `xtask-gate` that nothing depends on.** The build script moves from `crates/rb-model/build.rs` unchanged apart from its header, and `rb-model` loses its build dependency on `xtask`. `xtask-gate` is a workspace member whose library exports nothing: cargo runs a build script only for a package with a target, and the crate's behaviour is the build script. A re-run now recompiles `xtask-gate` and nothing else.

The check still runs inside every workspace-wide `cargo build`, `cargo test` and `cargo clippy`, including `cargo build --release` at the root, in `cargo xtask lint`, and in the `docs-links` job. What changes is that a command scoped to one package, `cargo test -p rb-rules` say, no longer runs it. That is the point: the per-package command is the inner loop, and the workspace command, the lint and the job remain the gate.

**2. Dev and test builds carry line tables only.** `[profile.dev] debug = "line-tables-only"`. A panic's backtrace still names file and line. A debugger session that needs locals sets `CARGO_PROFILE_DEV_DEBUG=full` for that build. The release profile is unchanged.

**3. `sccache` is set per machine, not in the repository.** A machine that has it sets `RUSTC_WRAPPER=sccache` (or `build.rustc-wrapper` in `~/.cargo/config.toml`), and every worktree on that machine then compiles each third-party crate once rather than once per `target/`. The repository's `.cargo/config.toml` does not name it, because a wrapper that is not installed fails every build. CI keeps `Swatinem/rust-cache`.

## Consequences

One edit now rebuilds the crate edited and the crates above it, and no others. Measured after the change on the same branch and machine:

| Edit, then `cargo check --workspace --all-targets` | Before | After |
| --- | --- | --- |
| one file in `rb-cli` | 155 s, ten crates | 9.9 s, `rb-cli` |
| one file in `rb-rules` | 155 s, ten crates | 78 s, `rb-rules`, `rb-report`, `rb-cli`, `rb-node` |
| one file in `rb-model` | 155 s, ten crates | 121 s, ten crates (they all depend on it) |
| one Markdown file | ten crates (not timed) | 8.9 s, `xtask-gate` |

- Each mutant rebuilds the crate it mutates and nothing below it; `rb-model` is no longer rebuilt for a mutant of `rb-rules`.
- Editing a document rebuilds `xtask-gate` alone.
- `cargo build -p rb-cli` and `cargo test -p <crate>` do not check links. The `docs-links` job and the workspace commands catch a broken link before merge, as [ADR-0023](0023-documentation-link-and-lint-gates.md) already relied on for anything not compiled.
- `xtask-gate` has no lines to cover. It is outside `crates/` and is not a directory `scripts/coverage-per-crate.sh` walks, in the same way [ADR-0023](0023-documentation-link-and-lint-gates.md) decision 4 places `xtask` outside [ADR-0010](0010-crate-layout-and-extractor-boundary.md)'s crate list. The code the build script runs is `xtask::doclinks`, which carries its coverage and its mutants in `xtask`.
- `.cargo/mutants.toml`'s build-script exclusion names the new path.
- Backtraces lose inlined-frame detail and debuggers lose locals by default; the override above restores both for one build.

## Alternatives considered

- **Keep the script in `rb-model` and watch only the documentation roots.** Rejected: links in doc comments would no longer be checked by a compile, which is half of what [ADR-0023](0023-documentation-link-and-lint-gates.md) gates.
- **Run the check from `rb-cli`'s build script.** Rejected: `rb-cli` is the largest crate, and every document edit would recompile it.
- **Run the check from `xtask`'s own build script.** Rejected: a broken link would then stop `cargo xtask check-links` and `cargo xtask lint --fix` from compiling, which are the commands that report and repair it.
- **One shared `CARGO_TARGET_DIR` for every worktree.** Rejected: cargo's lock on the directory serialises the worktrees' builds, which defeats running them in parallel, and two branches rebuilding the same crate evict each other's output.
- **`rustc-wrapper = "sccache"` in the repository's `.cargo/config.toml`.** Rejected: it makes `sccache` a requirement for every contributor and every CI job.
