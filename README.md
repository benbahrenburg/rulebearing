# Rulebearing

**Deterministic guardrails for agentic engineering.** Architecture rules your coding agent cannot talk its way past, with the fix attached to every finding, for TypeScript, .NET and Python in one rule file.

> **Status: wave 0.** The design is complete, every quality gate is wired and green, and no subcommand ships yet. The first release, a drop-in for repositories already using dependency-cruiser, is [wave 1](docs/plans/pending/0001-wave-1-typescript-parity.md). If the problem below is yours, star or watch the repository; the [roadmap](#roadmap) says what lands when.

## The problem with telling an agent the rules

Most teams working with coding agents have written the same file. It is called `AGENTS.md` or `CLAUDE.md`, it started as a page, and it is now forty kilobytes of prose rules, each one recording a mistake that already cost something. "Apps only talk to each other over HTTP." "Nothing in `Domain` may reference `Infrastructure`." "Features must not import each other."

Prose rules do not hold. Not because agents ignore them, but because of how agents actually work:

- **They follow what fails fast and locally.** A rule that fails only in CI after merge documents drift; it does not prevent it. A rule the agent never sees fail is not a rule.
- **They act on the message, not the rule.** Given `from -> to` and a line number, an agent fixes the import. Given a rule name, it goes searching. Given a paragraph in a Markdown file, it does what the paragraph mostly seems to say.
- **They take the cheapest path to green.** Widen a pattern, raise a budget, add an exception. Every one of those is a legitimate edit unless something refuses it.
- **They write rules when asked, and the rules match nothing.** In the monorepo this project comes from, four architecture rules sat for months matching zero files, reading as standing fences and guarding nothing.

The tools that could enforce these rules already exist, one per language, and none of them was built with an agent as the reader of its findings. A finding names two files and no line. The fix-it advice lives in a comment only one output mode prints. Asking "may this file import that one" costs a thirteen-second full run. And a rule that matches nothing passes.

## What a deterministic guardrail looks like

Rulebearing turns the paragraph into a fence the agent runs into before it finishes its turn, with the way out attached.

```yaml
# rulebearing.yaml
rules:
  dependencies:
    forbidden:
      - name: no-cross-app-imports
        comment: "Apps share only packages/* and HTTP. adr:0003"
        fix: "Call the other app over its API, or move the shared code into packages/*."
        severity: error
        from: { path: "^apps/([^/]+)/" }
        to:   { path: "^apps/([^/]+)/", pathNot: "^apps/$1/" }
        examples:
          forbidden: ["apps/web/src/x.ts -> apps/worker/src/y.ts"]
          allowed:   ["apps/web/src/x.ts -> packages/format/src/index.ts"]
```

Every part of that rule is load-bearing for an agent:

| Field | What it does for the agent |
| --- | --- |
| `name` | A stable id a decision record can cite and a review comment can reference |
| `comment` with `adr:0003` | The *why*, linked to the decision. A rule without a decision token is rejected |
| `fix` | The imperative the agent follows when the rule fires. Printed in every output an agent reads |
| `examples` | Executable. `rulebearing test` proves the rule fires before CI has to |
| `from` and `to` | Match the **resolved** target, so switching import style cannot dodge the fence |

And when it fires, the agent gets something it can act on rather than a paragraph to interpret:

```jsonc
// rulebearing cruise --affected HEAD --output-type agent   (shape, illustrative)
{
  "violations": [{
    "id": "RB-4f2a9c1e",
    "rule": "no-cross-app-imports",
    "from": "apps/web/src/checkout/total.ts", "line": 3, "column": 1,
    "to": "apps/worker/src/pricing/index.ts",
    "fix": "Call the other app over its API, or move the shared code into packages/*.",
    "decision": "adr:0003"
  }],
  "inspected": { "files": 212, "modules": 212 }
}
```

A line and a column. A stable id, so "fix RB-4f2a9c1e" means the same thing on every run. The fix text. The decision it serves. A receipt of what was looked at, so "the check passed" is distinguishable from "the check looked at nothing". Token-budgeted with `--max-findings`, grouped by rule, no prose to parse.

## Inside the loop, not beside it

A check the agent has to remember to run is a check that gets skipped. Rulebearing installs itself into the loops agents already run.

```sh
rulebearing hooks install --claude-code
```

That writes three hooks for Claude Code:

| Hook | What happens | Why it matters |
| --- | --- | --- |
| **SessionStart** | Injects a token-budgeted architecture brief: tiers, hot boundaries, open violations, ratchet headroom | The agent starts every session knowing the architecture, not just the task |
| **PreToolUse** on Edit and Write | Runs `impact` on the file about to change | The agent is told *before* editing that the file sits on a boundary, not after |
| **Stop** | Runs the affected check and feeds violations back before the turn ends | Sub-two-second budget, so it stays on. A thirteen-second check gets disabled |

For pipelines and editors, the same rules and the same graph feed a pre-commit hook, one test case per rule in xUnit, NUnit, pytest or vitest, an ESLint rule that flags a boundary violation inline as the agent types, a Roslyn analyzer that fails `dotnet build` with the line, and an MCP server so the architecture is available as tools rather than as a document the agent may not have read.

Before it writes the import, the agent can ask:

```sh
rulebearing can-import apps/web/src/x.ts apps/worker/src/y.ts   # yes or no, and which rule decides, in milliseconds
rulebearing place --imports a,b --imported-by c --language ts    # where would a new module with these edges be legal
rulebearing impact src/Domain/Entities/Order.cs                  # what depends on this, what rules mention it, is it on a cycle
rulebearing explain no-cross-app-imports                         # the rule, its fix, what it matches today, the first ten edges
```

## Guardrails on the guardrails

An agent asked to "stop Domain from reaching Web" will write the regex. The tool makes that safe rather than trusting it.

- **A rule that matches nothing fails.** Every rule of every kind, by default. The four dead rules mentioned above would have failed the pull request that orphaned them. This check is what the tool is named for: a rule that bears nothing is not a rule.
- **A rule needs a decision.** `--require-comment-token` refuses a fence that does not name the decision record it serves.
- **A rule ships with proof.** `examples` are required for a new rule and `rulebearing test` runs them against a synthetic graph, so a rule cannot merge without evidence that it fires.
- **`propose` drafts the rule from evidence.** Give it globs, a selector or one forbidden edge, and it returns the narrowest rule that covers it with the current match counts on both sides. The agent starts from that, not from a blank regex.
- **`config lint` catches the mistakes agents make by hand.** A pattern that matches nothing, a rule shadowed by an earlier one, an allow-list that admits everything, a predicate the language cannot answer.
- **Ratchets only fall.** A budget file records a ceiling that `count --write` may lower and refuses to raise. An agent clearing a check by editing the budget gets a failure, not a green build.
- **Runs are hermetic.** No network, no code execution outside a sandboxed config evaluator, deterministic output. The agent's local run and CI agree byte for byte, and `attest` writes a receipt CI verifies, which answers the recurring review question on agent-authored pull requests: did it actually run the check it says it ran.

## One rule file for the whole repository

The rules above work the same way whether the edge is a TypeScript import, a .NET type reference read from the compiled assembly, or a Python import. Rulebearing is a strict superset of [dependency-cruiser](https://github.com/sverweij/dependency-cruiser) for TypeScript and JavaScript and of [ArchUnitNET](https://github.com/TNG/ArchUnitNET) for .NET, with [import-linter](https://github.com/seddonym/import-linter)'s contract kinds mapped one to one for Python. Every rule attribute, option, flag and reporter of the incumbents has a row in the [coverage tables](docs/artifacts/README.md) with its status; nothing is dropped, and the claim is proven by running their own test suites against this tool as required checks.

So a repository with a TypeScript front end, a .NET service and a Python pipeline keeps one rule file, one gate, one graph other scripts can read, and one answer to "may this file import that one". If you already have a `.dependency-cruiser.js`, it runs unchanged on day one.

## Try it

**Today, in wave 0.** Nothing runs yet, and this README will not pretend otherwise. What you can do:

1. Read the [rule language](docs/architecture.md#configuration-and-the-rule-language) and the [agent surface](docs/architecture.md#agent-surface), and open an issue if a rule you need has no way to be written.
2. If you run dependency-cruiser, ArchUnitNET, NetArchTest or import-linter today, your repository may be a good [validation target](testbeds/manifest.yaml). Rulebearing must reproduce your tool's findings with zero difference; a repository that breaks that is the most useful kind.
3. Star or watch. The first release is a drop-in, and this is where it will be announced.

**Wave 1, the first release.** In a TypeScript repository:

```sh
npm install --save-dev rulebearing
npx rulebearing init                      # reads the repo and proposes rules that already pass
npx rulebearing hooks install --claude-code
npx rulebearing cruise --output-type agent
```

Or, if you already have a dependency-cruiser config, replace `depcruise` with `rulebearing` in your pipeline and change nothing else.

**Wave 2.** `dotnet tool install Rulebearing` and `pip install rulebearing`, with `rulebearing import archunit` and `rulebearing import import-linter` to bring existing rules across as a command rather than a rewrite.

## Roadmap

Five waves of part-time work, TypeScript first because that is where the largest set of validation repositories is. Each wave has a [plan](docs/plans/README.md) with an architect section, step-by-step developer instructions and a sub-wave schedule.

| Wave | Weeks | What lands for you | Exit criterion |
| --- | --- | --- | --- |
| **[0 Spike](docs/plans/pending/0000-wave-0-spike.md)** | 4 | The TypeScript extractor proven against dependency-cruiser's 546 fixtures; the .NET metadata reader; both conformance harnesses; the name held on four registries | Fixtures at 95%; 99% of .NET types attributed to a source file, or the fallback extractor is invoked |
| **[1 TypeScript](docs/plans/pending/0001-wave-1-typescript-parity.md)** | 10 | The drop-in: full dependency-cruiser parity, the `agent` reporter, `fix` and `examples`, line-precise findings, liveness by default, `init`, `adopt`, `hooks install`, `attest`, `can-import`, `explain`, `test`; npm package and GitHub Action | Zero difference against dependency-cruiser on its own repository, langfuse and FluidFramework |
| **[2 .NET and Python](docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md)** | 10 | Both extractors; ArchUnitNET's full vocabulary as declarative element rules; `propose`, `impact`, `place`, `docs`; importers; test-runner adapters; the ESLint rule; SARIF and JUnit | Every imported .NET test agrees with `dotnet test`; every Python contract reproduces |
| **[3 Inner loop](docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md)** | 8 | Caching and `--affected`; a source mode for .NET that answers without a build; `guard --watch`; the Roslyn analyzer; MCP and LSP servers; framework presets; the public rule library | Stop hook under two seconds on a large .NET solution; all reporters byte-compared |
| **[4 Reach](docs/plans/pending/0004-wave-4-reach.md)** | 8 | Browser playground; pull-request app; `fix --plan`; rules across a fleet of repositories | Funded only if the numbers below move |

**This is measured, not believed.** From wave 1, six signals are tracked on the repositories where agent-authored pull requests can be seen: the share that pass the boundary check on their first CI run (target above 90%), the median turns from a violation to green (target one), rules caught by the authoring guardrails, Stop-hook latency, the share of rules carrying `fix` text, and budget raises merged (target zero). If the first two do not move, the agent surface is cut back to the reporter and the hook, and what remains is a faster dependency-cruiser that also covers .NET and Python. Saying that in advance is cheaper than discovering it later.

## Validated nightly against real repositories

Every night the repositories in [testbeds/manifest.yaml](testbeds/manifest.yaml) are cloned at a pinned commit and their incumbent tool runs with its own configuration: dependency-cruiser, NetArchTest, ArchUnitNET or import-linter. From wave 1 Rulebearing runs beside it, and the table shows whether the two agree to the finding and how long each took ([testbeds/README.md](testbeds/README.md)). The latest full run is on the [`testbeds-results`](https://github.com/benbahrenburg/rulebearing/tree/testbeds-results) branch; the table below is refreshed from it by pull request.

<!-- testbeds:start -->
<!-- testbeds:end -->

## Built the way it asks you to build

This repository holds itself to the bar it proposes for yours. Every gate below is required and green today, before any feature code exists, so the first feature pull request faces the finished harness.

| Gate | What it enforces |
| --- | --- |
| `cargo lint` | Four languages behind one command: rustfmt and clippy, eslint and prettier, ruff and mypy, dotnet format |
| Documentation links | Every relative link and anchor resolves, checked inside every compile. A broken reference fails `cargo build` |
| Coverage | 70% of lines per crate, not per workspace |
| Mutation testing | A surviving mutant is a missing assertion and fails the build. The gate starts at zero survivors |
| Conformance | The incumbents' own test suites, as ratchets that may only tighten |
| Supply chain | Licences, advisories, allowed registries; every action pinned by commit SHA |
| Reproducibility | Minimum Rust version, every feature combination, determinism asserted byte for byte |

The reasoning behind each is a numbered decision record in [docs/adr/](docs/adr/README.md), and the repository's own `rulebearing.yaml` cites them the way it asks yours to.

## Repository map

| Path | What is there |
| --- | --- |
| [docs/artifacts/](docs/artifacts/README.md) | The source design and the two coverage tables, exported verbatim |
| [docs/prd.md](docs/prd.md) | Requirements with fixed identifiers the plans and code cite |
| [docs/architecture.md](docs/architecture.md) | Stages, crates, the graph document, extractors, security, performance |
| [docs/adr/](docs/adr/README.md) | Every decision, numbered |
| [docs/plans/](docs/plans/README.md) | One plan per wave |
| `crates/` | The Rust workspace: model, config, rules, three extractors, ingest, reporters, CLI, Node binding |
| `conformance/`, `testbeds/` | The upstream suites and the pinned repositories validated nightly |
| `wrappers/`, `adapters/`, `frontends/` | npm, NuGet and pip wrappers; test-runner adapters; the ESLint plugin and Roslyn analyzer |

## Building

```sh
cargo build --release          # target/release/rulebearing
cargo test --workspace --all-features
cargo lint                     # all four languages, plus documentation links
cargo mutants --package rb-model --package rb-rules --package xtask
git config core.hooksPath .githooks   # formatting, links and tests before each commit and push
```

## Contributing and licence

[CONTRIBUTING.md](CONTRIBUTING.md) is short and points at the four documents that govern everything else. One person builds this in evenings; the reviewer of record is a pair of upstream test suites anyone can run, and a second maintainer is the goal by the end of wave 2.

[MIT](LICENSE). Security reports go through the [security policy](SECURITY.md).
