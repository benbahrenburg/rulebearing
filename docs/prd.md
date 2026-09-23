# Rulebearing: product requirements

| | |
| --- | --- |
| **Version** | 1 |
| **Date** | 2026-09-20 |
| **Owner** | Ben Bahrenburg |
| **Status** | Draft for review |

**Sources:** [design document](artifacts/design.md), [dependency-cruiser 18.2.0 coverage](artifacts/dependency-cruiser-18.2.0-coverage.md), [ArchUnitNET 0.13.4 coverage](artifacts/archunitnet-0.13.4-coverage.md), [target architecture](architecture.md), [ADR index](adr/README.md). Every requirement below states what the product must do; the architecture says how, the [plans](plans/README.md) say when, and the coverage tabs are the ledger that says whether a parity claim has been proven.

## Problem statement and vision

Rulebearing is one open-source command-line tool and one rule language that does what dependency-cruiser does for TypeScript and JavaScript, what ArchUnitNET does for .NET, and what import-linter does for Python, over a single graph. A repository with more than one language keeps one set of architecture rules, one gate, and one answer to "may this file import that one" ([design § Why](artifacts/design.md#why)). It is a personal project under the MIT licence at `benbahrenburg/rulebearing`.

The three incumbent tools each have a real user base, and none of them reads the others' repositories, none of the .NET or Python ones emits a graph another script can consume, and none was designed for a coding agent as the reader of its findings. What has to carry over is not a tool but the practice around it, learned from running dependency-cruiser daily in a private 5,500-module monorepo: a rule is a named, commented fence with a severity whose name a decision record can cite; matchers hit the resolved target so a boundary cannot be evaded by import style; the graph is a first-class artefact that other guards read; and liveness is checked, because a rule whose from-side matches zero files is a fence that is not standing ([design § Why](artifacts/design.md#why)).

The name states the product's most distinctive check. A load-bearing wall is the one you cannot take out; a rule-bearing codebase is one whose rules carry real weight, and the tool asks of every rule: is it bearing anything? A rule that matches nothing bears nothing, and the tool fails it by default ([design § The name](artifacts/design.md#the-name)). The name `rulebearing` was verified free on npm, PyPI, crates.io and NuGet on 2026-09-20 ([ADR-0020](adr/0020-single-name-across-registries.md)).

The first repository to switch will be a heavy dependency-cruiser user, and it will switch with its existing config file. That repository uses a narrow slice of the rule language (48 `forbidden` rules, `$1` captures, `to.circular`, `from.orphan`, `type-only` exclusions, a decision token in every comment, one config computed in JavaScript) and a wide slice of the output: one 13-second cruise writes a 19 MB JSON that five guards read (the gate, a ratchet, an importer finder, an ADR guard, a severity pin test), plus a toolchain guard that exists only because an unsupported TypeScript version makes dependency-cruiser cruise zero modules and exit 0 ([design § What a heavy dependency-cruiser user needs](artifacts/design.md#what-a-heavy-dependency-cruiser-user-needs)). That slice must be bit-for-bit identical on day one, and the silent empty cruise is the one failure mode to design out.

## Goals and non-goals

### Goals

1. **Superset, precisely.** Every rule attribute, option, command-line flag, reporter and result field in dependency-cruiser 18.2.0, and every selector, predicate, condition, slice rule, loader option and PlantUML feature in ArchUnitNET 0.13.4, has a coverage-tab row with a status; nothing is dropped ([design § Specification coverage](artifacts/design.md#specification-coverage)).
2. **Drop-in.** Every one of the 112 `.dependency-cruiser.*` files the search found is a valid Rulebearing configuration on day one, and the JSON field names, `fmt`, the `err` reporter and the exit code are identical ([ADR-0005](adr/0005-native-config-superset-and-compat.md)).
3. **One graph, three languages.** One `cruise`, one graph, one rule pass, one exit code and one JSON for a repository with TypeScript apps, a .NET service and a Python pipeline ([design § One engine, three languages, one monorepo](artifacts/design.md#one-engine-three-languages-one-monorepo)).
4. **Agent-shaped.** Line-precise findings, `fix` text, stable ids, a receipt, a token-budgeted reporter, and a sub-two-second affected run so the check lives inside the loops agents already run ([design § Rules an agent can implement and follow](artifacts/design.md#rules-an-agent-can-implement-and-follow)).
5. **Migration is a command.** `import archunit`, `import import-linter` and `import eslint` make the 88 .NET and 109 Python repositories found by the search possible adopters ([design § The five to fund first](artifacts/design.md#the-five-to-fund-first)).
6. **Conformance is measured, not claimed.** dependency-cruiser's and ArchUnitNET's own test suites are the specification and run as required, ratcheting checks ([ADR-0009](adr/0009-conformance-suites-as-specification.md)).

### Non-goals

| Non-goal | Why | Source |
| --- | --- | --- |
| Invented cross-language edges | A C# service that calls a Python process over HTTP has no import edge to it, and the tool does not guess; that relationship belongs in a service-contract check. Wave 4 adds *declared* edges, marked as such. | [design § What stays honest across the boundary](artifacts/design.md#what-stays-honest-across-the-boundary), [ADR-0014](adr/0014-no-invented-cross-language-edges.md) |
| Custom C# predicates and conditions | `FollowCustomPredicate`, `FollowCustomCondition`, `IPredicate<T>` have no declarative form and stay in ArchUnitNET; a repo that needs one keeps a small ArchUnitNET test project beside `rulebearing.yaml`. | [design § Element rules](artifacts/design.md#element-rules-archunitnet-declarative), [ArchUnitNET coverage § Stays](artifacts/archunitnet-0.13.4-coverage.md#stays-in-archunitnet) |
| Replacing the compiler | The Roslyn analyzer is an inner-loop front-end, not a replacement for the metadata extractor, which sees IL and every assembly; `--mode source` is approximate and never the gate. | [design § Where it would be ignored](artifacts/design.md#where-it-would-be-ignored), [ADR-0011](adr/0011-read-dotnet-assemblies-not-source.md) |
| Reading C# source for the .NET edge set | All three .NET tools read assemblies; the compiler has already resolved every reference. | [design § Prior art](artifacts/design.md#prior-art) |
| A product-owned rule file or graph | NDepend keeps its graph; Rulebearing emits a plain JSON other scripts consume. | [design § Prior art](artifacts/design.md#prior-art) |
| Modifying test-bed repositories | Test beds are cloned at a pinned SHA, never modified; a pull request is offered only where a contributing guide invites it, via an issue first. | [design § Test beds](artifacts/design.md#test-beds-open-source-repositories-to-validate-against), [design § Open questions](artifacts/design.md#open-questions) |

## Users and personas

| Persona | Needs | Today's pain | What Rulebearing gives them |
| --- | --- | --- | --- |
| **Heavy dependency-cruiser user, monorepo guard author** | The config file unchanged; `modules[].source` and `dependencies[].resolved` as a public contract; `fmt` and the `err` reporter; exit code equal to the error count; a `rules --json` listing; `dot` output with `collapsePattern` | A 13-second cruise; a separate toolchain guard against the silent empty cruise; ratchet, liveness and ADR guards written by hand; `require`-ing a JavaScript config to read rule names; safe-regex rejecting a needed pattern | A 1 to 2 second cruise with the same config and output path; exit 2 on an untrustworthy run; ratchets as config; liveness by default; `rules --json`; linear-time regex ([design § What a heavy dependency-cruiser user needs](artifacts/design.md#what-a-heavy-dependency-cruiser-user-needs)) |
| **.NET architect on ArchUnitNET or NetArchTest** | The full selector, predicate and condition vocabulary; solution-driven loading; slices and diagram adherence | Rules are C# tests in one project loaded against one assembly; nothing else in the repo can cite, count or diff them; NetArchTest passes silently on an empty selection; no graph export | A declarative rule file with ArchUnitNET's names in camelCase, `import archunit` keeping the C# as a comment, a graph other scripts read, cycle and reachability rules over files, path matchers through the portable PDB ([design § Prior art](artifacts/design.md#prior-art)) |
| **Python team on import-linter** | `forbidden`, `layers`, `independence`, `protected`, acyclic-siblings contracts; unmatched-ignore alerting | Contracts are Python-only; no graph; a mixed repo needs a second tool | Contract kinds map one to one; `import import-linter` reads `.importlinter` or `[tool.importlinter]`; `knownViolations` with `shrink-only` replaces unmatched-ignore alerting; a versioned stdlib list ([design § import-linter contracts](artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them)) |
| **Coding agent (Claude Code), a first-class user** | Findings with a line, a `fix` and a member reference; a yes/no answer before writing an import; a check that runs per turn in under two seconds; rules it writes held to a bar | A finding names two files and no line; fix advice lives in a `comment` only `err-long` prints; four vacuous rules stood for months; a 13-second cruise is not run per turn; the contributing guide documents a stale invocation | The `agent` reporter, `can-import`, `place`, `impact`, `explain`, `hooks install --claude-code`, `docs --format agents-md` and `skill`, `test`, `propose`, `config lint`, ratchets that only fall, hermetic runs ([design § Rules an agent can implement and follow](artifacts/design.md#rules-an-agent-can-implement-and-follow)) |
| **Reviewer, architecture review board member** | Proof that the check ran; a review comment that names an unambiguous finding; the architecture diff of a pull request; every rule cited to a decision | The recurring question on agent-authored pull requests is whether the check ran; findings are not stable across runs; rule files grow and never shrink | `attest`, stable `RB-` ids, `diff --base`, `sarif` and `github-annotations`, `--require-comment-token`, `decisions`, `changelog`, `snapshot`, rule lifecycle fields ([design § The agentic engineering hat](artifacts/design.md#the-agentic-engineering-hat-turn-two), [§ The architect's hat](artifacts/design.md#the-architects-hat-across-repos-and-across-time)) |
| **Open-source maintainer of a test-bed repo** | A drop-in beside the incumbent, never replacing it; zero difference on their own config; no unsolicited pull requests | A new tool asks for maintainer time | Oracle zero-diff at a pinned commit attached to an issue before any pull request; `init` that produces a passing config on greenfield repos; a public nightly run ([design § Test beds](artifacts/design.md#test-beds-open-source-repositories-to-validate-against), [§ Adoption order](artifacts/design.md#adoption-order)) |

## Market evidence and prior art

A GitHub filename search on 2026-09-19 returned **112** repositories carrying a `.dependency-cruiser.*` config, **88** carrying a NetArchTest or ArchUnitNET project reference, and **109** carrying an import-linter contract, capped by the search limits rather than by the population ([design § Why](artifacts/design.md#why)). The repos include the current generation of agent tooling (dify, langfuse, promptflow, langextract, semantic-kernel, autogen) ([design § Will agentic developers embrace it?](artifacts/design.md#will-agentic-developers-embrace-it)).

| Tool | Source of truth | Rule form | Graph export | Baseline | Liveness | Licence |
| --- | --- | --- | --- | --- | --- | --- |
| ArchUnitNET 0.13.4 | Assemblies via Mono.Cecil | Fluent C# in a test | None | None | Fails an empty selection unless `WithoutRequiringPositiveResults` | Apache 2.0 |
| NetArchTest 1.3.2 | Assemblies via Mono.Cecil | Fluent C# | None | None | Silent pass on empty | MIT |
| NDepend | Assemblies, PDBs, source | CQLinq in a licensed project file | Inside the product | Yes | Yes | Commercial |
| import-linter | Source imports via grimp | INI or TOML contracts | Via grimp as a library | `ignore_imports` | Unmatched ignores can alert | BSD-2 |
| dependency-cruiser 18.2.0 | Source imports via enhanced-resolve | JSON, YAML or JavaScript config | `json`, `dot` and 19 more | `knownViolations` | None; heavy users bolt it on | MIT |

Full table: [design § Prior art](artifacts/design.md#prior-art). What it settles: read assemblies for .NET, keep the rule file declarative and text, emit the graph, take both test suites as the specification.

### Test beds by role

| Role | What it validates | Repositories |
| --- | --- | --- |
| **Oracle** (carries a config; zero difference required at a pinned commit) | TypeScript | sverweij/dependency-cruiser, langfuse/langfuse, microsoft/FluidFramework, ag-grid/ag-grid, tsparticles/tsparticles, liveblocks/liveblocks, remult/remult, invertase/react-native-firebase, infinitered/ignite, aws/aws-toolkit-vscode |
| | .NET | evolutionary-architecture/evolutionary-architecture-by-example, phongnguyend/Practical.CleanArchitecture, ardalis/RiverBooks, nager/Nager.Date, karaoke-dev/karaoke, onebeyond/monaco, dennisdoomen/packageguard, NeVeSpl/NetArchTest.eNhancedEdition, DrJohnMelville/Pdf; TNG/ArchUnitNET and BenMorris/NetArchTest as the conformance suites |
| | Python | seddonym/import-linter, kedro-org/kedro, sqlfluff/sqlfluff, bridgecrewio/checkov, openedx/openedx-platform, napari/napari, nolar/kopf, online-ml/river, wemake-services/wemake-python-styleguide, google/langextract, microsoft/promptflow, HKUDS/DeepTutor |
| | Mixed | langgenius/dify, open-metadata/OpenMetadata |
| **Greenfield** (no tool; exercises `init`, `propose`, the multi-language path) | | microsoft/semantic-kernel, microsoft/autogen, jasontaylordev/CleanArchitecture, abpframework/abp, umbraco/Umbraco-CMS, apache/superset, getsentry/sentry, zulip/zulip, PostHog/posthog |
| **Scale** (timed nightly) | | n8n-io/n8n, grafana/grafana, elastic/kibana, dotnet/aspnetcore, jellyfin/jellyfin, home-assistant/core |

Source: [design § Test beds](artifacts/design.md#test-beds-open-source-repositories-to-validate-against).

## Product principles

| Principle | Statement | Consequence |
| --- | --- | --- |
| **Superset, precisely** | Every dependency-cruiser 18.2.0 and ArchUnitNET 0.13.4 feature has a coverage row; a row cannot say Parity until the pinned test suite says so | Two required conformance gates ([ADR-0009](adr/0009-conformance-suites-as-specification.md)) |
| **Drop-in** | The config file, the JSON field names, `fmt`, the `err` reporter and the exit code are identical on day one | Both formats load into one model ([ADR-0005](adr/0005-native-config-superset-and-compat.md)); JavaScript configs run in a sandbox ([ADR-0006](adr/0006-embedded-quickjs-config-evaluator.md)) |
| **The graph is a first-class artefact** | Other guards read it; a tool that only prints violations covers a third of the use | One extraction feeds the gate, every ratchet, the MCP server and `fmt` ([ADR-0004](adr/0004-graph-document-is-cruise-result-superset.md)) |
| **Liveness is checked** | A rule matching nothing fails | Vacuous rules exit 2 by default in a native configuration and warn in a dependency-cruiser one ([ADR-0007](adr/0007-vacuous-rules-fail-by-default.md), [ADR-0032](adr/0032-liveness-follows-the-configuration-format.md)) |
| **One binary, no runtime** | Must run in a TypeScript, a .NET and a Python pipeline | Rust, thin wrappers ([ADR-0002](adr/0002-rust-as-implementation-language.md), [ADR-0020](adr/0020-single-name-across-registries.md)) |
| **Agent-shaped** | Line-precise findings, `fix` text, a token-budgeted reporter, sub-two-second affected runs; the CLI is the primary surface | Stable ids ([ADR-0015](adr/0015-stable-violation-id.md)); CLI first ([ADR-0021](adr/0021-agent-surface-cli-first.md)) |
| **Hermetic** | No network, no code execution outside the sandbox, deterministic ordering | A local run and CI agree byte for byte; `attest` can hash the run ([architecture § Security posture](architecture.md#security-posture)) |

## Functional requirements

Each requirement is stated once here; the [catalogue](plans/README.md) and every plan cite it by id. Waves are calendar waves from [design § Waves](artifacts/design.md#waves).

### Core

#### FR-CORE-01

**One static binary, three extractors, one graph.** Rulebearing MUST ship as one static binary with a TypeScript, a .NET and a Python extractor, each a Cargo feature behind the extractor boundary, feeding one graph document with a module layer (dependency-cruiser's model: files and the edges between them) and a code layer (ArchUnitNET's model: types, members, attributes, calls, inheritance). Every language MUST fill both layers as far as it can, and the rule engine MUST NOT know which language a node came from. The extractors MUST be the only crates that read files other than the config.

Acceptance:
- A Python-only build (`--no-default-features --features extract-python`) links no metadata reader; verified by a CI build matrix.
- `rb-rules` contains no `match language`; enforced by this repository's own `rulebearing.yaml` self-check from wave 1.
- One `cruise` over a repo with all three languages (semantic-kernel) yields one document, one exit code.

Source: [design § Architecture](artifacts/design.md#architecture), [§ Crate layout](artifacts/design.md#crate-layout) | Wave: 0-2 | ADRs: [0002](adr/0002-rust-as-implementation-language.md), [0010](adr/0010-crate-layout-and-extractor-boundary.md)

#### FR-CORE-02

**Five-stage pipeline; `fmt` re-reports without extracting.** `cruise` MUST run five stages: discover the workspace without building anything, extract both layers, build the graph document, evaluate rules, report. `fmt <json>` MUST re-report a saved JSON without re-extracting, as `depcruise-fmt` does, so one extraction feeds the gate, the graph, every ratchet and the MCP server. Discovery alone MUST support project-level rules and MUST be fast enough for a pre-commit hook.

Acceptance:
- `cruise --output-type json > cruise.json` followed by `fmt --exit-code --output-type err cruise.json` gives the same violations and exit code as a single `cruise --output-type err`.
- `fmt` performs no filesystem reads beyond the JSON and the config; verified with a sandboxed test.
- Conformance gate 1 layer 3 runs every `test/report` fixture through `fmt`.

Source: [design § The five stages](artifacts/design.md#the-five-stages) | Wave: 1 | ADRs: [0010](adr/0010-crate-layout-and-extractor-boundary.md)

#### FR-CORE-03

**The graph document is `cruise-result` plus additive fields.** The module layer MUST be dependency-cruiser 18.2.0's `cruise-result` schema unchanged: `modules[]`, `folders[]`, `summary`, `revisionData` with every field the coverage tab lists. Additions MUST be additive only: `language`, `project`, `namespaces`, `attribution` on modules; `line`, `column`, `dependencyKind`, `member` on dependencies; `inspected` and `vacuousRules[]` on `summary`; and a top-level `code` section with `types[]`, `members[]`, `attributes[]`, `calls[]`. A script that reads `modules[].dependencies[].resolved` today MUST read the new document unchanged. The superset schema MUST be published at a stable `$schema` URL.

Acceptance:
- Conformance gate 1 layer 4: every emitted `json` validates against the pinned 18.2.0 `cruise-result` schema once additions are stripped.
- The reference monorepo's ratchet and importer-finder scripts run unchanged against Rulebearing's JSON.
- `schema/v1.json` is generated from `rb-model` and committed; a diff fails CI.

Source: [design § The five stages](artifacts/design.md#the-five-stages), [coverage § Result document](artifacts/dependency-cruiser-18.2.0-coverage.md#result-document-cruise-result-schema) | Wave: 0-1 | ADRs: [0004](adr/0004-graph-document-is-cruise-result-superset.md)

#### FR-CORE-04

**Line and column on every edge, a stable id on every violation, a receipt on every report.** Every edge MUST carry `line` and `column` from the AST span, the PDB sequence point or the Python node. Every violation MUST carry an `id` of the form `RB-` plus eight hex characters, a SHA-256 over rule, from, to and dependency kind, excluding the line so an unrelated edit does not churn it; the id is the SARIF fingerprint, the baseline key and the reference in a review comment. Every report MUST carry `summary.inspected` with counts of files, assemblies and modules per language, so "the check passed" is distinguishable from "the check looked at nothing".

Acceptance:
- A fixed-vector test in `rb-model` pins the hash; changing it fails CI.
- Every reporter fixture that prints a finding shows a line for TypeScript, .NET (PDB) and Python inputs.
- `inspected` is present and non-zero in every `json` produced on the oracle repos.

Source: [design § Precision an agent can act on](artifacts/design.md#precision-an-agent-can-act-on) | Wave: 1 | ADRs: [0015](adr/0015-stable-violation-id.md)

#### FR-CORE-05

**Vacuous rules fail by default.** A rule of any family whose selecting side (`from`, `module` or `select`) matches nothing MUST be listed in `summary.vacuousRules[]` and MUST make the run exit 2. `allowEmpty: true` on a rule (`WithoutRequiringPositiveResults` in compatibility mode), or the rule's name in a native file's top-level `allowEmpty` list, MUST turn the check off for that rule; `--no-liveness` MUST turn it off globally for a repository with its own guard. A dependency-cruiser configuration run as it is MUST report a vacuous rule as a warning without changing the exit code, as dependency-cruiser does, and `--liveness strict` MUST make it fail ([ADR-0032](adr/0032-liveness-follows-the-configuration-format.md)).

Acceptance:
- A native config with one rule whose `from.path` matches no file exits 2 and names the rule; with `allowEmpty: true`, or the rule named in `allowEmpty`, it exits 0.
- The same rule in a `.dependency-cruiser.*` file exits as dependency-cruiser does, with a warning naming the rule; `--liveness strict` exits 2.
- Conformance gate 1 runs the upstream specs with `--no-liveness`, documented in `conformance/README.md`.
- `rules --json` reports `fromMatches` and `toMatches` for every rule.

Source: [design § Why](artifacts/design.md#why), [§ The five stages](artifacts/design.md#the-five-stages) | Wave: 1 | ADRs: [0007](adr/0007-vacuous-rules-fail-by-default.md), [0032](adr/0032-liveness-follows-the-configuration-format.md)

#### FR-CORE-06

**Exit-code contract.** The exit code MUST be 0 for no error-severity violation; 1 to 255 for the number of error-severity violations, capped at 255; 2 when the run cannot be trusted (zero modules found, a solution with no built assemblies, a non-portable PDB, an unsupported file the sidecar could not handle, or a vacuous rule under the default liveness setting); 3 when the config is invalid against the schema or a predicate names a concept the language lacks. Only `error` severity counts; `warn`, `info` and `ignore` do not.

Acceptance:
- One exit-code function in `rb-cli` shared by every reporter and `fmt`, tested against a table.
- The unsupported-transpiler case that makes dependency-cruiser cruise zero modules and exit 0 exits 2 with a named reason.
- `--exit-code-mode strict` (wave 3) shifts the count to `10 + n`.

Source: [design § Exit codes](artifacts/design.md#exit-codes) | Wave: 1 | ADRs: [0008](adr/0008-exit-code-contract.md)

#### FR-CORE-07

**Hermetic, deterministic runs.** A run MUST make no network connection, MUST execute no code outside the sandboxed config evaluator unless `--sidecar node` or `--config-via-node` is passed explicitly, and MUST sort modules, dependencies and violations before output so an agent's local run and CI agree byte for byte.

Acceptance:
- Two runs on the same inputs produce byte-identical `json`; the nightly test-bed run diffs consecutive runs.
- A network-denied sandbox test passes for every subcommand.
- The report records when a sidecar or Node was spawned.

Source: [design § Rules an agent writes, held to the same bar](artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar) | Wave: 1 | ADRs: [0006](adr/0006-embedded-quickjs-config-evaluator.md)

### Configuration

#### FR-CFG-01

**dependency-cruiser configs accepted as is.** `.dependency-cruiser.json`, `.yaml`, `.yml`, `.cjs`, `.js` and `.mjs` MUST be detected by name (or forced with `--config-format dependency-cruiser`) and MUST run without conversion. `extends` MUST resolve files, npm packages and the bundled presets `recommended`, `recommended-strict` and `recommended-warn-only`, as a string or an array, exactly as today.

Acceptance:
- Every accepted config validates against the pinned 18.2.0 `configuration` schema (gate 1 layer 4).
- Each TypeScript oracle repo cruises with its own config unchanged and zero-diffs `modules[]`, `dependencies[]` and `summary.violations` (gate 1 layer 5).

Source: [design § The dependency-cruiser format](artifacts/design.md#the-dependency-cruiser-format), [coverage § Rules](artifacts/dependency-cruiser-18.2.0-coverage.md#rules) | Wave: 1 | ADRs: [0005](adr/0005-native-config-superset-and-compat.md)

#### FR-CFG-02

**A native format that is a strict superset.** `rulebearing.yaml` (also `.json`, `.jsonc`, `.toml`) MUST accept every dependency-cruiser key at the same place with the same meaning, MUST group rules by family (`rules.dependencies`, `elements`, `slices`, `diagrams`, `ratchets`), and MUST publish a `$schema` with descriptions. The native format adds keys; it MUST NOT rename any.

Acceptance:
- A dependency-cruiser config pasted under the native top level runs identically.
- The schema is served at `https://benbahrenburg.github.io/rulebearing/schema/v1.json` and the sample config in the design validates against it.

Source: [design § The native format](artifacts/design.md#the-native-format) | Wave: 1 | ADRs: [0005](adr/0005-native-config-superset-and-compat.md)

#### FR-CFG-03

**JavaScript configs in an embedded sandbox.** `.cjs`, `.js` and `.mjs` configs MUST be evaluated in an embedded QuickJS engine with a CommonJS and ESM shim whose `require` and `import` resolve JSON files, other config modules on disk and the bundled `dependency-cruiser/configs/*` presets, and nothing else: no filesystem beyond the repo, no network, no `process`, no timers. A config that needs more MUST run through `--config-via-node`, which asks a local Node to print the evaluated object as JSON; `--webpack-config-json` MUST accept a pre-evaluated webpack config.

Acceptance:
- The reference monorepo's computed config (a `require` of an exceptions JSON spliced into a regex) evaluates unchanged.
- A sandbox-escape test (filesystem outside the repo, `process`, network) fails as expected.

Source: [design § The dependency-cruiser format](artifacts/design.md#the-dependency-cruiser-format) | Wave: 1 | ADRs: [0006](adr/0006-embedded-quickjs-config-evaluator.md)

#### FR-CFG-04

**`defines`.** The native format MUST provide `defines`: a named value read from a JSON file, selected with a small path expression (`fromJson`, `select`, `joinWith`), and referenced as `${name}` in any pattern, as the declarative replacement for computed JavaScript.

Acceptance:
- The design's `legacyApps` example expands to the expected alternation; `config expand` shows it.
- A missing file or an empty selection is a `config lint` error.

Source: [design § The native format](artifacts/design.md#the-native-format) | Wave: 1 | ADRs: [0005](adr/0005-native-config-superset-and-compat.md)

#### FR-CFG-05

**`config convert`, `config lint`, `config expand`.** `config convert` MUST translate dependency-cruiser to native losslessly and native to dependency-cruiser lossily, saying exactly what it dropped (element, slice, diagram and ratchet rules; `fix`; `examples`). `config lint` MUST report a rule that can never match, a rule shadowed by an earlier one, overlapping `allowed` entries, an `allowed` list that admits everything, a severity below `error` on a rule with zero current violations, a rule with no `fix`, a `fix` that restates the rule name, and a predicate the language cannot answer. `config expand` MUST print the expansion of shorthands and `defines`.

Acceptance:
- Round-trip dc to native to dc is byte-identical after key ordering.
- Each lint finding has a fixture in `rb-config/tests`.

Source: [design § The native format](artifacts/design.md#the-native-format), [§ Shorthands](artifacts/design.md#shorthands), [§ Rules an agent writes](artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar) | Wave: 1 | ADRs: [0005](adr/0005-native-config-superset-and-compat.md)

#### FR-CFG-06

**`languages` block and per-language presets.** `languages.typescript`, `languages.dotnet` and `languages.python` MUST hold per-language settings; dependency-cruiser's flat option names (`tsConfig`, `tsPreCompilationDeps`, `babelConfig`, `webpackConfig`, `enhancedResolveOptions`, `moduleSystems`, `parser`) MUST be accepted at the top level as aliases into `languages.typescript`. Per-language defaults (excludes, orphan exclusions) MUST be presets `rulebearing:typescript`, `rulebearing:dotnet`, `rulebearing:python`, composed by `rulebearing:recommended`, so a single-language repo does not carry the others' excludes.

Acceptance:
- A config with `tsConfig` at the top level and one with `languages.typescript.tsConfig` produce the same `optionsUsed`.
- `rulebearing:python` alone excludes `.venv/`, `site-packages/`, `__pycache__/` and nothing from `obj/` or `node_modules/`.

Source: [design § The native format](artifacts/design.md#the-native-format), [§ What stays honest across the boundary](artifacts/design.md#what-stays-honest-across-the-boundary) | Wave: 1-2 | ADRs: [0005](adr/0005-native-config-superset-and-compat.md)

#### FR-CFG-07

**Rule metadata `fix`, `examples`, `owner`, `expires`.** Every rule of every family MUST accept `fix` (the imperative an agent follows), `examples` (`allowed` and `forbidden` edges that `test` asserts), `owner` and `expires` (a temporary exception with a date; the run fails the day after). These MUST be surfaced by `explain`, `err-long`, `sarif`, `junit` and the `agent` reporter. `--require-comment-token` MUST fail any rule whose `comment` lacks a decision token (`adr:NNNN` or `plan:<slug>`).

Acceptance:
- `err-long` prints the `fix` under each finding; fixture in `rb-report/tests`.
- A rule with `expires` set to yesterday exits 2 with the rule named.
- `--require-comment-token` on the reference monorepo's config passes; on a config with one untokened rule exits 3.

Source: [design § Rule metadata that says what to do](artifacts/design.md#rule-metadata-that-says-what-to-do) | Wave: 1 | ADRs: [0021](adr/0021-agent-surface-cli-first.md)

### Rules

#### FR-RULE-01

**Dependency rules: the whole dependency-cruiser 18.2.0 restriction set.** `forbidden` (regular, reachability and dependents variants), `allowed` with `allowedSeverity`, and `required` MUST be supported with every `from`, `to` and `module` attribute in the coverage tab: `path`, `pathNot`, `orphan`, `circular`, `via`, `viaOnly`, `viaNot`, `viaSomeNot`, `dependencyTypes(Not)`, `dynamic`, `exoticallyRequired`, `exoticRequire(Not)`, `license(Not)`, `moreThanOneDependencyType`, `moreUnstable`, `preCompilationOnly`, `couldNotResolve`, `ancestor`, `reachable`, `numberOfDependentsLessThan`/`MoreThan`. `scope: module|folder` MUST be honoured, and `$0` to `$9` captured in `from.path` MUST substitute into any `to` or `module` pattern. Severity MUST be `error`, `warn`, `info` or `ignore`.

Acceptance:
- Conformance gate 1 layer 2: dependency-cruiser's `test/validate` (25 specs) and `test/graph-utl` specs run unmodified through the shim and pass.
- The reference monorepo's 48 `forbidden` rules produce identical `summary.violations`.

Source: [design § Dependency rules](artifacts/design.md#dependency-rules-the-whole-of-dependency-cruiser-1820), [coverage § Rules](artifacts/dependency-cruiser-18.2.0-coverage.md#rules) | Wave: 1 | ADRs: [0009](adr/0009-conformance-suites-as-specification.md)

#### FR-RULE-02

**Cross-language additions to dependency rules.** `from` and `to` MUST additionally accept `language`, `namespace`/`namespaceNot`, `project`/`projectNot`, `assembly`/`assemblyNot`, `dependencyKind`/`dependencyKindNot` (`inherits`, `implements`, `field`, `signature`, `body`, `attribute`, `generic-argument`, `typeof`). `dependencyTypes` MUST keep dependency-cruiser's forty values for TypeScript exactly; .NET MUST add `local`, `project`, `package`, `framework`, `test-only`, `signature-only`, `unresolved`; Python MUST add `local`, `stdlib`, `site`, `type-only`, `dynamic`, `unresolved`. The validator MUST warn when a .NET rule names `type-only`. A dependency-cruiser config MUST never see the additions.

Acceptance:
- A `to.dependencyKind: inherits` rule fires on a base-type edge in the ArchUnitNET `TestAssembly` fixture.
- `config convert` to dependency-cruiser reports each dropped cross-language attribute.

Source: [design § Dependency rules](artifacts/design.md#dependency-rules-the-whole-of-dependency-cruiser-1820) | Wave: 2 | ADRs: [0014](adr/0014-no-invented-cross-language-edges.md)

#### FR-RULE-03

**Element rules with the full ArchUnitNET vocabulary.** `select: { kind, where } ... should` MUST support ArchUnitNET's eight selectors plus `function` and `module`; every predicate and condition in the coverage tab as a camelCase key; `all`, `any`, `not` as `And`, `Or` and the negated forms; a value that is a name, a regex, a list or a nested selector (`...TypesThat`); `because`; and `allowEmpty` (default false). A predicate the language cannot answer MUST be a validation error (exit 3), never a silent false; where the concept exists it MUST answer cross-language (decorators for `haveAnyAttributes`, `export` or a leading underscore for `arePublic`).

Acceptance:
- Conformance gate 2: every test under `ArchUnitNETTests/Fluent/Syntax/Elements/**` and NetArchTest's test project is ported as a data-driven case; the unported count is zero except custom predicates.
- `areSealed` on `kind: class` with a Python-only selection exits 3.

Source: [design § Element rules](artifacts/design.md#element-rules-archunitnet-declarative), [ArchUnitNET coverage](artifacts/archunitnet-0.13.4-coverage.md) | Wave: 2 | ADRs: [0009](adr/0009-conformance-suites-as-specification.md), [0014](adr/0014-no-invented-cross-language-edges.md)

#### FR-RULE-04

**Slice rules.** `matching` MUST accept ArchUnitNET's pattern syntax with `(*)` and `(**)` (`MatchingWithPackages`), as a namespace pattern for .NET, a dotted module pattern for Python and a path pattern for TypeScript. `should` MUST accept `notDependOnEachOther` and `beFreeOfCycles`; `ignore` and `where` MUST filter slices.

Acceptance:
- The RiverBooks oracle's slice tests pass or fail as `dotnet test` reports.
- `apps/(*)/` with `notDependOnEachOther` reproduces the reference monorepo's `no-cross-app-imports` violations.

Source: [design § Slice rules](artifacts/design.md#slice-rules), [ArchUnitNET coverage § Slices](artifacts/archunitnet-0.13.4-coverage.md#slices-sliceruledefinition) | Wave: 2 | ADRs: [0009](adr/0009-conformance-suites-as-specification.md)

#### FR-RULE-05

**Diagram rules and the `plantuml` reporter.** `adhereTo: <file>.puml` MUST parse a PlantUML component diagram, match types to components by `<<pattern>>` stereotype, and fail any dependency the diagram does not draw, exactly as `AdhereToPlantUmlDiagram`. Import exceptions (`IllegalDiagramException`, `ComponentIntersectionException`) MUST become config-lint errors with the same meaning. The `plantuml` reporter MUST write a component diagram from slices, types, namespaces or folders with `LimitDependencies`, `C4Style`, `FocusOn`, `IncludeDependenciesToOther` and `DependencyFilters`.

Acceptance:
- ArchUnitNET's PlantUML tests are ported in gate 2.
- A diagram generated by the reporter and then enforced with `adhereTo` on the same graph yields zero violations.

Source: [design § Diagram rules](artifacts/design.md#diagram-rules), [ArchUnitNET coverage § PlantUML](artifacts/archunitnet-0.13.4-coverage.md#plantuml) | Wave: 2-3 | ADRs: [0009](adr/0009-conformance-suites-as-specification.md)

#### FR-RULE-06

**Ratchets as config.** `rules.ratchets` MUST accept `name`, `from`, `to` and a `budget` file. `count --from <regex> --to <regex> [--budget file] [--write]` MUST count matching direct edges and apply the ratchet rule: a count above the ceiling fails, `--write` lowers the ceiling, and a `--write` that would raise it MUST refuse. A missing budget file and an implausible zero MUST fail hard.

Acceptance:
- The reference monorepo's hand-written ratchet script is replaced by one `rules.ratchets` entry with the same count.
- `count --write` with a higher count exits non-zero and leaves the file unchanged.

Source: [design § The native format](artifacts/design.md#the-native-format), [§ The subcommands a guard reaches for](artifacts/design.md#the-subcommands-a-guard-reaches-for) | Wave: 1 | ADRs: [0021](adr/0021-agent-surface-cli-first.md)

#### FR-RULE-07

**`layers` and `independence` shorthands; import-linter mapping.** `layers` MUST expand to one `forbidden` rule per lower-to-higher pair and `independence` to one `$1` fence, for any language; `config expand` MUST print the expansion. import-linter contract kinds MUST map one to one: `forbidden` to a `forbidden` rule, `layers` to the shorthand, `independence` to the shorthand or a slice rule, `protected` to an `allowed` rule, `acyclic siblings` to `beFreeOfCycles` or `circular: true` with `via`, `ignore_imports` to `knownViolations` with `shrink-only`.

Acceptance:
- Every Python oracle's contracts, imported, reproduce each contract's broken-import list.
- `config expand` on a three-layer `layers` block prints three `forbidden` rules.

Source: [design § Shorthands](artifacts/design.md#shorthands), [§ import-linter contracts](artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them) | Wave: 1-2 | ADRs: [0005](adr/0005-native-config-superset-and-compat.md)

#### FR-RULE-08

**Graph analyses.** Cycles MUST come from Tarjan's strongly connected components with the path enumerated, at module and folder scope; reachability (`reachable`, `reaches`, `via`, `viaOnly`, `viaNot`, `viaSomeNot`) from breadth-first search from the `from` matches with `maxDepth`; dependents, orphans, instability and folder metrics (afferent and efferent couplings) from the consolidated folder graph; `moreUnstable` and `metrics` for modules, folders and .NET projects.

Acceptance:
- Conformance gate 1 layer 2: `test/graph-utl` specs pass unmodified.
- The reference monorepo's `to.circular: true` rule reports the same cycles with the same `cycle[]` paths.

Source: [design § The five stages](artifacts/design.md#the-five-stages) | Wave: 1 | ADRs: [0009](adr/0009-conformance-suites-as-specification.md)

#### FR-RULE-09

**`knownViolations` baseline and `baseline`.** `knownViolations[]` MUST be keyed by the stable id and MUST accept optional `expires`, `owner` and `reason` per entry. `baseline --baseline-mode full|shrink-only|format` MUST match `depcruise-baseline`; `shrink-only` MUST fail when a baselined entry no longer occurs. `--ignore-known [file]` and `--no-ignore-known` MUST be honoured.

Acceptance:
- A baseline survives an unrelated edit above the import (line churn) without change.
- `shrink-only` on a baseline with one resolved entry exits non-zero and names it.

Source: [coverage § Options](artifacts/dependency-cruiser-18.2.0-coverage.md#options), [§ Command line](artifacts/dependency-cruiser-18.2.0-coverage.md#command-line) | Wave: 2 | ADRs: [0015](adr/0015-stable-violation-id.md)

#### FR-RULE-10

**Linear-time regex with strict compatibility.** All pattern matching MUST use a linear-time engine. A pattern safe-regex would reject MUST be accepted, with a warning naming the rule in compatibility mode; `--strict-compat` MUST refuse it with exit 3. Captures MUST be substituted escaped so a captured segment can never become a wildcard. Lookaround and backreferences MUST be reported as exit 3 with the offending rule, and the JavaScript-to-Rust syntax differences MUST be enumerated in a published compatibility table.

Acceptance:
- The compatibility table is a test fixture in `rb-rules`.
- A nested-quantifier pattern runs under compat mode with a warning and exits 3 under `--strict-compat`.

Source: [design § The dependency-cruiser format](artifacts/design.md#the-dependency-cruiser-format), [§ The rule file](artifacts/design.md#the-rule-file) | Wave: 1 | ADRs: [0016](adr/0016-linear-time-regex-and-strict-compat.md)

### TypeScript extraction

#### FR-EXT-TS-01

**`oxc_parser` extracts every dependency form.** The TypeScript extractor MUST parse JS, TS, JSX, TSX, `.mjs`, `.cjs`, `.mts`, `.cts`, `.d.ts`, decorators and stage-3 syntax, and MUST read every dependency form dependency-cruiser knows: ES imports and re-exports, `import type`, `import()`, `require`, AMD `define` and `require`, exotic require names, `import =`, triple-slash directives, JSDoc imports and `process.getBuiltinModule`, with the same `dependencyTypes` and `moduleSystem` values. Parity MUST be measured against the 546 fixtures in dependency-cruiser's `test/extract`.

Acceptance:
- Wave 0 exit: `test/extract` fixtures pass at 95% or better; wave 1 exit: gate 1 layer 1 green (byte-compared modules and dependencies).
- The forty dependency types and four module systems in the coverage tab are all produced by at least one fixture.

Source: [design § What each extractor has to get right](artifacts/design.md#what-each-extractor-has-to-get-right), [coverage § Extraction and resolution](artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution) | Wave: 0-1 | ADRs: [0012](adr/0012-oxc-for-typescript.md), [0009](adr/0009-conformance-suites-as-specification.md)

#### FR-EXT-TS-02

**`oxc_resolver` honours every resolution option.** Resolution MUST honour `enhancedResolveOptions` (`exportsFields`, `conditionNames`, `extensions`, `mainFields`, `mainFiles`, `aliasFields`; `cacheDuration` accepted and ignored), tsconfig `paths`, `baseUrl`, `extends` and project references, webpack `resolve.alias` and `modules`, `babel-plugin-module-resolver` aliases, workspaces, symlinks (`preserveSymlinks`), Yarn PnP (`externalModuleResolutionStrategy`), `combinedDependencies`, and `tsPreCompilationDeps` as `true`, `false` or `"specify"`. Matchers MUST hit the resolved path so an alias, a workspace name and a relative import are one edge.

Acceptance:
- The alias `dependencyTypes` (`aliased-tsconfig-paths`, `aliased-webpack`, `aliased-workspace`, and the rest) are reported per the table that resolved the specifier.
- FluidFramework (pnpm workspace) and langfuse (Next.js) zero-diff at pinned commits.

Source: [coverage § Options](artifacts/dependency-cruiser-18.2.0-coverage.md#options) | Wave: 1 | ADRs: [0012](adr/0012-oxc-for-typescript.md)

#### FR-EXT-TS-03

**npm classification and core modules.** The extractor MUST classify `npm`, `npm-dev`, `npm-peer`, `npm-optional`, `npm-bundled`, `npm-no-pkg`, `npm-unknown` from the nearest `package.json` (monorepo walk-up), read `license` and `deprecated` from the installed package, detect core modules per Node version with the `node:` protocol and `process.getBuiltinModule` (`detectProcessBuiltinModuleCalls`), and honour `builtInModules.add` and `override`.

Acceptance:
- `test/extract` fixtures covering npm and core classification are byte-identical.
- `to.license` and `to.licenseNot` rules fire on the licence fixture.

Source: [coverage § Extraction and resolution](artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution) | Wave: 1 | ADRs: [0012](adr/0012-oxc-for-typescript.md)

#### FR-EXT-TS-04

**Vue, Svelte and Markdown.** `.vue` and `.svelte` files MUST have their `<script>` blocks split and parsed with `oxc`; Markdown code fences MUST be scanned when `.md` is listed in `extraExtensionsToScan`; `detectJSDocImports` MUST work on the comment table.

Acceptance:
- n8n (Vue) parses in the nightly scale run with no exit 2.
- The `extraExtensionsToScan` and JSDoc fixtures in `test/extract` are byte-identical.

Source: [coverage § Extraction and resolution](artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution) | Wave: 2 | ADRs: [0012](adr/0012-oxc-for-typescript.md)

#### FR-EXT-TS-05

**CoffeeScript and LiveScript via sidecar.** `.coffee`, `.litcoffee`, `.ls`, `.cjsx` and `.csx` MUST be handled by `--sidecar node`, which spawns dependency-cruiser for those files and merges the edges, marked `sidecar` on the edge. Without the flag, such a file MUST produce exit 2 with a named reason, never a silent skip.

Acceptance:
- The CoffeeScript specs listed in `conformance/excluded.json` pass with the sidecar in wave 3 and the list is then empty.
- The report records that Node was spawned.

Source: [coverage § Extraction and resolution](artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution) | Wave: 3 | ADRs: [0017](adr/0017-coffeescript-livescript-sidecar.md)

### .NET extraction

#### FR-EXT-DN-01

**MSBuild discovery and loader options.** Discovery MUST read `.sln` and `.slnx`, every `.csproj`, `Directory.Build.props` and `Directory.Packages.props`, honouring `IsTestProject`, `OutputPath` and `TargetFramework`, yielding project nodes and `ProjectReference`/`PackageReference` edges without building. `languages.dotnet` MUST accept `solution`, `assemblies` (globs), `directories` with `filter`, `namespaces`, `includeDependencies`, `configuration` and `excludeProjects`, covering every ArchUnitNET `ArchLoader` method.

Acceptance:
- Practical.CleanArchitecture (many projects) discovers every project and its built assembly; a solution with no built assemblies exits 2.
- Each `ArchLoader` row in the coverage tab has a fixture.

Source: [design § One engine, three languages](artifacts/design.md#one-engine-three-languages-one-monorepo), [ArchUnitNET coverage § Loader](artifacts/archunitnet-0.13.4-coverage.md#loader-and-caches) | Wave: 0-2 | ADRs: [0011](adr/0011-read-dotnet-assemblies-not-source.md)

#### FR-EXT-DN-02

**ECMA-335, IL and portable PDB reader.** The extractor MUST read the metadata tables, scan IL operands and read the portable PDB's `Document` and `MethodDebugInformation` tables to map every type and method to a source file and line. A body call, a generic instantiation, an attribute, a base type, an interface, a parameter type and a `typeof` MUST each be an edge with a `dependencyKind`. A type's fields and attributes MUST be attributed to the document of its first constructor or method; a type with no PDB row MUST be attributed by naming convention and flagged `attribution: inferred`; an unattributable type is `attribution: none` and path rules skip it with a warning. A non-portable PDB MUST exit 2. If the wave-0 spike cannot attribute at least 99% of the types in the .NET oracle repos within three weeks, the extractor MUST fall back to a C# `dotnet tool` writing the same document.

Acceptance:
- Wave 0 exit: 99% of the oracle repos' types attributed to a source file, or the fallback invoked.
- Fuzz target for the metadata reader runs nightly; malformed input exits 2, never panics.

Source: [design § What each extractor has to get right](artifacts/design.md#what-each-extractor-has-to-get-right), [§ The fallback](artifacts/design.md#the-fallback-decided-now-rather-than-under-pressure) | Wave: 0-2 | ADRs: [0011](adr/0011-read-dotnet-assemblies-not-source.md), [0003](adr/0003-dotnet-extractor-fallback.md)

#### FR-EXT-DN-03

**.NET code layer.** The code layer for .NET MUST carry everything ArchUnitNET reads: types, members, attributes with arguments, visibility (all six C# levels), sealed, abstract, record, static, readonly, virtual, getters and setters with their visibility, return types, calls and body dependencies, base types and interfaces, nesting.

Acceptance:
- Conformance gate 2 over the committed `TestAssembly` fixture: every ported predicate and condition answers correctly.
- Each .NET oracle's imported tests agree with `dotnet test`.

Source: [design § One engine, three languages](artifacts/design.md#one-engine-three-languages-one-monorepo) | Wave: 2 | ADRs: [0011](adr/0011-read-dotnet-assemblies-not-source.md)

#### FR-EXT-DN-04

**`--mode source` for .NET.** A source mode over `tree-sitter-c-sharp` MUST give namespace-level edges from `using` directives and qualified names in under a second, marked `approximate` in the document; compiled mode MUST remain the gate and source mode MUST never be used for the CI exit code.

Acceptance:
- Stop-hook p95 under 2 s on aspnetcore in source mode (wave 3 exit).
- A report from source mode carries `approximate` and `attest` refuses to sign it as a gate run.

Source: [design § Where it would be ignored](artifacts/design.md#where-it-would-be-ignored), [§ The architect's hat](artifacts/design.md#the-architects-hat-across-repos-and-across-time) | Wave: 3 | ADRs: [0011](adr/0011-read-dotnet-assemblies-not-source.md)

### Python extraction

#### FR-EXT-PY-01

**`ruff_python_parser` and resolution.** The Python extractor MUST parse with `ruff_python_parser`, read `import`, `from ... import`, `__all__` and literal `importlib.import_module` / `__import__`, and resolve absolute imports against the discovered roots, relative imports against the file's package, then the bundled `sys.stdlib_module_names` snapshot for `languages.python.version`, then installed distributions, then `unresolved`. A `TYPE_CHECKING`-guarded import MUST be `type-only`; a literal dynamic import MUST be `dynamic`. `__init__.py` stands for its package.

Acceptance:
- Every Python oracle's contracts reproduce (wave 2 exit); home-assistant/core (namespace packages, dynamic imports) runs in the scale table.
- Changing `languages.python.version` reclassifies a module that entered or left the stdlib.

Source: [design § What each extractor has to get right](artifacts/design.md#what-each-extractor-has-to-get-right) | Wave: 2 | ADRs: [0013](adr/0013-ruff-parser-for-python.md)

#### FR-EXT-PY-02

**Python code layer.** The code layer for Python MUST carry classes, functions, methods, decorators, bases, `@property`, `@staticmethod`, `@classmethod`, `@dataclass` (with `frozen=True` as `areImmutable`), and underscore visibility, so element rules with cross-language predicates answer.

Acceptance:
- `kind: function` with `arePublic` distinguishes `_helper` from `helper` in a fixture.
- `haveAnyAttributes` reads decorators in a fixture; `areSealed` on Python exits 3.

Source: [design § One engine, three languages](artifacts/design.md#one-engine-three-languages-one-monorepo) | Wave: 2 | ADRs: [0013](adr/0013-ruff-parser-for-python.md), [0014](adr/0014-no-invented-cross-language-edges.md)

### Outputs

#### FR-OUT-01

**All 21 dependency-cruiser output types.** Every output type MUST exist under its own name: `err`, `err-long`, `err-html`, `json`, `text`, `csv`, `teamcity`, `azure-devops`, `dot`, `ddot`, `cdot`/`archi`, `fdot`/`flat`, `x-dot-webpage`, `mermaid`, `d2`, `html`, `markdown`, `anon`, `baseline`, `metrics`, `null`, plus `plugin:<path>` through the embedded engine. Every `reporterOptions` key in the coverage tab (`collapsePattern`, `filters`, `showMetrics`, `theme`, the 18 markdown keys, `mermaid.minify`, `metrics.orderBy`, `anon.wordlist`, `text.highlightFocused`, the `err` unresolved flags) MUST be honoured.

Acceptance:
- Conformance gate 1 layer 3: every `test/report/<reporter>` fixture byte-compared through `fmt --from dependency-cruiser`, version string normalised; all 21 by the wave 3 exit.
- `err-long` prints the `fix` under each finding.

Source: [design § Reporters](artifacts/design.md#reporters), [coverage § Output types](artifacts/dependency-cruiser-18.2.0-coverage.md#output-types) | Wave: 1-3 | ADRs: [0009](adr/0009-conformance-suites-as-specification.md)

#### FR-OUT-02

**New reporters.** `agent` MUST emit JSON shaped for a model: each violation with from, to, line, the member reference that formed the edge, `fix` and decision link, sorted by estimated fix cost with the cost shown, and `--max-findings N` grouping by rule with counts. `github-annotations` MUST emit inline review annotations. `sarif` MUST emit one SARIF rule per config rule with the comment as help text, the `fix` as the recommendation and `partialFingerprints` from the stable id. `junit` and `trx` MUST emit one test case per rule with the `fix` as the message. `plantuml` is specified in FR-RULE-05.

Acceptance:
- `sarif` uploads cleanly to GitHub code scanning in the .NET pipeline example; fingerprints are stable across two runs.
- `agent --max-findings 3` on a 20-violation fixture shows three per rule with counts.

Source: [design § Reporters](artifacts/design.md#reporters), [§ Precision an agent can act on](artifacts/design.md#precision-an-agent-can-act-on) | Wave: 1-3 | ADRs: [0015](adr/0015-stable-violation-id.md), [0021](adr/0021-agent-surface-cli-first.md)

#### FR-OUT-03

**`json --strict-schema`.** `--strict-schema` MUST strip every addition and the result MUST validate against the pinned 18.2.0 `cruise-result` schema.

Acceptance:
- Gate 1 layer 4 runs on every `json` fixture with `--strict-schema`.

Source: [coverage § Output types](artifacts/dependency-cruiser-18.2.0-coverage.md#output-types) | Wave: 1 | ADRs: [0004](adr/0004-graph-document-is-cruise-result-superset.md)

### Command line

#### FR-CLI-01

**The subcommands a guard reaches for.** `cruise` MUST extract, evaluate and report. `fmt <json>` MUST accept every `depcruise-fmt` flag (`-f`, `-T`, `-I`, `-F`, `-x`, `-S`, `-e`, `-p`, `--highlight`, `--exit-code`, `--output-type`, `--include-only`, `--focus`, `--reaches`, `--collapse`, `--prefix`) and `--from dependency-cruiser`. `rules --json` MUST list every rule with `name`, `family`, `severity`, `comment`, `fix`, `from`, `to`, `select`, `fromMatches`, `toMatches`, `violations`. `count` is FR-RULE-06. `diff <old> <new>` MUST print added and removed edges and new violations; `--base main` MUST do the same against the base branch's cruise. `explain <rule>` MUST print the rule, its `fix`, what it matched on both sides and the first ten edges; `--plain` MUST render one deterministic English sentence from a template. `baseline` is FR-RULE-09. `test` MUST assert every rule's `examples` against a synthetic graph; `--generate` MUST write `examples` from current matches.

Acceptance:
- The reference monorepo's ADR guard and severity pin test run on `rules --json` instead of `require`-ing the config.
- `explain --plain` output is byte-stable across runs and is what `docs --format agents-md` embeds.

Source: [design § The subcommands a guard reaches for](artifacts/design.md#the-subcommands-a-guard-reaches-for) | Wave: 1-2 | ADRs: [0021](adr/0021-agent-surface-cli-first.md)

#### FR-CLI-02

**Questions an agent asks, and docs derived from rules.** `can-import <from> <to>` MUST answer yes or no with the deciding rule from the cached graph. `place --imports a,b --imported-by c --language ts` MUST list the directories where a new module with those edges would be legal. `impact <file>` MUST print the rules that mention the file, its dependents to depth N, whether it sits on a cycle, and which ratchets its edges count toward. `propose --from <glob> --to <glob>` and `--select <kind> --where <predicate>` MUST draft a rule with current match counts on both sides and the edges it would flag; `--from-example "a -> b"` MUST generalise one edge to the narrowest covering rule. `docs --format agents-md|contributing|skill` MUST render the rule file as an `AGENTS.md` section, a "what does this error mean" table, or a Claude Code `SKILL.md`, with `--verify` failing a stale copy. `decisions` MUST list rule-to-decision links, fail a dangling one, and `decisions new` MUST scaffold a record with enforcement ids filled in.

Acceptance:
- `can-import` answers in milliseconds from the cache (NFR-PERF-03).
- `docs --verify` fails in CI when the committed `AGENTS.md` section is stale.
- `propose` on semantic-kernel's obvious layers proposes rules with non-zero matches on both sides (wave 2 exit).

Source: [design § Questions an agent can ask](artifacts/design.md#questions-an-agent-can-ask-before-it-writes-the-import), [§ Docs derived from the rules](artifacts/design.md#docs-derived-from-the-rules-never-written-beside-them), [§ Rules an agent writes](artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar) | Wave: 1-2 | ADRs: [0021](adr/0021-agent-surface-cli-first.md)

#### FR-CLI-03

**First run, brownfield adoption, hooks, attestation.** `init` MUST read the repo before it asks anything (languages, `apps/` and `packages/` split, layered namespaces, `src/features/*`) and MUST write a starter config with every rule commented and passing or baselined. `adopt` MUST write the baseline (`knownViolations` with `expires` and an owner per entry), the CI step, the hook and `docs/architecture/rulebearing.md` in one pull request that is green on day one. `hooks install --claude-code` MUST write three hooks: SessionStart injecting a token-budgeted architecture brief (`summary --format agent`), PreToolUse on Edit and Write running `impact`, Stop running `cruise --affected HEAD --output-type agent`. `attest` MUST write a receipt (config hash, inputs hash, results hash, tool version) to `.graph/attest.json` that CI verifies against `HEAD`.

Acceptance:
- `init` produces a passing config on every greenfield test bed, committed as a fixture (wave 1 and 2 exits).
- `adopt` opens a green pull request on a repo with a non-empty baseline (wave 1 exit).
- A tampered `attest.json` fails CI verification.

Source: [design § The developer relations hat](artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo), [§ The agentic engineering hat](artifacts/design.md#the-agentic-engineering-hat-turn-two), [§ The five to fund first](artifacts/design.md#the-five-to-fund-first) | Wave: 1 | ADRs: [0021](adr/0021-agent-surface-cli-first.md)

#### FR-CLI-04

**Importers.** `import archunit` MUST read an ArchUnitNET or NetArchTest test project and emit element rules with the C# kept as a comment. `import import-linter` MUST read `.importlinter` or the `[tool.importlinter]` table. `import eslint` MUST read `import/no-restricted-paths` and `eslint-plugin-boundaries` configs.

Acceptance:
- Every .NET oracle's imported tests pass or fail exactly as `dotnet test` reports (gate 2 live diff).
- Every Python oracle's contracts reproduce after import (wave 2 exit).

Source: [design § The developer relations hat](artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo) | Wave: 2 | ADRs: [0009](adr/0009-conformance-suites-as-specification.md)

#### FR-CLI-05

**Cache, affected, watch.** `--cache [folder]`, `--cache-strategy metadata|content`, `--no-cache` and `cache.compress` MUST match dependency-cruiser; the .NET cache MUST key on assembly and PDB hashes. `--affected [revision]` MUST compute the changed files' dependent closure from the cache; for .NET, changed `.cs` files MUST map through the PDB to affected types. The cache MUST be worktree-aware (keyed by worktree and `HEAD`) so parallel agents do not share stale graphs, and `diff --base` MUST work across worktrees. `guard --watch` MUST re-check a saved file within 100 ms and write findings to a file the Stop hook reads.

Acceptance:
- `--affected` on a one-file change in the reference monorepo evaluates only that file's closure and finishes within the 2 s hook budget.
- Two worktrees on different `HEAD`s keep separate cache entries; verified by a test.

Source: [coverage § Options](artifacts/dependency-cruiser-18.2.0-coverage.md#options), [design § The agentic engineering hat](artifacts/design.md#the-agentic-engineering-hat-turn-two) | Wave: 2-3 | ADRs: [0021](adr/0021-agent-surface-cli-first.md)

#### FR-CLI-06

**MCP and LSP servers.** `serve --mcp` MUST expose `rules`, `explain`, `can_import`, `place`, `impact`, `count`, `query` (edges matching a from and to pair) and `diff` over the cached graph, as a thin loop over the query commands in the same binary. `serve --lsp` MUST publish the same findings as editor diagnostics with the `fix` as the quick-fix title, over the same incremental graph.

Acceptance:
- Each MCP tool returns the same answer as its CLI counterpart on a fixture; tested pairwise.
- Neither server has its own rule file or reads anything the CLI does not.

Source: [design § Hooks, test runners, an MCP server, an LSP](artifacts/design.md#hooks-test-runners-an-mcp-server-an-lsp) | Wave: 3 | ADRs: [0021](adr/0021-agent-surface-cli-first.md)

#### FR-CLI-07

**Rule lifecycle.** Rules MUST accept `since`, `deprecated` and `replacedBy`; `rules --unused` MUST list rules with zero matches on both sides for N releases. `snapshot` MUST write a small per-release summary JSON (counts, instability per folder or project, violations). `changelog --since <rev>` MUST print the architecture diff between two revisions in words: new edges across boundaries, retired rules, ratchets that fell.

Acceptance:
- `changelog --since` between two committed snapshots of this repository prints the expected paragraph; fixture-tested.

Source: [design § The architect's hat](artifacts/design.md#the-architects-hat-across-repos-and-across-time), [§ The developer relations hat](artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo) | Wave: 3 | ADRs: [0021](adr/0021-agent-surface-cli-first.md)

#### FR-CLI-08

**dependency-cruiser CLI parity.** The CLI MUST accept positional files, directories and globs; `--config`/`--validate`, `--no-config`, `--config-format`, `--config -` (stdin); `--init` with per-language presets; `--info` per extractor; `--output-type`, `--output-to`; `--include-only`, `--focus`, `--focus-depth`, `--reaches`, `--highlight`, `--collapse`, `--exclude`, `--do-not-follow`, `--max-depth`, `--module-systems`, `--prefix`; `--ts-pre-compilation-deps`, `--ts-config`, `--webpack-config`, `--preserve-symlinks`; `--metrics`/`--no-metrics`; `--progress [type]`/`--no-progress`; `--version`, `--help`; and `wrap-html` for `depcruise-wrap-stream-in-html`.

Acceptance:
- `--help` is byte-compared against a committed fixture for the shared flags.
- Conformance gate 1's `test/cli` expectations pass for every flag listed as Parity in wave 1.

Source: [coverage § Command line](artifacts/dependency-cruiser-18.2.0-coverage.md#command-line) | Wave: 1-3 | ADRs: [0008](adr/0008-exit-code-contract.md)

### Distribution

#### FR-DIST-01

**One binary, every registry.** Rulebearing MUST be published as `rulebearing` on npm (platform binaries under `optionalDependencies`), `Rulebearing` on NuGet (`dotnet tool` carrying the binary under `runtimes/`), `rulebearing` on PyPI (a wheel per platform), `rulebearing` on crates.io, GitHub Releases (macOS arm64 and x64, Linux x64, arm64 and musl, Windows x64) and a GitHub Action. A placeholder `0.0.1` MUST be published to all four registries on the same day in wave 0. Release MUST be one tag with all wrappers at the same version.

Acceptance:
- Wave 0: `0.0.1` visible on npm, NuGet, PyPI and crates.io; repository and organisation created.
- `npx rulebearing --version`, `dotnet tool run rulebearing --version` and `pipx run rulebearing --version` print the same version.

Source: [design § Language decision](artifacts/design.md#language-decision), [§ Open questions](artifacts/design.md#open-questions) | Wave: 0-2 | ADRs: [0002](adr/0002-rust-as-implementation-language.md), [0020](adr/0020-single-name-across-registries.md)

#### FR-DIST-02

**`rb-node` binding.** A napi-rs binding MUST expose `cruise(files, options, resolveOptions, transpileOptions)`, `format(result, options)`, `extractDepcruiseConfig`, `extractTSConfig`, `extractWebpackResolveConfig`, `extractBabelConfig`, `getAvailableTranspilers` and `allExtensions` with dependency-cruiser's signatures, for scripts that call dependency-cruiser as a library today.

Acceptance:
- A script written against `dependency-cruiser`'s API runs against `rulebearing` with only the import changed; fixture in `crates/rb-node/tests`.

Source: [coverage § Programmatic API](artifacts/dependency-cruiser-18.2.0-coverage.md#programmatic-api) | Wave: 3 | ADRs: [0002](adr/0002-rust-as-implementation-language.md)

#### FR-DIST-03

**Test adapters.** `Rulebearing.TestAdapter` MUST provide a data source for xUnit v2 and v3, NUnit, MSTest v2 and v4 and TUnit that runs the binary and yields one test per rule with the `fix` in the failure message. `pytest-rulebearing` and `rulebearing/vitest` MUST do the same for Python and TypeScript. Adapters read the JSON and have no rule logic of their own.

Acceptance:
- A rule failure appears in the test tab of each framework's runner with the `fix` text; one fixture project per framework under `adapters/`.
- 70% line coverage per adapter (NFR-QUAL-01).

Source: [ArchUnitNET coverage § Test framework adapters](artifacts/archunitnet-0.13.4-coverage.md#test-framework-adapters), [design § Hooks, test runners](artifacts/design.md#hooks-test-runners-an-mcp-server-an-lsp) | Wave: 2 | ADRs: [0021](adr/0021-agent-surface-cli-first.md)

#### FR-DIST-04

**ESLint plugin and Roslyn analyzer.** `eslint-plugin-rulebearing` MUST provide one rule, `rulebearing/boundaries`, that asks the cached graph `can-import` for each import statement and reports inline. `Rulebearing.Analyzer` MUST read `rulebearing.yaml` and evaluate dependency and element rules on the semantic model at compile time, reporting `RB0001`-style diagnostics with the `fix` as the message. Both MUST read the same config and graph as the gate so they cannot disagree with it.

Acceptance:
- On a fixture, every diagnostic the analyzer or plugin reports is also a gate violation, and vice versa for the rules they cover.

Source: [design § Two front-ends that will matter more than the MCP server](artifacts/design.md#two-front-ends-that-will-matter-more-than-the-mcp-server) | Wave: 2-3 | ADRs: [0021](adr/0021-agent-surface-cli-first.md)

### Reach

#### FR-REACH-01

**Playground and docs site.** A browser playground built from the same crates compiled to WebAssembly MUST let a user paste a config and a cruise JSON, or drop a small repo, and see the violations, the graph and `explain` output with no install. The docs site MUST embed it beside every rule kind and MUST carry the rules cookbook (one page per architecture question with the rule in both formats, the failing edge and the fix) and live coverage tables.

Acceptance:
- The playground runs the design's sample config against a bundled sample graph in a browser with no network call after load.

Source: [design § The developer relations hat](artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo) | Wave: 4 | ADRs: [0002](adr/0002-rust-as-implementation-language.md)

#### FR-REACH-02

**Pull-request app and Azure DevOps extension.** A GitHub app and an Azure DevOps extension MUST post one comment per run with new violations, the `fix`, an `explain` link and the architecture diff, updated in place.

Acceptance:
- On a fixture pull request, a second run edits the existing comment rather than adding one.

Source: [design § The developer relations hat](artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo) | Wave: 4 | ADRs: [0021](adr/0021-agent-surface-cli-first.md)

#### FR-REACH-03

**Fix plans, fleet, declared edges, eval harness, usage counts.** `fix --plan <rule>` MUST list, per violation, the three cheapest refactors that clear it as a machine-readable plan with the edges each touches. `fleet` MUST run over a workspace manifest of sibling repositories with per-repo and fleet-wide rules. Declared cross-service edges (`edges.yaml`, or read from OpenAPI clients) MUST be marked `declared`, never detected. An eval harness MUST score `fix` strings by whether an agent cleared the rule without widening it. Opt-in anonymous usage counts MUST be off by default and printed before sending.

Acceptance:
- A `declared` edge appears with the marker in `json` and is distinguishable from a detected edge in every reporter.
- No network call is made unless usage counts are opted in; sandbox test.

Source: [design § The agentic engineering hat](artifacts/design.md#the-agentic-engineering-hat-turn-two), [§ The architect's hat](artifacts/design.md#the-architects-hat-across-repos-and-across-time) | Wave: 4 | ADRs: [0014](adr/0014-no-invented-cross-language-edges.md)

#### FR-REACH-04

**Public rule library and framework presets.** `rulebearing-rules` MUST be a public repository published to all three registries that any repo `extends`, versioned with a changelog. Framework presets `rulebearing:nextjs`, `rulebearing:clean-architecture`, `rulebearing:django`, `rulebearing:fastapi` and `rulebearing:vertical-slices` MUST be off by default, each a documented opinion.

Acceptance:
- `extends: [rulebearing:clean-architecture]` on jasontaylordev/CleanArchitecture passes with non-vacuous rules.

Source: [design § The developer relations hat](artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo) | Wave: 3 | ADRs: [0005](adr/0005-native-config-superset-and-compat.md)

## Non-functional requirements

#### NFR-PERF-01

**Full-cruise speed.** A full cruise of the 5,500-module private TypeScript monorepo (5,574 modules, 19 MB JSON) MUST complete in 1 to 2 seconds, against 13 seconds with dependency-cruiser today, with the same `--metrics` flag, roots and output path.

Acceptance:
- Wall-clock on the reference monorepo recorded per release; above 2 s is a regression.

Source: [design § Three pipelines](artifacts/design.md#three-pipelines) | Wave: 1 | ADRs: [0002](adr/0002-rust-as-implementation-language.md), [0012](adr/0012-oxc-for-typescript.md)

#### NFR-PERF-02

**Stop-hook budget.** The p95 of the Stop hook running `cruise --affected HEAD --output-type agent` MUST be under 2 seconds, including on dotnet/aspnetcore in source mode. Above that, the hook gets disabled.

Acceptance:
- Wave 3 exit: p95 under 2 s on aspnetcore in source mode, measured in the nightly run.

Source: [design § How to know, rather than believe](artifacts/design.md#how-to-know-rather-than-believe) | Wave: 1-3 | ADRs: [0021](adr/0021-agent-surface-cli-first.md)

#### NFR-PERF-03

**Query latency and scale regression.** `can-import` MUST answer in milliseconds from the cached graph. `guard --watch` MUST re-check a saved file in under 100 ms. The nightly scale table (wall-clock and peak memory per scale repo) MUST be published in the README, and a regression over 20% MUST fail the nightly run.

Acceptance:
- Timing assertions in the nightly workflow; the README table regenerated nightly.

Source: [design § Questions an agent can ask](artifacts/design.md#questions-an-agent-can-ask-before-it-writes-the-import), [§ Test beds](artifacts/design.md#test-beds-open-source-repositories-to-validate-against) | Wave: 1-3 | ADRs: [0021](adr/0021-agent-surface-cli-first.md)

#### NFR-CONF-01

**Conformance gate 1.** dependency-cruiser 18.2.0's test suite MUST be a required, ratcheting CI check in five layers: extraction compared over `test/extract` (the design's 546 fixtures; at 18.2.0 the suite has 480 tests, of which 296 recorded cases exercise an extraction surface, accounted for in [conformance/README.md](../conformance/README.md)); `test/validate` and `test/graph-utl` specs run unmodified through a shim; reporters byte-compared over `test/report`; schema validation of every `json` and every accepted config; live zero-diff on the TypeScript oracle repos plus a mutation branch of dependency-cruiser's repo with twelve deliberate violations. `conformance/excluded.json` MUST list any excluded spec with a reason and MAY only shrink.

Acceptance:
- Required check from the first pull request; wave 1 exit: layers 1 to 5 green; wave 3 exit: `excluded.json` empty.

Source: [design § Conformance gate 1](artifacts/design.md#conformance-gate-1-dependency-cruisers-tests-validate-rulebearing) | Wave: 0-3 | ADRs: [0009](adr/0009-conformance-suites-as-specification.md)

#### NFR-CONF-02

**Conformance gate 2.** ArchUnitNET 0.13.4's `TestAssembly` MUST be built with a portable PDB and committed as a fixture; every test under `ArchUnitNETTests/Fluent/Syntax/Elements/**` and NetArchTest's test project MUST be ported as a data-driven case. The ported count MAY only rise. The .NET oracle repos' imported tests MUST agree with `dotnet test`.

Acceptance:
- Required check from the first pull request; wave 2 exit: unported count zero except custom predicates.

Source: [design § Conformance gate 2](artifacts/design.md#conformance-gate-2-archunitnets-test-assemblies-validate-the-element-rules) | Wave: 0-2 | ADRs: [0009](adr/0009-conformance-suites-as-specification.md)

#### NFR-CONF-03

**Nightly test-bed run.** A nightly workflow MUST clone every test bed at its pinned SHA and run: oracle zero-diff (dependency-cruiser oracles diff `summary.violations`; .NET oracles diff each imported rule's outcome against `dotnet test`; import-linter oracles diff each contract's broken-import list); greenfield `init` and `propose` with output committed as fixtures; and the scale timing table. Any difference MUST be a bug or a documented divergence with a reason.

Acceptance:
- The run is public; the manifest is `testbeds/manifest.yaml`; timing regressions fail it.

Source: [design § Test beds](artifacts/design.md#test-beds-open-source-repositories-to-validate-against) | Wave: 0-4 | ADRs: [0009](adr/0009-conformance-suites-as-specification.md)

#### NFR-QUAL-01

**Coverage floor, and what proves a test asserts anything.** 70% line coverage MUST be a required check per crate (`cargo llvm-cov --workspace --fail-under-lines 70`, reported per crate) and per wrapper, adapter and front-end (vitest thresholds, coverlet, `pytest --cov-fail-under=70`). Generated code is excluded by path. Coverage is a floor rather than a measure of test quality, so three further gates MUST hold: mutation testing over the contract crates with no surviving mutants, property tests on pure functions over open input spaces, and committed snapshots for anything a third party parses. Determinism MUST be asserted: the same graph, built twice, serialises to the same bytes.

Acceptance:
- Installed in wave 0 before any feature code; a crate below 70% fails even when the workspace is above.
- `cargo mutants` over `rb-model`, `rb-rules` and `xtask` reports no survivors; every exclusion in `.cargo/mutants.toml` carries its reason.
- The command-line help snapshot and the doc examples run in CI.

Source: user brief; [design § What Rust does not solve](artifacts/design.md#what-rust-does-not-solve) | Wave: 0 | ADRs: [0018](adr/0018-test-coverage-threshold.md), [0024](adr/0024-test-quality-gates.md)

#### NFR-QUAL-02

**Static checks and fuzzing.** `cargo fmt --check`, `cargo clippy -D warnings` and `cargo deny` (licences and advisories) MUST be required checks; fuzz targets for the metadata reader and the config parsers MUST run nightly. Every language in the repository MUST have a configured linter, reachable from one entry point: Rust (rustfmt, clippy), TypeScript and JavaScript (eslint with type-aware rules, prettier), Python (ruff, ruff format, mypy strict) and C# (.NET analyzers with warnings as errors, `dotnet format`).

Acceptance:
- `cargo xtask lint --strict` is a required check and runs every linter plus the documentation link check; a linter that is not installed fails the run rather than being skipped.
- Each language's configuration is committed at the repository root, and a language whose tree does not exist yet reports "not applicable" rather than passing.
- `fuzz/` targets in the nightly workflow.
- Workflows run least-privileged (`permissions: contents: read`), with every action pinned to a commit SHA, a timeout on every job and a concurrency group per branch.
- `cargo deny check licenses advisories bans sources` allows only crates.io; a git or private-registry dependency needs an ADR.
- The declared minimum Rust version is built in CI, and `cargo hack check --feature-powerset` builds every extractor feature combination.
- `typos`, `actionlint` and `shellcheck` run over the repository, the workflows and every committed script.

Source: [architecture § Verification strategy](architecture.md#verification-strategy) | Wave: 0 | ADRs: [0019](adr/0019-mit-licence.md), [0018](adr/0018-test-coverage-threshold.md), [0023](adr/0023-documentation-link-and-lint-gates.md), [0025](adr/0025-ci-and-supply-chain-hardening.md)

#### NFR-SEC-01

**Security posture.** The binary MUST make no outbound connection. Config evaluation MUST be sandboxed with no filesystem beyond the repository, no `process`, no timers; escaping the sandbox is a test that must fail. Assemblies, PDBs and source files MUST be parsed defensively: a malformed input exits 2 with a named reason, never a panic. Node MUST be spawned only with `--sidecar node` or `--config-via-node`, and the report MUST record it. Dependencies MUST be pinned and release binaries built in CI from a tag.

Acceptance:
- Sandbox-escape and network-denied tests in CI; fuzz targets nightly.

Source: [design § The dependency-cruiser format](artifacts/design.md#the-dependency-cruiser-format), [§ Rules an agent writes](artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar) | Wave: 1 | ADRs: [0006](adr/0006-embedded-quickjs-config-evaluator.md), [0017](adr/0017-coffeescript-livescript-sidecar.md)

#### NFR-COMPAT-01

**Licence.** Rulebearing MUST be MIT-licensed, matching dependency-cruiser and NetArchTest, so the conformance harness can vendor their fixtures. ArchUnitNET's Apache 2.0 fixtures MUST carry their `NOTICE` under `conformance/archunitnet/`. No GPL dependency (for example `dotnetdll`) may be added; `cargo deny` enforces the allow-list.

Acceptance:
- `cargo deny check licenses` green from wave 0; `LICENSE` and `NOTICE` present.

Source: [design § Open questions](artifacts/design.md#open-questions) | Wave: 0 | ADRs: [0019](adr/0019-mit-licence.md)

#### NFR-DOC-01

**Traceability.** Every code file, plan and ADR MUST carry linked references: a crate's `lib.rs` links its architecture section and plan; a plan links the PRD requirements, ADRs and design sections; an ADR links the design section it derives from and the architecture section it constrains. Plans MUST live in `plans/pending/` and move to `plans/implemented/` only when every exit criterion is met. This repository's own `rulebearing.yaml` MUST cite ADRs in every rule. Every relative link MUST resolve, both the file and, where the link carries one, the `#anchor`.

Acceptance:
- The link check runs inside every compile (`crates/rb-model/build.rs`), in `cargo xtask lint` and as its own CI job; a broken link fails the build, not only the pull request.
- Rust doc comments are checked as well as Markdown, so a renamed document breaks the crate that cites it.
- A plan move is a reviewed pull request showing the exit criteria met.

Source: [ADR-0001](adr/0001-record-architecture-decisions.md) | Wave: 0 | ADRs: [0001](adr/0001-record-architecture-decisions.md), [0023](adr/0023-documentation-link-and-lint-gates.md)

#### NFR-ADOPT-01

**Adoption signals.** From wave 1, on the repositories where agent-authored pull requests are visible, the six signals in [Success metrics](#success-metrics) MUST be measured monthly. If the first two do not move after two months, the agent surface MUST be cut back to the `agent` reporter and the hook. Wave 4 is funded only when all six are met for two consecutive months.

Acceptance:
- A dashboard or table per month, committed with the release notes.

Source: [design § How to know, rather than believe](artifacts/design.md#how-to-know-rather-than-believe) | Wave: 1-4 | ADRs: [0021](adr/0021-agent-surface-cli-first.md)

#### NFR-ADOPT-02

**Adoption order and upstream etiquette.** Adoption MUST proceed: own repositories (wave 0); TypeScript oracle repos as a drop-in offered upstream, dependency-cruiser's maintainer first (wave 1); .NET oracle repos through `import archunit`, starting with evolutionary-architecture-by-example and RiverBooks (wave 2); Python oracle repos through `import import-linter`, starting with kedro and sqlfluff (wave 2); greenfield mixed-language repos through `init` and `propose` (wave 3). Each offer MUST go to an issue first with the zero-diff result attached, and MUST be withdrawn without argument if declined.

Acceptance:
- Wave 1 exit: the drop-in offered upstream to at least one oracle repo; wave 4 exit: one greenfield maintainer accepting a proposed rule set.

Source: [design § Adoption order](artifacts/design.md#adoption-order), [§ Open questions](artifacts/design.md#open-questions) | Wave: 0-3 | ADRs: [0021](adr/0021-agent-surface-cli-first.md)

## Command-line surface

| Subcommand | Purpose | Requirement | Wave |
| --- | --- | --- | --- |
| `cruise` | extract, evaluate, report; `--output-type json` is the one call per pipeline | FR-CLI-01 | 1 |
| `fmt <json>` | re-report a saved JSON with `depcruise-fmt`'s flags and `--from dependency-cruiser` | FR-CORE-02, FR-CLI-01 | 1 |
| `rules --json`, `--unused` | list every rule with matches and violations; find dead rules | FR-CLI-01, FR-CLI-07 | 1, 3 |
| `count` | ratchet: count edges, `--write` lowers a ceiling only | FR-RULE-06 | 1 |
| `test`, `test --generate` | run a rule's `examples`; write them from current matches | FR-CLI-01 | 1, 2 |
| `explain <rule>`, `--plain` | rule, `fix`, matches, first ten edges; one English sentence | FR-CLI-01 | 1 |
| `can-import <from> <to>` | yes or no with the deciding rule | FR-CLI-02 | 1 |
| `config convert`, `lint`, `expand` | format translation, authoring mistakes, shorthand expansion | FR-CFG-05 | 1 |
| `init`, `adopt` | repo-aware starter config; brownfield baseline plus CI plus hook plus doc in one PR | FR-CLI-03 | 1 |
| `hooks install --claude-code` | SessionStart brief, PreToolUse `impact`, Stop affected cruise | FR-CLI-03 | 1 |
| `attest` | run receipt in `.graph/attest.json`, verified by CI | FR-CLI-03 | 1 |
| `baseline` | `--baseline-mode full`, `shrink-only`, `format` | FR-RULE-09 | 2 |
| `place`, `impact`, `propose` | where code may live; what an edit touches; draft a rule with counts | FR-CLI-02 | 2 |
| `docs --format agents-md`, `contributing`, `skill` | rule-derived documentation with `--verify` | FR-CLI-02 | 2 |
| `decisions`, `decisions new` | rule-to-decision links; scaffold a record | FR-CLI-02 | 2 |
| `import archunit`, `import-linter`, `eslint` | migrate existing rules | FR-CLI-04 | 2 |
| `diff <old> <new>`, `--base` | added and removed edges, new violations | FR-CLI-01, FR-CLI-05 | 3 |
| `guard --watch` | re-check on save in under 100 ms | FR-CLI-05 | 3 |
| `snapshot`, `changelog --since` | per-release summary; architecture diff in words | FR-CLI-07 | 3 |
| `serve --mcp`, `serve --lsp` | the query commands as tools; findings as diagnostics | FR-CLI-06 | 3 |
| `wrap-html` | `depcruise-wrap-stream-in-html` | FR-CLI-08 | 3 |
| `fix --plan`, `fleet` | cheapest refactors; multi-repo runs | FR-REACH-03 | 4 |

Global flags of note: `--config`, `--config-format`, `--config-via-node`, `--strict-compat`, `--no-liveness`, `--require-comment-token`, `--sidecar node`, `--mode source`, `--cache`, `--affected`, `--max-findings`, `--strict-schema`. Sources: [design § The subcommands a guard reaches for](artifacts/design.md#the-subcommands-a-guard-reaches-for), [§ Where each lands](artifacts/design.md#where-each-lands), [coverage § Command line](artifacts/dependency-cruiser-18.2.0-coverage.md#command-line).

## Reporters

| Reporter | Purpose | Wave |
| --- | --- | --- |
| `err`, `err-long`, `err-html`, `text`, `csv`, `json`, `null` | dependency-cruiser's terminal and data formats; `err-long` prints the comment and now the `fix` under each finding | 1 (`err-html` 2) |
| `teamcity`, `azure-devops`, `github-annotations` | inline review annotations; the last is new | 1 |
| `agent` | JSON for a model: from, to, line, member reference, `fix`, decision link; fix-cost ordered; `--max-findings` | 1 |
| `sarif` | code-scanning upload; one SARIF rule per config rule; stable `partialFingerprints` | 2 |
| `junit`, `trx` | one test case per rule with the `fix` as the message; what the adapters read | 2 |
| `dot`, `ddot`, `archi`/`cdot`, `flat`/`fdot`, `mermaid`, `d2`, `x-dot-webpage` | graphs with `collapsePattern`, `theme`, `filters`; `mermaid` renders in a pull request without Graphviz | 2 (`x-dot-webpage` 3) |
| `plantuml` | a component diagram from slices, folders, namespaces or types; the file a diagram rule can then enforce | 3 |
| `metrics`, `baseline`, `markdown`, `html`, `anon`, `plugin:<path>` | as dependency-cruiser | 2 to 3 |

Source: [design § Reporters](artifacts/design.md#reporters).

## Release plan

Five waves of part-time work at roughly ten hours a week, TypeScript first because the largest set of oracle repos is TypeScript. Exit criteria are quoted from [design § Waves](artifacts/design.md#waves); each wave has a plan.

| Wave | Weeks | Plan | Exit criterion |
| --- | --- | --- | --- |
| 0 spike | 4 | [0000-wave-0-spike](plans/pending/0000-wave-0-spike.md) | `test/extract` fixtures pass at 95% or better; 99% of the oracle repos' types attributed to a source file, or the C# extractor fallback is invoked |
| 1 TypeScript parity, the native format, the first-run experience | 10 | [0001-wave-1-typescript-parity](plans/pending/0001-wave-1-typescript-parity.md) | Gate 1 layers 1 to 5 green; zero-diff on dependency-cruiser's own repo, langfuse and FluidFramework at pinned commits; `adopt` opens a green pull request on a repo with a non-empty baseline; the drop-in offered upstream to at least one oracle repo |
| 2 .NET, Python, element rules, migration | 10 | [0002-wave-2-dotnet-python-element-rules](plans/pending/0002-wave-2-dotnet-python-element-rules.md) | Gate 2 unported count at zero except custom predicates; every .NET oracle's imported tests agree with `dotnet test`; every Python oracle's contracts reproduce; `init` produces a passing config on semantic-kernel and autogen |
| 3 operations, the rest of the surface, the inner loop | 8 | [0003-wave-3-operations-surface-inner-loop](plans/pending/0003-wave-3-operations-surface-inner-loop.md) | `conformance/excluded.json` empty; all twenty-one dependency-cruiser output types byte-compared; Stop hook p95 under 2 s on aspnetcore in source mode; the scale table published |
| 4 reach, funded on the numbers | 8 | [0004-wave-4-reach](plans/pending/0004-wave-4-reach.md) | The six adoption signals met for two consecutive months on the repos where they can be measured; one greenfield test bed maintainer accepting a proposed rule set |

Wave 4 is funded on the adoption numbers ([NFR-ADOPT-01](#nfr-adopt-01)), not in advance. Both conformance gates are required checks from the first pull request and ratchet ([NFR-CONF-01](#nfr-conf-01), [NFR-CONF-02](#nfr-conf-02)).

## Success metrics

Measured from wave 1 on the repositories where agent-authored pull requests are visible: the author's own, the private monorepo where dependency-cruiser runs today, and any test bed whose maintainers accept the drop-in ([design § How to know, rather than believe](artifacts/design.md#how-to-know-rather-than-believe)).

| Signal | Target after two months | Why this one |
| --- | --- | --- |
| Share of agent-authored pull requests whose first CI run passes the boundary gate | above 90%, from a baseline measured before the switch | The loop is working if violations are caught locally |
| Median time from a violation appearing to green, in agent turns | one turn | `fix` and line precision are doing their job |
| Rules added by agents that fail `rulebearing test` or liveness before merge | any number, as long as it is caught | The authoring guardrails are load-bearing |
| p95 of the Stop-hook run with `--affected` | under 2 seconds | Above that, the hook gets disabled |
| Rules carrying `fix` text | above 80% | The metadata is being written, not skipped |
| Budget-file edits that raise a ceiling | zero merged | The ratchet holds against the cheapest path |

If the first two numbers do not move, the tool is a better dependency-cruiser and nothing more, and the agent surface is cut back to the `agent` reporter and the hook.

| Gate | Metric | Required |
| --- | --- | --- |
| Conformance gate 1 | `test/extract` pass rate (95% wave 0, 100% wave 1); layers 2 to 5 green; `excluded.json` shrinking to empty by wave 3 | yes |
| Conformance gate 2 | ported test count rising to all but custom predicates by wave 2; oracle agreement with `dotnet test` | yes |
| Coverage | 70% lines per crate, wrapper, adapter and front-end | yes |
| Nightly | oracle zero-diff; greenfield fixtures unchanged; scale timing within 20% | nightly, failing |
| Coverage tabs | a row may say Parity only when the pinned suite says so | ledger |

## Open questions

| Question | Owner | Resolved in | Note |
| --- | --- | --- | --- |
| Name ownership: does `rulebearing` stay free until publication; is `@rulebearing` free on npm? | Ben Bahrenburg | Wave 0 | Placeholder `0.0.1` on all four registries on the same day; repository and organisation created then; `@rulebearing` checked from a signed-in session ([ADR-0020](adr/0020-single-name-across-registries.md)) |
| Licence: can the conformance harness vendor both suites' fixtures? | Ben Bahrenburg | Wave 0 | MIT; ArchUnitNET's Apache 2.0 fixtures carry their `NOTICE` ([ADR-0019](adr/0019-mit-licence.md)) |
| One maintainer: how does the project survive a stall? | Ben Bahrenburg | Wave 2 | Suites as reviewer, public nightly run; a second maintainer is the goal by the end of wave 2 |
| .NET Framework projects: how many oracle projects lack a portable PDB, and can `DebugType=portable` be set? | Ben Bahrenburg | Wave 0 | Measured in the spike; `attribution: none` handled |
| The sidecar: is it exercised by anything but dependency-cruiser's own fixtures? | Ben Bahrenburg | Wave 3 | No oracle repo uses CoffeeScript or LiveScript; the coverage tab says so ([ADR-0017](adr/0017-coffeescript-livescript-sidecar.md)) |
| Custom predicates: does any test bed's imported test need one the declarative vocabulary cannot express? | Ben Bahrenburg | Wave 2 | Revisit only on evidence; record the gap in the coverage tab |
| Upstream etiquette: how is a drop-in offered without costing maintainer time? | Ben Bahrenburg | Wave 1 | An issue first with the zero-diff result attached; withdrawn without argument if declined |

Source: [design § Open questions](artifacts/design.md#open-questions).

## Glossary

The full glossary is in [architecture § Glossary](architecture.md#glossary). The terms this document leans on:

| Term | Meaning |
| --- | --- |
| Cruise | one extraction plus evaluation run; the verb dependency-cruiser uses |
| Graph document | the JSON with the module layer (files and edges) and the code layer (types, members, attributes, calls) |
| Vacuous rule | a rule whose selecting side matches nothing; fails by default |
| Oracle, greenfield, scale repository | a test bed that carries a config and must zero-diff; one with no tool that exercises `init` and `propose`; one used only for timing |
| Parity, Parity+, Kept, Sidecar, Stays | the coverage-tab statuses: same name and semantics proven by upstream tests; also works for .NET and Python; TypeScript-only by nature; handled by spawning dependency-cruiser; remains in ArchUnitNET |
| Ratchet | a numeric ceiling that may only fall |
| Receipt | `summary.inspected`, the proof a run looked at something |
| Decision token | `adr:NNNN` or `plan:<slug>` in a rule's `comment` |
| Conformance gate | a required CI check that runs an upstream tool's own test suite against Rulebearing |
