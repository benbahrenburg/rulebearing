# Rulebearing

One architecture rule set for TypeScript, .NET and Python.

Rulebearing is a single command-line tool and one rule language that does what [dependency-cruiser](https://github.com/sverweij/dependency-cruiser) does for TypeScript and JavaScript, what [ArchUnitNET](https://github.com/TNG/ArchUnitNET) does for .NET, and what [import-linter](https://github.com/seddonym/import-linter) does for Python, over a single graph. A repository with more than one language keeps one set of architecture rules, one gate, and one answer to "may this file import that one".

> **Status: wave 0, the spike.** The workspace builds and every quality gate is wired and green, but no subcommand is implemented yet. Nothing is published to any registry. The [plans](docs/plans/README.md) say what lands when, and [the roadmap below](#roadmap) summarises them. Watch the repository rather than waiting for an announcement.

## Why this exists

A load-bearing wall is the one you cannot take out. A rule-bearing codebase is one whose rules carry real weight. The name is also the tool's most distinctive check: a rule that matches nothing bears nothing, and Rulebearing fails it by default.

Three good tools already enforce architecture, one per language. None of them reads the others' repositories, none of the .NET or Python ones emits a graph another script can consume, and none was designed for a coding agent as the reader of its findings. A GitHub search in September 2026 found 112 repositories carrying a dependency-cruiser config, 88 carrying a .NET architecture-test project and 109 carrying an import-linter contract, including much of the current generation of AI tooling. Those teams are doing the same work three times.

What carries over is not a tool but the practice around it, learned from running dependency-cruiser daily in a private 5,500-module monorepo:

- **A rule is a named, commented fence with a severity,** and its name is a stable id a decision record can cite.
- **Matchers hit the resolved target,** so a boundary cannot be dodged by changing import style.
- **The graph is a first-class artefact.** In that monorepo the cruise JSON is read by three other guards; a tool that only prints violations covers a third of the use.
- **Liveness is checked.** Four rules there matched zero files for months and read as standing fences.

## What it does

| | |
| --- | --- |
| **Reads** | TypeScript and JavaScript sources, built .NET assemblies with their portable PDBs, Python sources |
| **Produces** | One graph document: dependency-cruiser's `cruise-result` schema unchanged, plus a code layer of types, members, attributes and calls |
| **Enforces** | Dependency rules (dependency-cruiser's whole language), element rules (ArchUnitNET's vocabulary, declarative), slice rules, diagram adherence, and ratchets |
| **Reports** | Every dependency-cruiser output type, plus SARIF, GitHub annotations, JUnit, TRX and an output shaped for a coding agent |
| **Promises** | Your existing `.dependency-cruiser.*` config runs unchanged on day one |

**Superset, precisely.** Every rule attribute, option, flag, reporter and result field of dependency-cruiser 18.2.0, and every selector, predicate, condition, slice rule, loader option and PlantUML feature of ArchUnitNET 0.13.4, has a row in the coverage tables with its status. Nothing is dropped. The claim is measured, not asserted: the upstream projects' own test suites run against Rulebearing as required checks ([ADR-0009](docs/adr/0009-conformance-suites-as-specification.md)).

What neither incumbent has, and Rulebearing adds for all three languages: a line and column on every edge, `fix` text and worked examples on every rule, a stable violation id, a receipt of what was inspected, ratchets as configuration, and a rule that matches nothing failing the build.

```yaml
# rulebearing.yaml, abbreviated
rules:
  dependencies:
    forbidden:
      - name: no-cross-app-imports
        comment: "Apps share only packages/* and HTTP. adr:0003"
        fix: "Call the other app over its API, or move the shared code into packages/*."
        from: { path: "^apps/([^/]+)/" }
        to: { path: "^apps/([^/]+)/", pathNot: "^apps/$1/" }
  elements:
    - name: handlers-are-internal-and-sealed
      comment: "adr:0002"
      select: { kind: class, where: { haveNameEndingWith: Handler } }
      should: { all: [{ beInternal: true }, { beSealed: true }] }
```

## Roadmap

Five waves of part-time work, TypeScript first because that is where the largest set of validation repositories is. Each wave has a [plan](docs/plans/README.md) with an architect section, step-by-step developer instructions and a sub-wave delivery schedule. Durations are calendar weeks at roughly ten hours a week.

| Wave | Weeks | What lands | Exit criterion |
| --- | --- | --- | --- |
| **[0 Spike](docs/plans/pending/0000-wave-0-spike.md)** | 4 | The TypeScript extractor over `oxc` against dependency-cruiser's 546 extraction fixtures; the ECMA-335 and portable PDB reader; both conformance harnesses; the nightly validation runner; the name held on four registries | Extraction fixtures at 95%, and 99% of .NET types attributed to a source file, or the C# fallback extractor is invoked |
| **[1 TypeScript parity](docs/plans/pending/0001-wave-1-typescript-parity.md)** | 10 | The full dependency-cruiser rule language and options; both config formats; the terminal, JSON and agent reporters; `init`, `adopt`, hooks and `attest`; the npm package and a GitHub Action | Zero-diff against dependency-cruiser on its own repository, langfuse and FluidFramework at pinned commits |
| **[2 .NET, Python, element rules](docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md)** | 10 | Both remaining extractors; element, slice and diagram rules with ArchUnitNET's full vocabulary; SARIF, JUnit and graph reporters; importers from ArchUnitNET, import-linter and ESLint; test-runner adapters | Every .NET project's imported tests agree with `dotnet test`, and every Python project's contracts reproduce |
| **[3 Operations and the inner loop](docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md)** | 8 | Caching, `--affected`, `diff`; the remaining reporters; framework presets; a source mode for .NET; a Roslyn analyzer; MCP and LSP servers; the public rule library | All twenty-one output types byte-compared, and a sub-two-second check on a large solution |
| **[4 Reach](docs/plans/pending/0004-wave-4-reach.md)** | 8 | A WebAssembly playground and docs site; a pull-request app; `fix --plan`; fleet-wide rules across repositories | Funded only if the adoption measurements move; see below |

**Adoption order.** My own repositories first, then the TypeScript projects that already use dependency-cruiser, offered as a drop-in that reads their existing config. Then the .NET and Python projects, where migration is a command rather than a rewrite. Every offer goes to an issue first, with the zero-difference result attached, and is withdrawn without argument if declined.

**Wave 4 is conditional, deliberately.** Six adoption signals are measured from wave 1, including the share of agent-authored pull requests that pass the boundary check on the first run and the median number of turns from a violation to green. If the first two do not move, the agent-facing surface is cut back to the reporter and the hook, and the tool remains a better dependency-cruiser that also covers .NET and Python. That is still worth having, and saying so in advance is cheaper than discovering it later.

## Built for agents as well as people

A rule file is already the best interface a coding agent has to an architecture: it is text, it is in the repository, it names the fence and the reason, and CI runs it. What daily use in an agent-developed monorepo shows is where that interface leaks, and each leak has an answer here. Findings carry a line, a column and the member reference that formed the edge. Every rule can carry `fix` text and executable examples. `can-import`, `place` and `impact` answer questions before the code is written, in milliseconds, from a cached graph. A vacuous rule fails, so an agent cannot write a fence that matches nothing and call the job done. Ratchets only fall, so clearing a check by raising a budget fails instead of passing.

## Quality gates

Every gate below is required and green today, on a repository with no feature code yet. That ordering is intentional: the first feature pull request faces the finished harness.

| Gate | What it enforces |
| --- | --- |
| `cargo lint` | One entry point for four languages: rustfmt and clippy, eslint and prettier, ruff and mypy, dotnet format |
| Documentation links | Every relative link and anchor resolves, checked inside every compile, so a broken reference fails `cargo build` |
| Coverage | 70% of lines per crate, not just per workspace |
| Mutation testing | `cargo mutants` over the contract crates; a surviving mutant is a missing assertion and fails the build |
| Conformance | dependency-cruiser's and ArchUnitNET's own test suites, as ratchets that may only tighten |
| Supply chain | Licences, advisories, banned crates and allowed registries; actions pinned by commit SHA |
| Reproducibility | Minimum Rust version, every feature combination, determinism asserted byte for byte |

See [CLAUDE.md](CLAUDE.md) for the working agreement, and [ADR-0023](docs/adr/0023-documentation-link-and-lint-gates.md) through [ADR-0025](docs/adr/0025-ci-and-supply-chain-hardening.md) for why each gate exists.

## Repository map

| Path | What is there |
| --- | --- |
| [docs/artifacts/](docs/artifacts/README.md) | The source design and the two coverage tables. Read-only, exported verbatim |
| [docs/prd.md](docs/prd.md) | Requirements with fixed identifiers that the plans and the code cite |
| [docs/architecture.md](docs/architecture.md) | The target architecture: stages, crates, graph document, extractors, security, performance |
| [docs/adr/](docs/adr/README.md) | Every decision, numbered. A rule cites one as `adr:NNNN` |
| [docs/plans/](docs/plans/README.md) | One plan per wave, pending until its exit criteria are met |
| `crates/` | The Rust workspace: model, config, rules, three extractors, ingest, reporters, CLI, Node binding |
| `conformance/`, `testbeds/` | The upstream suites and the pinned open-source repositories validated nightly |
| `wrappers/`, `adapters/`, `frontends/` | npm, NuGet and pip wrappers; test-runner adapters; the ESLint plugin and Roslyn analyzer |

## Building

```sh
cargo build --release          # target/release/rulebearing
cargo test --workspace --all-features
cargo lint                     # all four languages, plus documentation links
cargo check-links              # documentation links and anchors only
cargo llvm-cov --workspace --fail-under-lines 70 && scripts/coverage-per-crate.sh 70
cargo mutants --package rb-model --package rb-rules --package xtask
```

Install the git hooks once, and formatting, links and tests run before each commit and push:

```sh
git config core.hooksPath .githooks
```

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) first; it is short and points at the four documents that govern everything else. The bar is unusual in one respect: a change is proven by the upstream specification rather than by argument, and no gate may be lowered to get a build green.

This is a personal project built in evenings by one person. The bus factor is one, and the mitigation is that the reviewer of record is a pair of upstream test suites that anyone can run. A second maintainer is the goal by the end of wave 2.

## Licence

[MIT](LICENSE), matching dependency-cruiser and NetArchTest, so the conformance harness can vendor their fixtures. ArchUnitNET's Apache-2.0 fixtures carry their notice. Security reports go through the [security policy](SECURITY.md).
