# ADR-0073: The extractors read files through `rb-model`'s `Files`, so the playground can extract from memory

- **Status:** Proposed (2026-10-11)
- **Date:** 2026-10-11
- **Derives from:** [plan 0004, Step 2](../plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables) ("an in-memory file system adapter for `rb-extract-ts` and `rb-extract-python` (both already take a `Fs` trait from wave 1 for testing; if they do not, add it here as the smallest change)") and the exports [§ 1.6](../plans/pending/0004-wave-4-reach.md#16-interfaces-and-contracts-this-wave-freezes) freezes; [ADR-0067](0067-the-playground-facade-and-an-optional-javascript-engine.md) (the facade)
- **Amends:** [ADR-0010](0010-crate-layout-and-extractor-boundary.md)'s crate table, as ADR-0067 does: `rb-wasm` may also depend on `rb-extract-ts` and `rb-extract-python`. ADR-0010's rules are unchanged: each extractor still depends on `rb-model` only.
- **Constrains:** `crates/rb-model` (`files`), `crates/rb-extract-ts`, `crates/rb-extract-python`, `crates/rb-rules` (`explain`, `plain`, `wants_metrics`), `crates/rb-config` (`options_used`), `crates/rb-wasm`, `crates/rb-cli/tests/crate_boundary.rs`
- **Requirements:** [FR-REACH-01](../prd.md#fr-reach-01), [FR-EXT-TS-01](../prd.md#fr-ext-ts-01), [FR-EXT-PY-01](../prd.md#fr-ext-py-01)

## Context

The playground's `cruise_ts(files, config, output_type)` and `cruise_py(...)` extract a dropped small repository in the browser. The page hands over a `Map` of paths to source text. A `wasm32-unknown-unknown` build has no file system: every `std::fs` call fails there.

The plan assumed both extractors already read through an `Fs` trait. Neither did. Both called `std::fs` directly:

- `rb-extract-ts`: the walk, the sources, `package.json` lookups, the Babel configuration, and `oxc_resolver`'s own `FileSystemOs`, which also reads the tsconfig;
- `rb-extract-python`: the layout discovery, the walk, the sources, and the `site-packages` index.

ADR-0010 lets an extractor depend on `rb-model` only, so a seam both extractors share can live nowhere else.

The plan also asks for `explain` in the playground, and for its output to equal the command line's. `explain` lived in `rb-cli`, which the facade never links (ADR-0067).

## Decision

**`rb-model` gains `files`: `Files`, `MemoryTree` and `lexical`.**

- `Files::disk()` is the default. It calls `std::fs` and nothing else, so a native run reads what it read before.
- `Files::memory(tree)` answers from a `MemoryTree`. The tree holds absolute paths, files only, every folder implied by the files below it, and no symbolic links. A path's canonical form is therefore its lexical one.
- The methods mirror `std::fs`: `read`, `read_to_string`, `metadata`, `symlink_metadata`, `read_dir`, `canonicalize`, `is_file`, `is_dir`.

**Every read an extractor makes goes through a `Files`.**

- `rb-extract-ts`: `Settings::files` and `ResolveConfig::files`, set together by `prepare_in(options, cwd, files)`. `prepare` is `prepare_in` over the disk. The resolver is `oxc_resolver` over `ResolverFiles`, which delegates to `FileSystemOs` for the disk and to the tree in memory otherwise.
- `rb-extract-python`: `Settings::files`, set by `prepare_in(files, base, options, virtual_env)`. `discover`, `walk`, `site::find` and `site::index` take the `Files` they read.

**Over files in memory, what needs the disk or a process is refused by name.** Nothing is quietly skipped:

- `--sidecar node` (the Node sidecar);
- `externalModuleResolutionStrategy: yarn-pnp`, whose manifest reader reads the disk itself;
- in the playground, a ratchet, whose budget is a file.

Each is an error that names the option and says to run the command line. A CoffeeScript or LiveScript file is refused as on the command line without `--sidecar`.

**What the playground shares with the command line moves into library crates.** `rb-cli` keeps a re-export or a thin call to each, so both print the same bytes:

| Code | Was in | Now in |
| --- | --- | --- |
| `explain`: finding the rule, `rule_json`, the text and JSON forms | `rb-cli` | `rb_rules::explain` |
| the `explain --plain` sentences | `rb-cli` | `rb_rules::plain` |
| `shouldCalculateMetrics` | `rb-cli` | `rb_rules::evaluate::wants_metrics` |
| `summary.optionsUsed` and `normalizeReporterOptions` | `rb-cli` | `rb_config::options_used` |

Neither crate's dependency row changes, and neither gains a `match language`.

**`rb-wasm` joins `rb-extract-ts` and `rb-extract-python` in its row.** It exports all five functions of § 1.6. The files the page gives sit under `/playground`, the run's working directory. A path that climbs out of it is refused.

## Consequences

- Native behaviour is unchanged, and the existing fixtures prove it:
  - gate 1 layer 1 (`extract_fixtures`) and every option fixture of `rb-extract-ts`;
  - the Python fixture;
  - `rb-cli`'s committed `explain` and MCP outputs.
- Every `rb-extract-ts` option fixture is also extracted from a copy of its files held in memory at the same paths, and must give the disk's extraction byte for byte, or the named refusal. The Python fixture is held to the same.
- The playground's fixtures under `crates/rb-wasm/tests/fixtures/` hold what the command line prints for 24 reporters and for `explain`. `rb-cli`'s `tests/playground_fixtures.rs` holds the command line to those bytes. `rb-wasm`'s native tests and `tests/wasm.mjs`, the built module under Node, hold the playground to the same bytes.
- Not compared, because the outputs differ by design:
  - `optionsUsed.baseDir` and `rulesFile`, which name the command line's own folder and file;
  - `sarif` and `github-annotations`, which put the folder's path from the repository root before each path;
  - `anon`, whose strings are drawn from a hash of the whole document;
  - `x-dot-webpage`, which runs GraphViz.
- The extractors' warnings go to standard error on the command line. The playground returns one string, so it does not show them.
- A later extractor that reads files reads them through `Files` too, or it cannot run in the playground.

## Alternatives considered

| Option | Why not |
| --- | --- |
| A `trait Fs` per extractor, with generic settings | Two copies of one seam, and a type parameter through every function of both pipelines, for the same two implementations. |
| A new crate for the seam | Changes every extractor's row in ADR-0010's table, which says an extractor depends on `rb-model` only. |
| WASI, with the files written to a virtual disk | Needs a WASI runtime in the page and a different target from the one ADR-0067 builds. The extractors' `std::fs` calls would still be the ones under test, not a seam the native tests can reach. |
| Leave `explain` in `rb-cli` and reimplement it in `rb-wasm` | Two copies of one output that are meant to stay byte-identical. |
