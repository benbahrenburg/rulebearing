# Contributing

Thank you for looking. This is a personal project built in evenings, under the MIT licence, and it is deliberately governed by documents rather than by memory.

## Read these first

| Document | What it tells you |
| --- | --- |
| [CLAUDE.md](CLAUDE.md) | The working agreement: where things live, the conventions per language, the definition of done |
| [docs/plans/README.md](docs/plans/README.md) | What is being built, in which wave, and what "done" means for it |
| [docs/adr/README.md](docs/adr/README.md) | Every decision, numbered. A rule cites one as `adr:NNNN` |
| [docs/artifacts/design.md](docs/artifacts/design.md) | Why the project exists and what it promises |

## Before you open a pull request

```sh
cargo lint          # doc links, rustfmt, clippy, eslint, prettier, ruff, mypy, dotnet format
cargo test --workspace --all-features
cargo llvm-cov --workspace --fail-under-lines 70 && scripts/coverage-per-crate.sh 70
```

Install the hooks once, and the first two run for you before each push:

```sh
git config core.hooksPath .githooks
```

The pull request template is the definition of done as a checklist. Name the plan and sub-wave your change belongs to, the requirement IDs it satisfies and the ADRs it applies.

## What makes a change easy to accept

- **It belongs to a plan.** If the design calls for something and no plan names it, add it to the plan in a separate change first.
- **It is proven by the specification, not by argument.** An extractor change runs dependency-cruiser's own fixtures; an element predicate runs its ported ArchUnitNET case. The conformance ratchets may only shrink.
- **It carries links.** Every source file, plan and ADR links the architecture section, the ADRs, the plan and the requirement it serves. A broken link fails the build.
- **It does not lower a gate.** Widening a pattern, raising a ratchet, adding an `allowEmpty` or disabling a lint to get green is the one thing that will be sent back. Record a genuine exception with an owner and an expiry instead.

## Decisions

An accepted ADR is never edited except to change its status; a reversal is a new ADR that supersedes it. If your change makes a decision, write the ADR in the same pull request.

## Adding a ported gate 2 case

Gate 2's cases are the upstream tests, ported as data ([ADR-0009](docs/adr/0009-conformance-suites-as-specification.md), [conformance/README.md](conformance/README.md)). A snapshot test in ArchUnitNET is ported by the tool: `python3 conformance/archunitnet/tools/port.py` rewrites `conformance/archunitnet/ported/` and the counts in `ported.json` and `unported.json`. A test whose assertions are C# is ported by hand:

1. Add a file `conformance/archunitnet/ported/<TestClass>.yaml` whose first line starts `# Ported from ArchUnitNET 0.13.4 <path> by hand:` and says why. Give each case an `id` (`<Test>#<n>`), a `query` in words, the upstream `csharp` assertion, the `rule`, and the `expect`: `pass` and `fail` sets of full names, `passes` for `HasNoViolations`, `vacuous`, or an `error`. A `family` other than `element` (`slice`, `diagram`, `plantuml`, `association`, `baseline`) is read as `crates/rb-rules/tests/gate2.rs` describes.
2. Rerun `port.py`: a hand-ported test replaces its one unported entry with its cases, and the tool must reproduce the counts you expect.
3. Run `cargo test -p rb-rules --test gate2 -- --nocapture`, `scripts/gate2-check.sh` and `scripts/gate2-ratchet.sh`. A case that fails is a bug in the engine or the extractor, never in the expectation: fix the code, not the case.

NetArchTest cases are written by `conformance/netarchtest/tools/Port`, which runs NetArchTest itself over the committed fixtures ([conformance/netarchtest/README.md](conformance/netarchtest/README.md)).

## A second maintainer

The project has one maintainer, and a second is the goal by the end of wave 2 ([design § Open questions](docs/artifacts/design.md#open-questions)). The criterion is fixed in advance ([plan 0002 § 1.7](docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#17-decisions-this-wave-must-make), "Second maintainer"):

| Criterion | What counts |
| --- | --- |
| An external contributor | Someone other than the current maintainer |
| With merge rights | Granted on this repository once the pull request below has merged |
| Who has landed a pull request that passed both gates | Merged, with conformance gate 1 (dependency-cruiser's suite) and conformance gate 2 (ArchUnitNET's and NetArchTest's suites) green on it ([ADR-0009](docs/adr/0009-conformance-suites-as-specification.md)), alongside the rest of the definition of done |

Two parts of the tree are the safest first pull request, because a committed specification tells you when you are right and a reviewer does not have to:

| If you are | Start in | Why it is safe |
| --- | --- | --- |
| A Python engineer | [`crates/rb-extract-python`](crates/rb-extract-python/src/lib.rs) | It depends on `rb-model` only ([ADR-0010](docs/adr/0010-crate-layout-and-extractor-boundary.md)), so a change cannot reach the engine; its fixture package is compared byte for byte with a committed expectation, and the import-linter oracles in [the test beds](testbeds/README.md) say whether a graph agrees with the tool Python teams already run |
| A C# engineer | the adapters under [`adapters/dotnet`](adapters/dotnet/README.md) | They never evaluate a rule, only report what the binary found, and each of the seven packages has its own test project at the 70% line floor; gate 2's ported cases fix what a finding looks like |

The invitation is made in the upstream-offer issues of [plan 0002, Step 15](docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#215-step-15-greenfield-init-proof-the-nightly-tables-upstream-offers-second-maintainer-2i): the ones that offer evolutionary-architecture-by-example, RiverBooks, kedro and sqlfluff a pull request adding Rulebearing beside their incumbent tool. Whether the criterion is met by the end of wave 2 is recorded in that plan's status table; not meeting it does not block the plan, and it is carried to wave 3.

## Reporting

Bugs and questions belong in issues. Security reports go through the [security policy](SECURITY.md), not an issue.
