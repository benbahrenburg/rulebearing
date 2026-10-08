# ADR-0052: `--affected` follows the configuration format: dependency-cruiser's `reaches` filter, or the closure

- **Status:** Accepted (2026-10-08, by the owner)
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

The semantics follow the configuration format, as liveness does ([ADR-0032](0032-liveness-follows-the-configuration-format.md)): a dependency-cruiser configuration, or none, gets dependency-cruiser's behaviour; a `rulebearing.*` configuration, which has no upstream to match, gets the closure the plan describes.

### With a dependency-cruiser configuration

- `--affected [revision]` does what dependency-cruiser does, with the same `git` commands, the same line parsing and the same escaping, so `summary.optionsUsed.reaches`, the modules, the edges, the violations and the exit code of a TypeScript cruise are dependency-cruiser's. Points 1 to 3 and 5 hold here as they do upstream. Layer 5's `zero-diff.mjs` over the scratch repository above finds no difference.
- A gating reporter's error count is the filtered report's (point 6), for `reaches` as for `--affected`, as ADR-0030's "the reporter's exit code" already implies. Before this, `cruise --reaches` exited with the whole graph's count.
- Three additions, none of which changes a TypeScript cruise:
  - **Other languages.** A changed Python or .NET module, and a changed file the PDB attributes a .NET type to (the type's primary file or any other file of a partial type), add their modules to the expression, fully escaped, after upstream's names. A graph from the C# fallback carries the same fields.
  - **Depth.** `--affected-depth N` keeps the modules that reach a changed one in at most `N` steps, measured by the shortest path (a breadth-first walk; upstream's depth-first walk, which `reaches` uses unbounded, can miss a module first found by a longer path). It is never written to `optionsUsed`, whose `reaches` upstream's schema closes.
  - **Receipt.** `summary.affected: { revision, changed, closure, depth? }`, stripped by `--strict-schema`.
- git runs with `core.quotePath=false` and `diff --no-relative`, the revision must name a commit (`rev-parse --verify <revision>^{commit}`), and `--` ends the revisions. For an ASCII path in a repository without `diff.relative` nothing changes; a non-ASCII path, which upstream receives quoted and so never matches, counts.
- One divergence: paths are relative to the cruise's base directory, as module names are (point 4). Upstream's behaviour there is a silent empty cruise, the failure mode ADR-0008 exists to remove.
### With a `rulebearing.*` configuration

- The rules are evaluated over the whole graph, so cycles, reachability, dependents and instability are what a full cruise finds. The report keeps the closure (the changed modules and the modules that reach them, to `--affected-depth`) with every edge its modules have, and the violations that touch it. Point 1 does not hold: the edited file's import of an unchanged, forbidden module is reported.
- A violation touches the closure when its `from` module is in it (dependency, instability and module-level violations: orphans, `required`, `numberOfDependentsLessThan`); for a cycle or reachability violation, when any module of its path or its `to` is (with a depth, its `from` module then joins the report); for a folder violation, when a closure module sits in the folder; for an element violation, when an end names a closure module or a type declared in a closure file; for a slice violation, whose ends are slice names, when an end of one of its member edges (`via`) does.
- Every changed file that is a module counts, whatever its extension (Python and .NET included), with the PDB mapping. Point 3 does not hold: the changes are listed with `git status --porcelain --untracked-files=all`. For point 2, the importers of a deleted file, and of the old name of a renamed one, are read from the saved graph (`.graph/cruise.json`) when it exists, since the current graph no longer names them; an unreadable saved graph exits 2. The modules a changed file depended on in the saved graph join the closure, without their dependents, so an edit that leaves a module an orphan or unreachable reports that module's violation.
- `reaches` is not set, so `optionsUsed` carries no expression; the receipt records the closure.

### Both

- `options.affected` applies in a `rulebearing.*` configuration as the flag would, and is ignored with a warning in a dependency-cruiser configuration, as dependency-cruiser ignores it. The flag wins over both.
- A revision git does not know, a directory outside a repository, a missing `git`, or a revision that begins with `-` (which git would read as an option) exits 2 with the reason named.

## Consequences

- A pipeline that switches from `depcruise --affected` to `rulebearing cruise --affected` gets the same report.
- The Stop hook recipe `cruise --affected HEAD --output-type agent` reports an agent's new forbidden import from an edited file in a repository with a `rulebearing.*` configuration. With a dependency-cruiser configuration it inherits points 1 and 3, as `depcruise --affected` does; the full cruise in CI still reports both.
- The same command gives different reports under the two formats, as liveness already does; `summary.affected` and the absence of `optionsUsed.reaches` say which ran.
- `cruise --reaches` with a gating reporter now exits with the filtered count, as dependency-cruiser does.

## Alternatives considered

- **The plan's closure for every configuration.** Rejected: the report, `optionsUsed` and the exit code would differ from dependency-cruiser's on every repository where an edited file imports an unchanged module, and parity is the bar for a dependency-cruiser configuration.
- **Upstream's filter for every configuration.** Rejected: the Stop hook, the reason the feature exists, would miss the most common violation an agent introduces.
- **`git diff <revision>...HEAD` (the merge base), as the plan's step sketches.** Rejected: watskeburt compares the revision itself with the working tree, and a different base changes which files count.
- **Expand untracked folders under a dependency-cruiser configuration too.** Rejected for the same reason: it changes the expression and so `optionsUsed.reaches`.
- **Keep upstream's repository-relative paths.** Rejected: it reproduces a silent empty report.
