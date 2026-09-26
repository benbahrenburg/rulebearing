# ADR-0039: The delivery plans and the exported design are kept local, not published

- **Status:** Accepted (2026-09-26, by the maintainer)
- **Date:** 2026-09-26
- **Derives from:** [ADR-0023](0023-documentation-link-and-lint-gates.md) (the documentation link gate), [ADR-0001](0001-record-architecture-decisions.md) ("link everything")
- **Constrains:** `.gitignore`, `xtask/src/doclinks.rs`, `crates/rb-model/build.rs`, `crates/rb-config/src/elements.rs` (the coverage-tab test)
- **Requirements:** [NFR-DOC-01](../prd.md#nfr-doc-01)

## Context

`docs/plans/` holds the step-by-step delivery plans and `docs/artifacts/` holds the source design and the coverage tabs. The maintainer keeps both as working context and does not want them published in the public repository. Source files, ADRs and the other documents link into both directories hundreds of times, and ADR-0023 makes a broken link fail `cargo build`, `cargo test`, `cargo clippy` and the CI `lint` job. A checkout without the two directories would fail to build.

## Decision

- `/docs/plans/` and `/docs/artifacts/` are listed in `.gitignore` and are not tracked. They exist only on the maintainer's machine.
- The link checker names them in `LOCAL_ONLY_ROOTS`. On a developer's machine every link into them is checked, anchor included, exactly as before; if the directories are missing there, the links are reported broken rather than skipped. On a CI server, identified by the `CI` environment variable being set and not `false` or `0`, links into them are skipped and every other link is still checked.
- Links into the two directories stay in the source. They are the local trail from code to plan, and removing them would lose that trail for the maintainer.
- A test that reads a local-only file returns early with a message when the file is absent, and otherwise runs in full.

## Consequences

- On GitHub, links into `docs/plans/` and `docs/artifacts/` do not resolve. File names and heading anchors in those links remain visible; the documents' contents do not.
- CI no longer proves that plan and design links resolve. That is proven by `cargo build` and `cargo xtask check-links` on the maintainer's machine.
- A git worktree does not carry ignored files. A local build in a worktree fails the link check until the two directories are copied or symlinked into it.
- `scripts/gate2-ratchet.sh` finds no `docs/plans/implemented/0002-*.md` on CI, so it stops enforcing the "no `not-yet` entry after wave 2" rule there. Run it locally once the plan moves.
- Removing the directories from the repository's history, and from GitHub's copies of it, is a separate operation this decision does not perform.

## Alternatives considered

- **Delete every link into the two directories.** About 1,100 links in about 490 files, and the local trail from code to plan would be gone. Rejected.
- **Skip the link check entirely on CI.** Published documents would lose their only server-side gate. Rejected.
- **Skip links whenever the target directory is absent.** A local worktree without the plans would pass silently. Rejected in favour of keying the skip on `CI`.
- **Keep the plans in a private repository added as a submodule.** A viable later step if the plans need sharing with collaborators; it does not change the checker's rule.
