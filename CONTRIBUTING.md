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

## Reporting

Bugs and questions belong in issues. Security reports go through the [security policy](SECURITY.md), not an issue.
