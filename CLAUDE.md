# CLAUDE.md

Working agreement for anyone, human or agent, changing this repository. Read this first, then the document the task points at. Nothing here restates what the code or the docs already say; it says where to look and what the bar is.

## What this project is

Rulebearing is one static binary and one rule language that does what dependency-cruiser does for TypeScript and JavaScript, what ArchUnitNET does for .NET, and what import-linter does for Python, over a single graph. The objective, the promise ("superset, precisely; nothing is dropped") and the reasoning are in the source design at [docs/artifacts/design.md](docs/artifacts/design.md). The requirements are in [docs/prd.md](docs/prd.md), the target architecture in [docs/architecture.md](docs/architecture.md), every decision in [docs/adr/](docs/adr/README.md), and the delivery plans in [docs/plans/](docs/plans/README.md).

The primary language is Rust (a Cargo workspace under `crates/`). TypeScript, C# and Python appear in the wrappers, adapters and front-ends under `wrappers/`, `adapters/` and `frontends/`, and as the languages the tool analyses.

## Where things are

| Path | Purpose | Governing document |
| --- | --- | --- |
| `crates/rb-model` | The graph document. Depends on nothing. | [ADR-0004](docs/adr/0004-graph-document-is-cruise-result-superset.md), [ADR-0010](docs/adr/0010-crate-layout-and-extractor-boundary.md) |
| `crates/rb-config`, `rb-rules`, `rb-report`, `rb-ingest`, `rb-cli`, `rb-node` | Config, engine, reporters, ingest, CLI, Node binding | [architecture § Crate layout](docs/architecture.md#crate-layout) |
| `crates/rb-extract-ts`, `rb-extract-dotnet`, `rb-extract-python` | The only crates that read files other than the config | [ADR-0010](docs/adr/0010-crate-layout-and-extractor-boundary.md), [ADR-0011](docs/adr/0011-read-dotnet-assemblies-not-source.md), [ADR-0012](docs/adr/0012-oxc-for-typescript.md), [ADR-0013](docs/adr/0013-ruff-parser-for-python.md) |
| `conformance/` | The two conformance gates; the upstream test suites are the specification | [ADR-0009](docs/adr/0009-conformance-suites-as-specification.md) |
| `testbeds/` | Pinned open-source repositories for the nightly proof | [design § Test beds](docs/artifacts/design.md#test-beds-open-source-repositories-to-validate-against) |
| `docs/adr/` | Decisions, numbered. Rules cite them as `adr:NNNN`. | [ADR-0001](docs/adr/0001-record-architecture-decisions.md) |
| `docs/plans/pending/` and `implemented/` | One plan per wave; moved when its exit criteria are met | [docs/plans/README.md](docs/plans/README.md) |
| `docs/artifacts/` | The exported design and coverage tabs. Read-only. | [docs/artifacts/README.md](docs/artifacts/README.md) |
| `rulebearing.yaml` | This repository's own rules, enforced from wave 1 | [ADR-0010](docs/adr/0010-crate-layout-and-extractor-boundary.md) |
| `xtask/` | The documentation link check and the one lint entry point for all four languages | [ADR-0023](docs/adr/0023-documentation-link-and-lint-gates.md) |
| `.githooks/`, `.cargo/` | Opt-in git hooks; the cargo aliases and the mutation-testing scope | [ADR-0025](docs/adr/0025-ci-and-supply-chain-hardening.md), [ADR-0024](docs/adr/0024-test-quality-gates.md) |
| `fuzz/` | cargo-fuzz targets, a workspace of its own; run nightly | [fuzz/README.md](fuzz/README.md) |
| `wrappers/` | The npm, PyPI and NuGet wrappers (the 0.0.1 name reservations until waves 1 and 2), and the crates.io reservation | [docs/release.md](docs/release.md), [ADR-0020](docs/adr/0020-single-name-across-registries.md) |

## How work is organised

1. **Find the plan.** Every feature belongs to a sub-wave of one plan under `docs/plans/pending/`. Do not build something no plan names; if the design calls for it and no plan does, add it to the plan first in a separate change.
2. **Read the ADRs the plan applies.** An ADR is not edited after acceptance; a reversal is a new ADR that supersedes it. The wave 0 .NET decision ([ADR-0003](docs/adr/0003-dotnet-extractor-fallback.md)) is the model: a measured trigger, a decision rule fixed in advance, an outcome recorded as a new ADR.
3. **Link everything.** Every source file's module doc, every plan and every ADR carries links to the architecture section, the ADRs, the plan and the PRD requirement it serves. The check is not advisory: it runs inside `cargo build`, `cargo test` and `cargo clippy` through `crates/rb-model/build.rs`, in `cargo xtask lint`, and as its own CI job, and it verifies both the file and the `#anchor` ([ADR-0023](docs/adr/0023-documentation-link-and-lint-gates.md)). A broken link fails the compile; `RB_SKIP_DOC_LINK_CHECK=1` bypasses it locally while you move a document. When you add a crate, copy the doc-comment header pattern from `crates/rb-model/src/lib.rs`.
4. **Update the ledger.** A coverage-tab row in `docs/artifacts/` is never edited. When the pinned upstream suite proves a row, the plan's status table records it with the CI run as evidence; the tab itself is regenerated from the design document, not hand-edited.
5. **Move the plan when it is done.** The checklist at the end of each plan's third section says what "done" means. The pull request that moves a plan to `implemented/` links the green checks for every exit criterion.

## Linting

One entry point runs every linter for every language, and the documentation link check runs with them: **`cargo xtask lint`** (`--fix` to apply what each linter can fix). CI runs `cargo xtask lint --strict`, where a linter that is not installed fails instead of being skipped. A language whose tree does not exist yet reports "not applicable"; the wave that lands its first file turns that into a pass ([ADR-0023](docs/adr/0023-documentation-link-and-lint-gates.md)).

| Language | Linters | Configuration |
| --- | --- | --- |
| Documentation | link and anchor check over every `.md` and every Rust doc comment | `xtask/src/doclinks.rs` |
| Rust | `rustfmt`, `clippy` (pedantic; `unwrap`, `expect`, `panic` denied), `cargo deny` | `rustfmt.toml`, `[workspace.lints]`, `deny.toml` |
| TypeScript, JavaScript | `eslint` with `typescript-eslint` strict and stylistic type-checked, `prettier` | `eslint.config.mjs`, `.prettierrc.json`, `tsconfig.base.json`, `package.json` |
| Python | `ruff check` (`select = ["ALL"]`), `ruff format`, `mypy --strict` | `pyproject.toml` |
| C# | .NET analyzers at `latest-recommended`, warnings as errors, `dotnet format` | `Directory.Build.props`, `.editorconfig` |

Do not add a per-package linter configuration that contradicts these; extend the root file instead, so one command still tells the truth about the whole repository.

## Testing and coverage

The floor is **70% line coverage per crate and per wrapper, adapter and front-end**, as a required check ([ADR-0018](docs/adr/0018-test-coverage-threshold.md)). It is a floor, not a target; the extractors and the engine sit far above it because the conformance fixtures drive them.

| Toolchain | Command | Where the threshold lives |
| --- | --- | --- |
| Rust | `cargo llvm-cov --workspace --all-features --fail-under-lines 70` then `scripts/coverage-per-crate.sh 70` | `.github/workflows/ci.yml` |
| TypeScript | `vitest run --coverage` | `vitest.config.ts` `coverage.thresholds.lines = 70` in each package |
| C# | `dotnet test /p:CollectCoverage=true /p:Threshold=70 /p:ThresholdType=line` | the test project |
| Python | `pytest --cov --cov-fail-under=70` | `pyproject.toml` |

Excluded from coverage by path: generated stdlib snapshots in `rb-extract-python`, generated schema output under `schema/`, and vendored upstream fixtures under `conformance/`. Nothing else.

Coverage is a floor, not the measure of a good test. Three further gates say whether the tests assert anything ([ADR-0024](docs/adr/0024-test-quality-gates.md)):

| Gate | Command | Rule |
| --- | --- | --- |
| Mutation testing | `cargo mutants --package rb-model --package rb-rules --package xtask` | A surviving mutant fails CI. It is a missing assertion, so the fix is a test, never a new exclusion. Exclusions live in `.cargo/mutants.toml` and each carries its reason |
| Property tests | `cargo test` | A pure function over an open input space (a hash, a slug, a path, a parser) carries `proptest` invariants beside its examples |
| Snapshots | `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-cli` | The command-line help is a committed snapshot, because flag parity is a promise. Regenerating one is a reviewable diff that must be explained |

What a change must ship with:

- **A unit test for every public function** and a property or table test where the input space is a vocabulary (severities, exit codes, dependency types, output types).
- **A fixture when behaviour is specified upstream.** An extractor change runs the relevant `test/extract` fixtures; a reporter change is byte-compared against `test/report`; an element predicate is proven by its ported ArchUnitNET case. The gates ratchet: `conformance/excluded.json` and the unported count may only shrink.
- **A fixed-vector test for anything that is a contract**: the violation-id hash, the exit-code table, the JSON field names. Changing one is a new ADR.
- **A sandbox-escape test** for any change to the QuickJS evaluator ([ADR-0006](docs/adr/0006-embedded-quickjs-config-evaluator.md)).
- **A timing** when a plan names a performance target; the nightly scale table is the record.
- **A determinism assertion** for anything that reaches output. Two runs on the same inputs serialise byte for byte, and a test proves it rather than assuming it.

## Rust conventions

The workspace lints in `Cargo.toml` are the rule; `cargo clippy --workspace --all-targets -- -D warnings` must be clean and `cargo fmt --all -- --check` must pass.

- **Edition 2024, `unsafe_code = "forbid"`.** No `unsafe` anywhere in the workspace; the metadata reader parses bytes with safe slicing and checked arithmetic. The one permitted exception is `rb-node`, whose napi-rs macros generate `unsafe`: that crate may lower the lint at crate level in wave 3, and only the macro-generated code may be unsafe ([Wave 3 plan](docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md)).
- **No `unwrap`, `expect` or `panic` outside tests.** Untrusted input (assemblies, PDBs, source files, configs, JSON) must produce exit 2 with a named reason, never a panic ([architecture § Security posture](docs/architecture.md#security-posture)). Library crates return `Result` with a `thiserror` enum; only `rb-cli` decides the exit code, through one function tested against the table in [ADR-0008](docs/adr/0008-exit-code-contract.md).
- **`missing_docs` is on.** Every public item has a doc comment. A crate's `lib.rs` opens with the linked header (architecture, decisions, plan, requirements, source).
- **The extractor boundary is absolute.** An extractor depends on `rb-model` only and writes graph-document types. `rb-rules` and `rb-report` contain no `match language`; they compare strings the document carries. If you need a language fact in the engine, put it in the document at extraction time ([ADR-0010](docs/adr/0010-crate-layout-and-extractor-boundary.md), [ADR-0014](docs/adr/0014-no-invented-cross-language-edges.md)).
- **Determinism.** Sort modules, dependencies and violations before output; never iterate a `HashMap` into output. Prefer `BTreeMap` where order reaches a reporter. Two runs on the same inputs serialise byte for byte.
- **Field names are a public contract.** The module layer is dependency-cruiser's `cruise-result` schema, camelCase, unchanged. Additions are additive and `skip_serializing_if` when absent. Renaming a field is a breaking change and needs an ADR ([ADR-0004](docs/adr/0004-graph-document-is-cruise-result-superset.md)).
- **Regex.** Only the `regex` crate. No `fancy-regex`. Captured groups are substituted escaped ([ADR-0016](docs/adr/0016-linear-time-regex-and-strict-compat.md)).
- **Dependencies** are declared once in `[workspace.dependencies]` and must pass `cargo deny` (MIT-compatible only; no GPL). Add a dependency in the plan that needs it, not in advance.
- **Features.** Each extractor is a feature of `rb-cli`; `cargo build -p rb-cli --no-default-features --features extract-python` must build. Do not add cross-feature `cfg` in the engine.
- **Performance.** Parallelise with `rayon` at the file level; never hold a whole repository's source text in memory at once; the cache is content-addressed under `.graph/`. Measure with the nightly table, not by feel.
- **Errors for agents.** Every error message names the file, the rule and the fix where one exists; `err-long`, `sarif`, `junit` and `agent` all print the rule's `fix` text.

## TypeScript conventions (wrappers, vitest reporter, ESLint plugin)

- `strict: true`, ESM, Node 22 LTS, `vitest` for tests, `eslint` with `typescript-eslint` strict-type-checked, `prettier` for formatting.
- The npm wrapper carries the platform binary under `optionalDependencies` and nothing else; it never re-implements a subcommand.
- The ESLint rule `rulebearing/boundaries` asks the cached graph through `can-import` and must report the same finding the gate would, with the `fix` text as the message.
- Match dependency-cruiser's API signatures exactly where the coverage tab says Parity (`cruise()`, `format()`, the `extract*` functions).

## C# conventions (test adapter, Roslyn analyzer, fallback extractor if invoked)

- .NET 10 SDK, `<Nullable>enable</Nullable>`, `<TreatWarningsAsErrors>true</TreatWarningsAsErrors>`, `<EnforceCodeStyleInBuild>true</EnforceCodeStyleInBuild>`, analyzers on, `dotnet format` clean.
- xUnit for tests; coverlet for coverage with the 70% line threshold.
- `Rulebearing.TestAdapter` yields one test per rule from the JSON with the `fix` text in the failure message; it never evaluates rules itself.
- `Rulebearing.Analyzer` reports `RB0001`-style diagnostics whose message is the rule's `fix`; the analyzer reads `rulebearing.yaml` and must agree with the binary on every finding it reports.
- If the C# fallback extractor is invoked ([ADR-0003](docs/adr/0003-dotnet-extractor-fallback.md)), it uses `System.Reflection.Metadata` only and writes the graph document schema from `schema/`; it is MIT.

## Python conventions (pip wrapper, pytest plugin)

- Python 3.11 minimum, `pyproject.toml` with `hatchling`, `ruff` for lint and format (`select = ["ALL"]` with documented ignores), `mypy --strict`, `pytest` with `pytest-cov` at 70%.
- The wheel carries the platform binary and a console script; it never re-implements a subcommand.
- `pytest-rulebearing` yields one test per rule from the JSON with the `fix` text in the failure message.

## Documentation conventions

- **ADRs** use the template in [ADR-0001](docs/adr/0001-record-architecture-decisions.md): Status, Date, Derives from, Constrains, Context, Decision, Consequences, Alternatives considered. Add the row to `docs/adr/README.md`.
- **Plans** have exactly three sections in this order: architect (for the architectural review board), lead developer (step by step), wave-based delivery (each sub-wave with a status table, a t-shirt size and a level of effort). The template and the sizing scale are in [docs/plans/README.md](docs/plans/README.md).
- **Style:** plain engineering prose, no marketing, no em-dashes, tables for parallel facts, code fences for YAML, JSON, Rust and command lines, relative links everywhere. Every behavioural claim cites the design section, a coverage-tab row, or an ADR.
- **User-facing docs** are generated from the rules where the design says so (`docs --format agents-md | contributing | skill`); never hand-write a table that `rulebearing docs` can produce.

## Commands you will run

```sh
cargo lint                                              # every linter, every language, plus doc links
cargo lint-fix                                          # the same, applying what can be fixed
cargo check-links                                       # documentation links and anchors only
cargo ci                                                # lint, then the workspace test suite
cargo build --release                                   # target/release/rulebearing (checks doc links)
cargo test --workspace --all-features
cargo llvm-cov --workspace --all-features --fail-under-lines 70 && scripts/coverage-per-crate.sh 70
cargo mutants --package rb-model --package rb-rules --package xtask   # a survivor fails CI
cargo deny check licenses advisories bans sources       # needs cargo-deny
typos && actionlint && git ls-files '*.sh' | xargs shellcheck --severity=style
conformance/dependency-cruiser/run.sh                   # gate 1, layers 1 and 2 (needs Node 22)
scripts/gate2-check.sh && scripts/ratchets.sh           # gate 2 fixture check; the conformance ratchets
cargo test -p rb-extract-ts --test extract_fixtures -- --nocapture   # layer 1 alone: prints passed/total/ratio
conformance/archunitnet/scripts/spike-b-attribution.sh # .NET attribution over the oracles (needs the .NET SDK)
fuzz/run.sh metadata_reader 600                         # fuzz the metadata reader (nightly toolchain, cargo-fuzz)
wrappers/publish-placeholders.sh --dry-run              # package the four 0.0.1 name reservations (docs/release.md)
./target/release/rulebearing --help
```

The aliases live in `.cargo/config.toml`. `scripts/check-links.sh` and `npm run lint` are thin wrappers over the same code, so all three routes give the same answer.

## Definition of done for a pull request

- The plan and sub-wave it belongs to are named in the description, with the requirement IDs and ADRs.
- `cargo lint` (doc links, rustfmt, clippy, eslint, prettier, ruff, mypy, dotnet format), `deny`, `test` (including doc tests), coverage (workspace and per crate), mutation testing, spelling, `actionlint`, `shellcheck` and both conformance gates are green.
- The conformance ratchets did not grow; if a row in a coverage tab is now proven, the plan's status table says so with the run linked.
- New public items have doc comments with links; new crates have the linked header.
- No `unsafe`, no `unwrap`/`expect`/`panic` outside tests, no network, no code execution outside the sandbox.
- If a decision was made, an ADR was added; if a plan's exit criteria are all met, the plan was moved.

## Things not to do

- Do not rename or drop a dependency-cruiser key, field, flag or output type. The promise is "nothing is dropped".
- Do not invent a cross-language edge, a heuristic resolution, or a silent false for a predicate a language cannot answer. Exit 3 with the rule named.
- Do not let a vacuous rule pass. Exit 2, `summary.vacuousRules[]` filled.
- Do not raise a ratchet ceiling, widen an `allowed` list, or add an `allowEmpty` to get green. Fix the code or record the exception with `expires` and an owner.
- Do not edit `docs/artifacts/`, an accepted ADR, or a plan's exit criteria to fit the work. Change the work, or write the ADR that changes the decision.
- Do not disable a linter rule, add a blanket `allow`, or set `RB_SKIP_DOC_LINK_CHECK` in a script or a workflow to get green. Fix the code or the link.
- Do not silence a surviving mutant with an exclusion, or regenerate a snapshot without saying what changed and why. Both are ways of deleting a test while appearing to keep it.
- Do not unpin a GitHub Action, widen `permissions:`, or add a dependency from a git URL or another registry. The first two need a review, the third needs an ADR.
- Do not commit or publish without being asked. Do not push a placeholder to a registry outside the wave 0E release task.
