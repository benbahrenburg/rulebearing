# Architecture Decision Records

Every decision that shapes Rulebearing's design is recorded here as an Architecture Decision Record (ADR). The format and the process are set by [ADR-0001](0001-record-architecture-decisions.md). Each ADR links to the section of the [design document](../artifacts/design.md) it derives from, to the [architecture](../architecture.md) section it constrains, and to the [plan](../plans/README.md) that implements it.

A rule in `rulebearing.yaml` cites a decision with the token `adr:NNNN` in its `comment`. The same convention applies to this repository's own rules once wave 1 lands.

| ADR | Title | Status |
| --- | --- | --- |
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions, plans and artifacts in `docs/` | Accepted |
| [0002](0002-rust-as-implementation-language.md) | Rust as the implementation language, one static binary | Accepted |
| [0003](0003-dotnet-extractor-fallback.md) | The .NET extractor falls back to a C# `dotnet tool` on a measured trigger | Superseded by [0022](0022-dotnet-reader-in-rust-confirmed.md) |
| [0004](0004-graph-document-is-cruise-result-superset.md) | The graph document is an additive superset of dependency-cruiser's `cruise-result` schema | Accepted |
| [0005](0005-native-config-superset-and-compat.md) | A native config format that is a strict superset, and dependency-cruiser's format accepted as is | Accepted |
| [0006](0006-embedded-quickjs-config-evaluator.md) | JavaScript configs are evaluated in a sandboxed embedded QuickJS engine | Accepted |
| [0007](0007-vacuous-rules-fail-by-default.md) | A rule whose selection is empty fails by default | Accepted |
| [0008](0008-exit-code-contract.md) | Exit codes: error count, 2 for an untrustworthy run, 3 for an invalid config | Accepted |
| [0009](0009-conformance-suites-as-specification.md) | dependency-cruiser's and ArchUnitNET's test suites are the specification | Accepted |
| [0010](0010-crate-layout-and-extractor-boundary.md) | Crate layout: extractors behind a feature-gated boundary, the engine language-agnostic | Accepted |
| [0011](0011-read-dotnet-assemblies-not-source.md) | .NET edges come from built assemblies and portable PDBs, not C# source | Accepted |
| [0012](0012-oxc-for-typescript.md) | `oxc_parser` and `oxc_resolver` for TypeScript and JavaScript | Accepted |
| [0013](0013-ruff-parser-for-python.md) | `ruff_python_parser` and a versioned stdlib list for Python | Accepted |
| [0014](0014-no-invented-cross-language-edges.md) | No cross-language edges are invented | Accepted |
| [0015](0015-stable-violation-id.md) | Every violation carries a stable id and a line and column | Accepted |
| [0016](0016-linear-time-regex-and-strict-compat.md) | Linear-time regex engine, with `--strict-compat` for portability | Accepted |
| [0017](0017-coffeescript-livescript-sidecar.md) | CoffeeScript and LiveScript run through a Node sidecar | Accepted |
| [0018](0018-test-coverage-threshold.md) | 70% line coverage is a required check on every crate and wrapper | Accepted |
| [0019](0019-mit-licence.md) | MIT licence | Accepted |
| [0020](0020-single-name-across-registries.md) | One name, `rulebearing`, on every registry | Accepted |
| [0021](0021-agent-surface-cli-first.md) | The command line with the `agent` reporter is the primary agent surface; MCP and LSP are additive | Accepted |
| [0022](0022-dotnet-reader-in-rust-confirmed.md) | The .NET extractor stays the Rust metadata reader; the C# fallback is not invoked (trigger figure 0.9929) | Accepted |
| [0023](0023-documentation-link-and-lint-gates.md) | Documentation links are checked on every compile, and every language has a configured linter behind one entry point | Accepted |
| [0024](0024-test-quality-gates.md) | Test quality is gated by mutation testing, property tests and snapshots, not by coverage alone | Accepted |
| [0025](0025-ci-and-supply-chain-hardening.md) | CI runs least-privileged, pinned and bounded, and the dependency supply chain is closed | Accepted |
| [0026](0026-bsl-exception-for-dragonbox.md) | One crate-scoped licence exception: BSL-1.0 for `dragonbox_ecma`, which oxc requires | Accepted |
| [0027](0027-pure-path-and-url-modules-in-the-config-sandbox.md) | The configuration sandbox offers pure `path` and `url` modules; every other Node built-in stays behind `--config-via-node` | Accepted |
| [0028](0028-backreferences-by-instantiation-on-the-linear-engine.md) | Backreferences are matched by instantiation on the linear-time engine; lookaround stays refused | Accepted |
| [0029](0029-ratchets-enforced-by-cruise-and-reported-in-the-summary.md) | Ratchets are enforced by `cruise` and reported in `summary.ratchets[]`; a missing budget exits 2 | Accepted |
| [0030](0030-the-reporter-decides-the-error-count-exit.md) | The reporter decides whether the error count is the exit code, as in dependency-cruiser; 2 and 3 do not depend on it | Accepted |
| [0031](0031-a-saved-result-carries-what-the-exit-code-counts.md) | A saved result carries everything the exit code counts: `summary.expired[]`; `fmt --exit-code` reads it and `vacuousRules` | Accepted |
| [0032](0032-liveness-follows-the-configuration-format.md) | Liveness follows the configuration's format: `warn` for a dependency-cruiser file, `strict` for a native one; named exceptions in `allowEmpty` | Accepted |
| [0033](0033-llvm-exception-for-ar-archive-writer.md) | One crate-scoped licence exception: Apache-2.0 WITH LLVM-exception for `ar_archive_writer`, which the Python parser's build requires | Accepted |
| [0034](0034-slices-group-types-or-modules-and-segments.md) | A slice groups .NET types or TypeScript and Python modules; `(*)` names a slice as `ArchUnitNET` does; `segments` keeps the first segments | Accepted |
| [0035](0035-referenced-types-in-the-code-layer.md) | The code layer holds the types the analysed code references, as `ArchUnitNET`'s `ReferencedTypes` | Accepted |
| [0036](0036-markdown-fences-follow-the-configuration-format.md) | Markdown fences are read for a native configuration; a dependency-cruiser one keeps upstream's `extraExtensionsToScan` | Accepted |
| [0037](0037-baseline-modes.md) | `baseline` has three modes of Rulebearing's own (`full`, `shrink-only`, `format`); dependency-cruiser has none | Accepted |
