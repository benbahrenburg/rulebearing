# Rulebearing

**Deterministic guardrails for agentic engineering.** Architecture rules your coding agent cannot talk its way past, with the fix attached to every finding, for TypeScript (TypeScript 7 included), JavaScript, .NET and Python in one rule file.

> **Status: released.** Waves 1 to 3 are done. `v0.1.3` is on npm, PyPI and NuGet, and the [docs site](https://benbahrenburg.github.io/rulebearing/) has a playground that runs in the browser. Most of wave 4 is on `main` and arrives with the next release; [What is new on `main`](#what-is-new-on-main) lists it. The move to dependency-cruiser 18.7.0 is in progress: its summary fields, options and exit codes are on `main`, and the rest of it is not. If the problem below is yours, star or watch the repository; the [roadmap](#roadmap) says what lands when.

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

For pipelines and editors, the same rules and the same graph feed a pre-commit hook, one test case per rule in xUnit, NUnit, pytest or vitest, an ESLint rule that flags a boundary violation inline as the agent types, a Roslyn analyzer that fails `dotnet build` with the line, an MCP server so the architecture is available as tools rather than as a document the agent may not have read (`rulebearing hooks install --claude-code --mcp`), and a language server that shows each finding in the editor with its `fix` as the quick-fix title ([serve.md](docs/serve.md)).

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

The rules above work the same way whether the edge is a TypeScript import, a .NET type reference read from the compiled assembly, or a Python import. Rulebearing is a strict superset of [dependency-cruiser](https://github.com/sverweij/dependency-cruiser) for TypeScript and JavaScript and of [ArchUnitNET](https://github.com/TNG/ArchUnitNET) for .NET, with [import-linter](https://github.com/seddonym/import-linter)'s contract kinds mapped one to one for Python. Every rule attribute, option, flag and reporter of the incumbents is accounted for; nothing is dropped, and the claim is proven by running their own test suites against this tool as required checks ([conformance](conformance/README.md)).

So a repository with a TypeScript front end, a .NET service and a Python pipeline keeps one rule file, one gate, one graph other scripts can read, and one answer to "may this file import that one". If you already have a `.dependency-cruiser.js`, it runs unchanged on day one.

## Try it

**In a TypeScript repository:**

```sh
npm install --save-dev rulebearing
npx rulebearing init                                # reads the repo and proposes rules that already pass
npx rulebearing cruise --output-type agent
npx rulebearing hooks install --claude-code
```

or in GitHub Actions, `uses: benbahrenburg/rulebearing@v0.1.3` with `args: --config rulebearing.yaml src`. A repository already on dependency-cruiser keeps its configuration: replace `depcruise` with `rulebearing cruise` and change nothing else, or run `rulebearing adopt` for a baseline, a CI step and one green pull request. It matches dependency-cruiser 18.2.0 with zero differences on dependency-cruiser's own repository, langfuse and FluidFramework ([conformance](conformance/README.md)).

**From source,** for what is on `main` and not yet released: `cargo build --release` writes `target/release/rulebearing`.

**Without installing anything,** try a rule in the [playground](https://benbahrenburg.github.io/rulebearing/).

The guides: [configuration](docs/config.md), [rules](docs/rules.md), [reporters](docs/reporters.md), [the command line](docs/cli.md) and [Rulebearing for coding agents](docs/agents.md).

**.NET and Python.** The .NET extractor reads the compiled assemblies and their portable PDBs, so build first; the Python extractor reads the source, without running an interpreter. Install with `dotnet tool install -g Rulebearing` or `pip install rulebearing`.

```sh
dotnet build -p:DebugType=portable                  # .NET: the assemblies and PDBs the extractor reads
rulebearing init                                    # finds the solution or pyproject.toml and proposes rules that pass
rulebearing cruise --output-type agent
```

Existing rules come across as a command, not a rewrite, with the original kept as a comment beside each rule:

```sh
rulebearing import archunit tests/Architecture.Tests --out rulebearing.yaml      # ArchUnitNET or NetArchTest chains
rulebearing import import-linter --from pyproject.toml --out rulebearing.yaml     # or .importlinter, setup.cfg
rulebearing import eslint --from eslint.config.js --out rulebearing.yaml          # no-restricted-paths, eslint-plugin-boundaries
```

The same rules run as tests in the suite you already have: `Rulebearing.TestAdapter` for xUnit, NUnit, MSTest and TUnit, `pytest-rulebearing`, and `rulebearing/vitest`, one test per rule with the fix in the failure message; `eslint-plugin-rulebearing` flags a boundary in the editor. ArchUnitNET's predicates are keys of [element rules](docs/rules.md), with each key's meaning in every language in the [generated reference](docs/reference/element-rules.md).

## What is new on `main`

These landed after `v0.1.3` and arrive with the next release. The [command line guide](docs/cli.md) has the details.

| Feature | What it does | Guide |
| --- | --- | --- |
| `fix --plan <rule>` | For each violation, prints the cheapest refactor of each kind (`move`, `port`, `invert`) as JSON, each one checked with `can-import` first. `cruise --output-type agent --with-plans` adds the plans to the agent report | [cli](docs/cli.md) |
| `fleet` | Cruises sibling repositories, each with its own configuration and cache, and evaluates fleet-wide rules over the union in one report | [config](docs/config.md#fleets-and-declared-edges) |
| Declared edges | `edges.yaml` and OpenAPI documents add cross-language edges you state yourself. Every reporter marks them `declared: true`, and `--strict-schema` strips them | [config](docs/config.md#fleets-and-declared-edges) |
| Usage counts | Counts commands and rules on your machine and prints them. Off by default, and this release sends nothing | [usage counts](docs/usage-counts.md) |
| A faster Stop hook | A run takes one `git status`, and a Stop hook with nothing to block on renders no report. p95 on dotnet/aspnetcore in source mode was 1.95 s in the first run on the slower CI runner class, against 2.02 to 2.36 s on the three nights that failed there. That leaves little headroom under the 2 s budget | [perf](docs/perf.md) |
| Question pages | The docs site's cookbook: eight architecture questions, each a small repository the page's playground loads | [cookbook](cookbook/README.md) |
| dependency-cruiser 18.7.0 summary fields and exit codes | The summary carries `advisedExitCode`, `baselineSize` and `baselineStale`, and the options `ignoreKnown` and `baseline.staleEntriesSeverity` take effect. `cruise`, `fmt --exit-code` and the gating reporters exit with `advisedExitCode` plus expired entries and ratchets. With `staleEntriesSeverity: error`, a run with only stale entries fails, and `err`, `err-long`, `err-html` and `markdown` say why | [config](docs/config.md) |
| Safe YAML | YAML is parsed by `serde-saphyr`, written in safe Rust, in place of the unmaintained `serde_yaml` | [ADR-0079](docs/adr/0079-yaml-is-parsed-with-serde-saphyr.md) |

## TypeScript 7

TypeScript 7 is supported. Rulebearing parses with oxc and resolves with `oxc_resolver`; it never loads the `typescript` package, so the compiler version a repository installs does not matter to it. What TypeScript 7 changes for an extractor is the tsconfig, and that is what is tested:

| What | How it is proven |
| --- | --- |
| `paths` without `baseUrl`, in the tsconfig itself or inherited through `extends` from another folder, and `${configDir}` | fixture [`ts7-config`](crates/rb-extract-ts/tests/options/ts7-config) |
| `moduleResolution: bundler` and `nodenext`, `.js` specifiers for `.ts` files, `#` subpath imports | the same fixture |
| The tsconfig's `module` and `target` deciding whether an import reaches the rules as `import` or `require` when `tsPreCompilationDeps` is off, as dependency-cruiser's `transpileModule` step decides it | fixture [`ts-config-module`](crates/rb-extract-ts/tests/options/ts-config-module), every expectation taken from dependency-cruiser 18.2.0 with TypeScript 6.0.3 |
| A real repository on TypeScript 7 | gate 1 layer 5 on langfuse, whose `web` package is on `typescript ^7.0.2` with a `baseUrl`-free tsconfig: 3,832 modules and 369 violations, zero differences from dependency-cruiser |

dependency-cruiser cannot load TypeScript 7 itself (its manifest accepts `typescript >=2.0.0 <7.0.0`), so a repository that keeps it beside Rulebearing installs TypeScript 6 for it, as langfuse does with `@typescript/typescript6`. Two resolutions follow TypeScript rather than dependency-cruiser, whose `tsconfig-paths` 4.2.0 departs from the compiler once `baseUrl` is gone: inherited `paths` resolve (dependency-cruiser leaves them unresolved), and a bare `src/x` with no `baseUrl` stays unresolved (dependency-cruiser resolves it against the tsconfig's folder). `module: amd`, `umd`, `system` and `none`, which TypeScript 7 removes, stop a run with exit 2 unless `tsPreCompilationDeps` is `true`. The reasoning is [ADR-0040](docs/adr/0040-typescript-tsconfig-semantics-where-tsconfig-paths-departs.md).

## Roadmap

Five waves of part-time work, TypeScript first because that is where the largest set of validation repositories is.

| Wave | Weeks | What lands for you | Exit criterion | Status |
| --- | --- | --- | --- | --- |
| **0 Spike** | 4 | The TypeScript extractor, matching dependency-cruiser on 292 of the 296 cases its own extraction suite records (0.9865); the .NET metadata reader, attributing 99.29% of the .NET oracles' types to a source file, so the C# fallback is not needed ([ADR-0022](docs/adr/0022-dotnet-reader-in-rust-confirmed.md)); both conformance harnesses; the nightly test beds; the name held on four registries | Fixtures at 95%; 99% of .NET types attributed to a source file, or the fallback extractor is invoked | Done |
| **1 TypeScript** | 10 | The drop-in: full dependency-cruiser parity, TypeScript 7 included; the `agent` reporter, `fix` and `examples`, line-precise findings, liveness by default, `init`, `adopt`, `hooks install`, `attest`, `can-import`, `explain`, `test`; npm package and GitHub Action | Zero difference against dependency-cruiser on its own repository, langfuse and FluidFramework | Done; released |
| **2 .NET and Python** | 10 | Both extractors; ArchUnitNET's full vocabulary as declarative element rules; `propose`, `impact`, `place`, `docs`; importers; test-runner adapters; the ESLint rule; SARIF and JUnit | Every imported .NET test agrees with `dotnet test`; every Python contract reproduces | Done; released on NuGet and PyPI |
| **3 Inner loop** | 8 | Caching and `--affected`; a source mode for .NET that answers without a build; `guard --watch`; the Roslyn analyzer; MCP and LSP servers; framework presets; the public rule library | Stop hook under two seconds on a large .NET solution; all reporters byte-compared | Done; released |
| **4 Reach** | 8 | Docs site and browser playground; question pages; `fix --plan` and its fix-text evaluation; `fleet` and declared edges; usage counts; a pull-request app | The adoption review closes on the six signals below | In progress. On `main`: all of it except the pull-request app and the closing adoption review, neither started |

Beside the waves, seven tracks. Three add languages and incumbents under the same promise: each incumbent's own test suite becomes a conformance gate before its row says Parity.

| Track | Weeks | What lands for you | Exit criterion | Status |
| --- | --- | --- | --- | --- |
| **dependency-cruiser 18.7.0** | 8 | Everything from 18.3 to 18.7: `cruise --baseline` and `--baseline-mode`, stale-baseline reporting with `staleEntriesSeverity` and `summary.advisedExitCode`, imports in ambient modules, `dot-webpage`, on the CLI, the npm API, the playground and the wrappers ([ADR-0080](docs/adr/0080-gate-1-moves-to-dependency-cruiser-18-7-0.md)) | Gate 1 runs 18.7.0's suite in all five layers, with no more exclusions than at 18.2.0 | In progress. On `main`: the summary fields, the options, the exit codes and stale-baseline reporting. Still to come: `--baseline` on `cruise`, ambient modules, `dot-webpage`, the wrappers and the move of gate 1, which still runs 18.2.0 |
| **Guard catalogue** | 5 | Quality, convention and lifecycle guards (cycles, dead code, licences, layer fences, budgets that only fall, exceptions that expire) as executable recipes: one fixture per guard, and the cookbook page generated from the fixtures so it cannot drift from the engine | Every recipe a green fixture in CI, none vacuous; the page checked against the fixtures | Started |
| **Rulebearing on Rulebearing** | 5 | This repository gated by its own tool: one graph over its five languages, the gate in CI, the test runners, the editor and the agent loop, and the first month of signals | The self-check green on `main`; the test runners agree with the gate | Started |
| **Java and ArchUnit** | 9 | Java as the fourth language, read from compiled classes; ArchUnit's vocabulary and library rules (layered and onion architectures, `GeneralCodingRules`); an importer for existing ArchUnit tests; JUnit adapter and a Maven Central package | ArchUnit's own tests pass as conformance gate 3; the Java oracles agree | Not started |
| **.NET, complete** | 6 | VB.NET and F# projects read as C# ones are; NetArchTest's own definitions as additive options, so a NetArchTest suite migrates without a changed verdict | NetArchTest's ported cases at 388 with ArchUnitNET's unchanged; the three .NET oracles that compare nothing today compared | Started |
| **Try a rule before it is written** | 8 | `propose --impact`, `suggest` and a `repl` session, so an agent or a person can test a rule against the graph before committing to it | Every candidate `suggest` prints holds with zero violations and matches something, on every test bed with a graph | Started |
| **Rust** | 9 | Rust as the fifth language, with arch-lint, archaven and TangleGuard as the incumbents; importers for their configurations; a crates.io package; this repository's own rules on the Rust graph instead of a script | Their suites pass as conformance gate 4; zero difference on the Rust oracles, or each difference recorded | Not started |

**This is measured, not believed.** From wave 1, six signals are tracked on the repositories where agent-authored pull requests can be seen: the share that pass the boundary check on their first CI run (target above 90%), the median turns from a violation to green (target one), rules caught by the authoring guardrails, Stop-hook latency, the share of rules carrying `fix` text, and budget raises merged (target zero). If the first two do not move, the agent surface is cut back to the reporter and the hook, and what remains is a faster dependency-cruiser that also covers .NET and Python. Saying that in advance is cheaper than discovering it later.

## Validated nightly against real repositories

Every night the repositories in [testbeds/manifest.yaml](testbeds/manifest.yaml) are cloned at a pinned commit and their incumbent tool runs with its own configuration: dependency-cruiser, NetArchTest, ArchUnitNET or import-linter. From wave 1 Rulebearing runs beside it, and the table shows whether the two agree to the finding and how long each took ([testbeds/README.md](testbeds/README.md)). The latest full run is on the [`testbeds-results`](https://github.com/benbahrenburg/rulebearing/tree/testbeds-results) branch; the table below is refreshed from it by pull request. In it, `failed` means the incumbent itself reports failing tests or broken contracts at the pinned commit, and `error` means the repository does not build there. Rows with no incumbent test Rulebearing alone, and the "What is tested" column says how: a `greenfield` row is `ok` when the configuration `init` writes cruises back with exit 0 and matches its [committed fixture](testbeds/init/README.md), and a `scale` row is timed in the [scale table](#scale-wall-clock-and-peak-memory-on-large-repositories) below. An `idle` row runs nothing.

<!-- testbeds:start -->
Rows: 51; error 2, failed 5, idle 2, ok 42. Rulebearing runs on the dependency-cruiser rows (wave 1); its time is the median of three runs, and zero diff compares its result with the incumbent's.

| Repository | Role | What is tested | Incumbent | Status | Incumbent time | Incumbent peak memory | Rulebearing time | Zero diff |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| [abpframework/abp](https://github.com/abpframework/abp/tree/e402e7e305002ebdd3a5befef743c65bd9ac9c92) | greenfield | `init`: the rules it proposes cruise clean and match the committed fixture | none | ok |  |  | 8.14 s |  |
| [apache/superset](https://github.com/apache/superset/tree/4c229cb66a7b6401113ca81f49ee617ad8426e64) | greenfield | `init`: the rules it proposes cruise clean and match the committed fixture | none | ok |  |  | 5.22 s |  |
| [getsentry/sentry](https://github.com/getsentry/sentry/tree/813bab919d13177e71d444b187473ee3cd881ae4) | greenfield | `init`: the rules it proposes cruise clean and match the committed fixture | none | ok |  |  | 24.91 s |  |
| [jasontaylordev/CleanArchitecture](https://github.com/jasontaylordev/CleanArchitecture/tree/1d71eefc5ccf9a5e9b2db86e4cf08070148c7bb4) | greenfield | `init`: the rules it proposes cruise clean and match the committed fixture | none | ok |  |  | 1.08 s |  |
| [microsoft/autogen](https://github.com/microsoft/autogen/tree/027ecf0a379bcc1d09956d46d12d44a3ad9cee14) | greenfield | `init`: the rules it proposes cruise clean and match the committed fixture | none | ok |  |  | 3.3 s |  |
| [microsoft/semantic-kernel](https://github.com/microsoft/semantic-kernel/tree/ca40aa7226531d28a721d0ca0e451d0aaf86dafc) | greenfield | `init`: the rules it proposes cruise clean and match the committed fixture | none | ok |  |  | 11.3 s |  |
| [PostHog/posthog](https://github.com/PostHog/posthog/tree/3ad9f699e53d824e424514f521a9432b27faa347) | greenfield | `init`: the rules it proposes cruise clean and match the committed fixture | none | ok |  |  | 0.12 s |  |
| [umbraco/Umbraco-CMS](https://github.com/umbraco/Umbraco-CMS/tree/e81538b0400677a2fc319e6b1600b51645be3497) | greenfield | `init`: the rules it proposes cruise clean and match the committed fixture | none | ok |  |  | 15.74 s |  |
| [zulip/zulip](https://github.com/zulip/zulip/tree/370f7b66fca175e66f9545330f4534290cc9f9db) | greenfield | `init`: the rules it proposes cruise clean and match the committed fixture | none | ok |  |  | 2.79 s |  |
| [ag-grid/ag-grid](https://github.com/ag-grid/ag-grid/tree/1a7979c4106fba6c9149f3b8cc13e5c7f4543323) | oracle | dependency-cruiser runs on the repository's own rules, beside Rulebearing where it has a time | dependency-cruiser | ok | 0.36 s | 84 MB | 0.58 s |  |
| [ardalis/RiverBooks](https://github.com/ardalis/RiverBooks/tree/0938f187f3963e225fa25e31bc10a5213932ddf4) | oracle | archunitnet runs on the repository's own rules, beside Rulebearing where it has a time | archunitnet | error |  |  |  |  |
| [aws/aws-toolkit-vscode](https://github.com/aws/aws-toolkit-vscode/tree/36ad9035f4b921e12a9135f07a5f5134b71f704b) | oracle | dependency-cruiser runs on the repository's own rules, beside Rulebearing where it has a time | dependency-cruiser | ok | 0.33 s | 94 MB | 0.54 s |  |
| [BenMorris/NetArchTest](https://github.com/BenMorris/NetArchTest/tree/a761f63b694a6facd4be01fbc8fe2ae26579572e) | oracle | netarchtest runs on the repository's own rules, beside Rulebearing where it has a time | netarchtest | failed | 2.14 s | 125 MB |  |  |
| [bridgecrewio/checkov](https://github.com/bridgecrewio/checkov/tree/92ca0101e39e7f575c8182b5b1fd7c89b04e136b) | oracle | import-linter runs on the repository's own rules, beside Rulebearing where it has a time | import-linter | ok | 0.31 s | 47 MB |  |  |
| [DrJohnMelville/Pdf](https://github.com/DrJohnMelville/Pdf/tree/2e9eef3d4af15391809d9a79253e6fff0058d668) | oracle | netarchtest runs on the repository's own rules, beside Rulebearing where it has a time | netarchtest | ok | 6.28 s | 147 MB |  |  |
| [evolutionary-architecture/evolutionary-architecture-by-example](https://github.com/evolutionary-architecture/evolutionary-architecture-by-example/tree/441a7cd94d3acfbac7d210a95ea8e78429fc9f47) | oracle | netarchtest runs on the repository's own rules, beside Rulebearing where it has a time | netarchtest | ok | 2.56 s | 124 MB |  |  |
| [google/langextract](https://github.com/google/langextract/tree/62b933a2c757fd2bbb100498571b8d1692db4344) | oracle | import-linter runs on the repository's own rules, beside Rulebearing where it has a time | import-linter | ok | 0.12 s | 32 MB |  |  |
| [HKUDS/DeepTutor](https://github.com/HKUDS/DeepTutor/tree/11f9c15f6d2727ce07ff8d66e511b1e7ae2f8463) | oracle | import-linter runs on the repository's own rules, beside Rulebearing where it has a time | import-linter | ok | 0.32 s | 44 MB |  |  |
| [infinitered/ignite](https://github.com/infinitered/ignite/tree/e829d2f922c5568a59a77bfb6232aeb500be3f13) | oracle | dependency-cruiser runs on the repository's own rules, beside Rulebearing where it has a time | dependency-cruiser | ok | 0.36 s | 96 MB | 0.05 s |  |
| [invertase/react-native-firebase](https://github.com/invertase/react-native-firebase/tree/fa3f29d815bdb6e4ee31aacf154a91c749b7c9a1) | oracle | dependency-cruiser runs on the repository's own rules, beside Rulebearing where it has a time | dependency-cruiser | ok | 0.37 s | 90 MB |  |  |
| [karaoke-dev/karaoke](https://github.com/karaoke-dev/karaoke/tree/8012c1a854e6d68ba8bdbe1d3fd7101ff61c7e3d) | oracle | archunitnet runs on the repository's own rules, beside Rulebearing where it has a time | archunitnet | ok | 13.16 s | 613 MB |  |  |
| [kedro-org/kedro](https://github.com/kedro-org/kedro/tree/8d9abdf02b1ce93fbe9079cff6f696fb230facc4) | oracle | import-linter runs on the repository's own rules, beside Rulebearing where it has a time | import-linter | ok | 0.24 s | 34 MB |  |  |
| [langfuse/langfuse](https://github.com/langfuse/langfuse/tree/17c3e9d97ca93603fe448436083e20c44ac1822a) | oracle | dependency-cruiser runs on the repository's own rules, beside Rulebearing where it has a time | dependency-cruiser | ok | 0.43 s | 100 MB | 1.5 s | yes |
| [langgenius/dify](https://github.com/langgenius/dify/tree/3a017e8cccb316f23f601af65d2e8ce92e68f788) | oracle | import-linter runs on the repository's own rules, beside Rulebearing where it has a time | import-linter | error |  |  |  |  |
| [liveblocks/liveblocks](https://github.com/liveblocks/liveblocks/tree/1e3a117a1a2020806b90c16cd4cd5171c00c232c) | oracle | dependency-cruiser runs on the repository's own rules, beside Rulebearing where it has a time | dependency-cruiser | ok | 0.33 s | 81 MB | 0.08 s |  |
| [microsoft/FluidFramework](https://github.com/microsoft/FluidFramework/tree/235654550d165d0da3a40c1b6fb1c6ec2bea676b) | oracle | dependency-cruiser runs on the repository's own rules, beside Rulebearing where it has a time | dependency-cruiser | ok | 0.48 s | 98 MB | 0.22 s | yes |
| [microsoft/promptflow](https://github.com/microsoft/promptflow/tree/3928a727b406e66d64ff42621534bb58e0ca18ce) | oracle | import-linter runs on the repository's own rules, beside Rulebearing where it has a time | import-linter | ok | 0.2 s | 43 MB |  |  |
| [nager/Nager.Date](https://github.com/nager/Nager.Date/tree/e2734bcfad2397957a5fe18a4f22fb145e87c1e0) | oracle | archunitnet runs on the repository's own rules, beside Rulebearing where it has a time | archunitnet | ok | 2.71 s | 125 MB |  |  |
| [napari/napari](https://github.com/napari/napari/tree/5ef58e212af1af11737bc005048cfd0fb7a1aefd) | oracle | import-linter runs on the repository's own rules, beside Rulebearing where it has a time | import-linter | ok | 0.29 s | 44 MB |  |  |
| [NeVeSpl/NetArchTest.eNhancedEdition](https://github.com/NeVeSpl/NetArchTest.eNhancedEdition/tree/7f29bdf5f24368dccad27b8e3ec52a807e813504) | oracle | netarchtest runs on the repository's own rules, beside Rulebearing where it has a time | netarchtest | ok | 10.13 s | 243 MB |  |  |
| [nolar/kopf](https://github.com/nolar/kopf/tree/e63c29c374cde63b6afa7378ae4486bb92062c0f) | oracle | import-linter runs on the repository's own rules, beside Rulebearing where it has a time | import-linter | failed | 0.17 s | 30 MB |  |  |
| [onebeyond/monaco](https://github.com/onebeyond/monaco/tree/5700abab4d735e7accdcd0d1fde823845e8c99c3) | oracle | archunitnet runs on the repository's own rules, beside Rulebearing where it has a time | archunitnet | failed | 3.42 s | 161 MB |  |  |
| [online-ml/river](https://github.com/online-ml/river/tree/d7085cde5bf90898b338d1ad522c71b9b9aa26f8) | oracle | import-linter runs on the repository's own rules, beside Rulebearing where it has a time | import-linter | ok | 0.23 s | 35 MB |  |  |
| [open-metadata/OpenMetadata](https://github.com/open-metadata/OpenMetadata/tree/325c3ee7af5176f615f48b89747f027018d6d61a) | oracle | import-linter runs on the repository's own rules, beside Rulebearing where it has a time | import-linter | failed | 0.86 s | 56 MB |  |  |
| [openedx/openedx-platform](https://github.com/openedx/openedx-platform/tree/23f45c133cf8e5074a723e6f6685137c87c01780) | oracle | import-linter runs on the repository's own rules, beside Rulebearing where it has a time | import-linter | ok | 0.85 s | 78 MB |  |  |
| [phongnguyend/Practical.CleanArchitecture](https://github.com/phongnguyend/Practical.CleanArchitecture/tree/033acecd44b6981d7d418d0fdab356c053e1b1d3) | oracle | netarchtest runs on the repository's own rules, beside Rulebearing where it has a time | netarchtest | ok | 2.4 s | 124 MB |  |  |
| [remult/remult](https://github.com/remult/remult/tree/f3ff339959ac19bdedb64161be8722d9a3ef8213) | oracle | dependency-cruiser runs on the repository's own rules, beside Rulebearing where it has a time | dependency-cruiser | ok | 0.87 s | 97 MB | 0.02 s |  |
| [seddonym/import-linter](https://github.com/seddonym/import-linter/tree/31927f1457e3df673912cb5efb0afa6dbc37585f) | oracle | import-linter runs on the repository's own rules, beside Rulebearing where it has a time | import-linter | ok | 0.16 s | 32 MB |  |  |
| [sqlfluff/sqlfluff](https://github.com/sqlfluff/sqlfluff/tree/0532c99e27811edb644f0afea13ade46182b72b2) | oracle | import-linter runs on the repository's own rules, beside Rulebearing where it has a time | import-linter | ok | 0.19 s | 41 MB |  |  |
| [sverweij/dependency-cruiser](https://github.com/sverweij/dependency-cruiser/tree/7c22858071cc4491321c735ff11894d90648ee6e) | oracle | dependency-cruiser runs on the repository's own rules, beside Rulebearing where it has a time | dependency-cruiser | ok | 1.2 s | 165 MB | 0.05 s | yes |
| [TNG/ArchUnitNET](https://github.com/TNG/ArchUnitNET/tree/28b62ec0d98f9babb4ccdb6d8a1c3673a0497bcb) | oracle | archunitnet runs on the repository's own rules, beside Rulebearing where it has a time | archunitnet | failed | 21.63 s | 2062 MB |  |  |
| [tsparticles/tsparticles](https://github.com/tsparticles/tsparticles/tree/d92ba4b96524f57921cdfbd3d4a60d31c897a149) | oracle | dependency-cruiser runs on the repository's own rules, beside Rulebearing where it has a time | dependency-cruiser | ok | 0.33 s | 94 MB | 0.03 s |  |
| [wemake-services/wemake-python-styleguide](https://github.com/wemake-services/wemake-python-styleguide/tree/b4d0171879edc9a6a71a1506a2ee5554f634e0ea) | oracle | import-linter runs on the repository's own rules, beside Rulebearing where it has a time | import-linter | ok | 0.14 s | 34 MB |  |  |
| [benbahrenburg/ai-sdk-otel-logger](https://github.com/benbahrenburg/ai-sdk-otel-logger/tree/b3072ce0cb125ccd3b184c05cda6bbf9de1cdca6) | own | Nothing yet: a maintainer's repository, listed for the adoption signals | none | idle |  |  |  |  |
| [benbahrenburg/rulebearing](https://github.com/benbahrenburg/rulebearing/tree/06768cbcbf80b2efc25585a21cea56b2abcc6fbb) | own | Nothing yet: a maintainer's repository, listed for the adoption signals | none | idle |  |  |  |  |
| [dotnet/aspnetcore](https://github.com/dotnet/aspnetcore/tree/b4be275a3b4fd83c304e7377403561a73eb4249e) | scale | Wall-clock and peak memory of a cruise with the rules `init` proposes | none | ok |  |  | 30.96 s |  |
| [elastic/kibana](https://github.com/elastic/kibana/tree/b502e688526bca862855361abf0081d84e65c547) | scale | Wall-clock and peak memory of a cruise with the rules `init` proposes | none | ok |  |  | 9.85 s |  |
| [grafana/grafana](https://github.com/grafana/grafana/tree/423f85091b0a63a21fb571122a933133da1652cd) | scale | Wall-clock and peak memory of a cruise with the rules `init` proposes | none | ok |  |  | 0.79 s |  |
| [home-assistant/core](https://github.com/home-assistant/core/tree/f50d777440e878554ef6872062341d24c6da0289) | scale | Wall-clock and peak memory of a cruise with the rules `init` proposes | none | ok |  |  | 10.65 s |  |
| [jellyfin/jellyfin](https://github.com/jellyfin/jellyfin/tree/fd75964da853765d63101b4889f23a4161e2758b) | scale | Wall-clock and peak memory of a cruise with the rules `init` proposes | none | ok |  |  | 4.76 s |  |
| [n8n-io/n8n](https://github.com/n8n-io/n8n/tree/f89687fd8751970cdf8b9cb76761a1258728ca62) | scale | Wall-clock and peak memory of a cruise with the rules `init` proposes | none | ok |  |  | 12.42 s |  |
<!-- testbeds:end -->

### Scale: wall-clock and peak memory on large repositories

Each night the scale repositories are built at their pinned commits, given the configuration `rulebearing init` writes, and cruised three times; the median run's wall-clock time and peak resident memory are below, aspnetcore read both from its compiled assemblies and from source, with the Stop hook's p95 on aspnetcore ([testbeds/bench/scale_table.py](testbeds/bench/scale_table.py), [plan 0003 Step 24](docs/plans/implemented/0003-wave-3-operations-surface-inner-loop.md#27-steps-for-sub-wave-3g-the-rule-library-the-scale-table-adoption)). The runner is GitHub's standard Linux runner: 4 CPUs, 16 GB of memory. A row more than 20% slower or larger than the baseline, the build behind the previous committed summary cruised beside it on the same runner, fails the night (hosted runners differ by more than 20% on the same binary from one night to the next); a row that did not complete keeps its last measurement, marked. The nightly rewrites this table by pull request.

<!-- scale-table:start -->
| Repository | .NET read as | Wall-clock | Peak memory | Modules | Measured in run |
| --- | --- | --- | --- | --- | --- |
| [n8n-io/n8n](https://github.com/n8n-io/n8n/tree/f89687fd8751970cdf8b9cb76761a1258728ca62) | - | 12.42 s | 1973 MB | javascript 1500, python 168, typescript 21244, unknown 4102 | [42206540369](https://github.com/benbahrenburg/rulebearing/actions/runs/42206540369) |
| [grafana/grafana](https://github.com/grafana/grafana/tree/423f85091b0a63a21fb571122a933133da1652cd) | - | 0.79 s | 174 MB | javascript 52, typescript 2077, unknown 351 | [42206540369](https://github.com/benbahrenburg/rulebearing/actions/runs/42206540369) |
| [elastic/kibana](https://github.com/elastic/kibana/tree/b502e688526bca862855361abf0081d84e65c547) | - | 9.85 s | 609 MB | javascript 271, typescript 6825, unknown 390 | [42206540369](https://github.com/benbahrenburg/rulebearing/actions/runs/42206540369) |
| [dotnet/aspnetcore](https://github.com/dotnet/aspnetcore/tree/b4be275a3b4fd83c304e7377403561a73eb4249e) | compiled | 30.96 s | 5617 MB | dotnet 10131, javascript 268, typescript 207, unknown 69 | [42206540369](https://github.com/benbahrenburg/rulebearing/actions/runs/42206540369) |
| [dotnet/aspnetcore](https://github.com/dotnet/aspnetcore/tree/b4be275a3b4fd83c304e7377403561a73eb4249e) | source | 6.04 s | 539 MB | dotnet 10131, javascript 268, typescript 207, unknown 69 | [42206540369](https://github.com/benbahrenburg/rulebearing/actions/runs/42206540369) |
| [jellyfin/jellyfin](https://github.com/jellyfin/jellyfin/tree/fd75964da853765d63101b4889f23a4161e2758b) | compiled | 4.76 s | 974 MB | dotnet 2317 | [42206540369](https://github.com/benbahrenburg/rulebearing/actions/runs/42206540369) |
| [home-assistant/core](https://github.com/home-assistant/core/tree/f50d777440e878554ef6872062341d24c6da0289) | - | 10.65 s | 2255 MB | javascript 3, python 22920, typescript 2, unknown 1 | [42206540369](https://github.com/benbahrenburg/rulebearing/actions/runs/42206540369) |

Stop hook on dotnet/aspnetcore in source mode: p95 1.872 s over 200 edits on 4 CPUs, run [42206540369](https://github.com/benbahrenburg/rulebearing/actions/runs/42206540369).
<!-- scale-table:end -->

### Oracle agreement: migrated rules against the incumbent

For the .NET and Python oracles the question is sharper than whether the tools agree on a graph: does a team's existing rule set, migrated with `rulebearing import archunit` or `rulebearing import import-linter`, give the verdict the incumbent gives, test by test and contract by contract. The harness in [testbeds/oracles](testbeds/README.md#oracle-harness) runs both sides at the pinned commit and writes one file per repository; `stays` is a test or contract the importer leaves with the incumbent (a custom predicate or contract type), `not imported` one it could not translate, each with its reason in the file. The table is generated from those files by `python3 testbeds/oracles/table.py --readme README.md`, and CI fails when it is stale.

<!-- oracles:start -->
| Repository | Incumbent | Compared | Agree | Disagree | Stays | Not imported | Errors | Graph (grimp vs Rulebearing) | Result |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| [ardalis/RiverBooks](testbeds/results/ardalis__RiverBooks.json) | archunitnet | 2 | 0 | 0 | 0 | 2 | 0 |  | nothing compared: every row stays or is not imported |
| [bridgecrewio/checkov](testbeds/results/bridgecrewio__checkov.json) | import-linter | 6 | 6 | 0 | 0 | 0 | 0 | 5499 = 5499 | agrees |
| [DrJohnMelville/Pdf](testbeds/results/DrJohnMelville__Pdf.json) | netarchtest |  |  |  |  |  |  |  | error: dotnet build Src/Melville.Pdf.DataModelTests/Melville.Pdf.DataModelTests.csproj failed (see build.log) |
| [evolutionary-architecture/evolutionary-architecture-by-example](testbeds/results/evolutionary-architecture__evolutionary-architecture-by-example.json) | netarchtest | 16 | 2 | 0 | 0 | 14 | 0 |  | agrees |
| [google/langextract](testbeds/results/google__langextract.json) | import-linter | 3 | 3 | 0 | 0 | 0 | 0 | 127 vs 154, 0 unexplained | agrees |
| [HKUDS/DeepTutor](testbeds/results/HKUDS__DeepTutor.json) | import-linter | 3 | 3 | 0 | 0 | 0 | 0 | 3485 vs 3613, 0 unexplained | agrees |
| [karaoke-dev/karaoke](testbeds/results/karaoke-dev__karaoke.json) | archunitnet | 5 | 0 | 0 | 0 | 5 | 0 |  | nothing compared: every row stays or is not imported |
| [kedro-org/kedro](testbeds/results/kedro-org__kedro.json) | import-linter | 4 | 4 | 0 | 0 | 0 | 0 | 222 vs 230, 0 unexplained | agrees |
| [langgenius/dify](testbeds/results/langgenius__dify.json) | import-linter | 37 | 37 | 0 | 0 | 0 | 0 | 6429 vs 8581, 0 unexplained | agrees |
| [microsoft/promptflow](testbeds/results/microsoft__promptflow.json) | import-linter | 1 | 1 | 0 | 0 | 0 | 0 | 557 vs 574, 0 unexplained | agrees |
| [nager/Nager.Date](testbeds/results/nager__Nager.Date.json) | archunitnet | 0 | 0 | 0 | 0 | 0 | 0 |  | error: dotnet test ran no tests (see incumbent.log) |
| [napari/napari](testbeds/results/napari__napari.json) | import-linter | 4 | 4 | 0 | 0 | 0 | 0 | 2423 vs 2764, 0 unexplained | agrees |
| [NeVeSpl/NetArchTest.eNhancedEdition](testbeds/results/NeVeSpl__NetArchTest.eNhancedEdition.json) | netarchtest | 355 | 0 | 0 | 0 | 355 | 0 |  | nothing compared: every row stays or is not imported |
| [nolar/kopf](testbeds/results/nolar__kopf.json) | import-linter | 11 | 10 | 0 | 1 | 0 | 0 | 373 = 373 | agrees |
| [onebeyond/monaco](testbeds/results/onebeyond__monaco.json) | archunitnet | 11 | 6 | 0 | 3 | 2 | 0 |  | agrees |
| [online-ml/river](testbeds/results/online-ml__river.json) | import-linter | 1 | 1 | 0 | 0 | 0 | 0 | 1237 = 1237 | agrees |
| [open-metadata/OpenMetadata](testbeds/results/open-metadata__OpenMetadata.json) | import-linter |  |  |  |  |  |  |  | error: import-linter could not run: ModuleNotFoundError: No module named 'pydantic' |
| [openedx/openedx-platform](testbeds/results/openedx__openedx-platform.json) | import-linter | 4 | 3 | 0 | 1 | 0 | 0 | 7098 vs 7097, 0 unexplained | agrees |
| [phongnguyend/Practical.CleanArchitecture](testbeds/results/phongnguyend__Practical.CleanArchitecture.json) | netarchtest | 5 | 5 | 0 | 0 | 0 | 0 |  | agrees |
| [seddonym/import-linter](testbeds/results/seddonym__import-linter.json) | import-linter | 2 | 2 | 0 | 0 | 0 | 0 | 84 = 84 | agrees |
| [sqlfluff/sqlfluff](testbeds/results/sqlfluff__sqlfluff.json) | import-linter | 4 | 4 | 0 | 0 | 0 | 0 | 985 = 985 | agrees |
| [wemake-services/wemake-python-styleguide](testbeds/results/wemake-services__wemake-python-styleguide.json) | import-linter | 6 | 6 | 0 | 0 | 0 | 0 | 530 = 530 | agrees |
<!-- oracles:end -->

## Built the way it asks you to build

This repository holds itself to the bar it proposes for yours. Every gate below was required and green before the first extractor line was written, so the extractors were built against the finished harness rather than alongside it.

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
| [docs/prd.md](docs/prd.md) | Requirements with fixed identifiers the code cites |
| [docs/architecture.md](docs/architecture.md) | Stages, crates, the graph document, extractors, security, performance |
| [docs/adr/](docs/adr/README.md) | Every decision, numbered |
| [docs/config.md](docs/config.md), [rules.md](docs/rules.md), [reporters.md](docs/reporters.md), [cli.md](docs/cli.md), [agents.md](docs/agents.md) | The user guides |
| [docs/perf.md](docs/perf.md), [docs/adoption.md](docs/adoption.md) | The performance and adoption measurements |
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
