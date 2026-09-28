# ADR-0052: `--affected` is dependency-cruiser's `reaches` filter, plus other languages and a depth

- **Status:** Proposed
- **Date:** 2026-09-27
- **Derives from:** [dependency-cruiser coverage § Options](../artifacts/dependency-cruiser-18.2.0-coverage.md#options) (row `affected`, Parity+), [§ Command line](../artifacts/dependency-cruiser-18.2.0-coverage.md#command-line) (row `--affected [revision]`), [ADR-0004](0004-graph-document-is-cruise-result-superset.md) (the receipt is additive), [ADR-0030](0030-the-reporter-decides-the-error-count-exit.md) (the reporter's exit code), [ADR-0032](0032-liveness-follows-the-configuration-format.md) (a behaviour may follow the configuration format)
- **Constrains:** `crates/rb-cli/src/affected.rs`, `crates/rb-cli/src/cmd/cruise.rs`, `crates/rb-cli/src/pipeline.rs`, `crates/rb-rules/src/graph/filters.rs` (`reaches` with a depth), `crates/rb-config/src/model.rs` (`options.affected`), `crates/rb-model/src/document.rs` (`summary.affected`)
- **Implemented by:** [Wave 3 plan](../plans/pending/0003-wave-3-operations-surface-inner-loop.md), [Step 3](../plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)
- **Requirements:** [FR-CLI-05](../prd.md#fr-cli-05), [NFR-CONF-01](../prd.md#nfr-conf-01)

## Context

The plan describes `--affected` as "the changed files and their dependents to depth N, then evaluate the rules over the closure". dependency-cruiser 18.2.0 does something narrower. Its `normalizeOptions` asks watskeburt 6.0.0 for the changed files (`git diff <revision> --name-status`, plus the untracked files of `git status --porcelain`), turns the ones with its seventeen extensions into a regular expression, and sets `reaches` to it. `filterReaches` then keeps the matching modules and every module that reaches them, and only the edges between kept modules; the summary is recomputed from what is kept.

Measured on 2026-09-27 on a scratch repository with the pinned upstream binary (one modified file that imports a forbidden module and closes a cycle, a staged new file, an untracked file, a deleted file, a staged rename, an untouched violation):

1. The forbidden edge from the modified file to an unchanged module is not reported, because the unchanged module does not reach a changed one.
2. The deleted file is not in the expression; renamed and copied files count by their new name.
3. A new file inside a new, untracked folder is not seen: `git status --porcelain` lists the folder, which has no extension.
4. Run from a subdirectory, the expression holds repository-relative paths while module names are relative to the subdirectory, so nothing matches and the report is empty, exit 0.
5. `affected` in a configuration file is ignored ("a command line only option"), and `affected` is not in `optionsUsed`.
6. The `err` reporter exits with the error count of the filtered report (4), not of the whole graph (6).

## Decision

- `--affected [revision]` does what dependency-cruiser does, with the same `git` commands, the same line parsing and the same escaping, so `summary.optionsUsed.reaches`, the modules, the edges, the violations and the exit code of a TypeScript cruise are dependency-cruiser's. Points 1 to 3 and 5 hold here as they do upstream. Layer 5's `zero-diff.mjs` over the scratch repository above finds no difference.
- A gating reporter's error count is the filtered report's (point 6), for `reaches` as for `--affected`, as ADR-0030's "the reporter's exit code" already implies. Before this, `cruise --reaches` exited with the whole graph's count.
- Three additions, none of which changes a TypeScript cruise:
  - **Other languages.** A changed Python or .NET module, and a changed file the PDB attributes a .NET type to (the type's primary file or any other file of a partial type), add their modules to the expression, fully escaped, after upstream's names. A graph from the C# fallback carries the same fields.
  - **Depth.** `--affected-depth N` keeps the modules that reach a changed one in at most `N` steps. It is never written to `optionsUsed`, whose `reaches` upstream's schema closes.
  - **Receipt.** `summary.affected: { revision, changed, closure, depth? }`, stripped by `--strict-schema`.
- One divergence: paths are relative to the cruise's base directory, as module names are (point 4). Upstream's behaviour there is a silent empty cruise, the failure mode ADR-0008 exists to remove.
- `options.affected` applies in a `rulebearing.*` configuration as the flag would, and is ignored with a warning in a dependency-cruiser configuration, as dependency-cruiser ignores it. The flag wins over both.
- A revision git does not know, a directory outside a repository, a missing `git`, or a revision that begins with `-` (which git would read as an option) exits 2 with the reason named.

## Consequences

- A pipeline that switches from `depcruise --affected` to `rulebearing cruise --affected` gets the same report.
- The Stop hook recipe `cruise --affected HEAD --output-type agent` inherits points 1 and 3: an agent's new import from an edited file to an unchanged, forbidden module, and a file in a new folder that is not yet staged, are not reported by the affected run. The full cruise in CI still reports both. If the hook needs them, a closure mode that keeps every edge leaving a changed module is a further decision, recorded as its own ADR, and cannot be the default for a dependency-cruiser configuration without breaking parity.
- `cruise --reaches` with a gating reporter now exits with the filtered count, as dependency-cruiser does.

## Alternatives considered

- **The plan's closure: every violation whose `from` is in the closure, edges leaving it kept.** Rejected as the default: the report, `optionsUsed` and the exit code would differ from dependency-cruiser's on every repository where an edited file imports an unchanged module, and parity with dependency-cruiser is the bar for a TypeScript cruise.
- **`git diff <revision>...HEAD` (the merge base), as the plan's step sketches.** Rejected: watskeburt compares the revision itself with the working tree, and a different base changes which files count.
- **Expand untracked folders (`git status --untracked-files=all`).** Rejected for the same reason: it changes the expression and so `optionsUsed.reaches`.
- **Keep upstream's repository-relative paths.** Rejected: it reproduces a silent empty report.
