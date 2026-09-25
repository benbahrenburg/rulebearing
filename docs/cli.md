# Command line

One binary, `rulebearing`. `rulebearing --help` and `rulebearing <command> --help` are the reference for every flag; this page says what each command is for. The help text is a committed snapshot ([crates/rb-cli/tests/help.txt](../crates/rb-cli/tests/help.txt)), because flag parity with dependency-cruiser is a promise ([ADR-0024](adr/0024-test-quality-gates.md)). Source: [design § The subcommands a guard reaches for](artifacts/design.md#the-subcommands-a-guard-reaches-for); the dependency-cruiser flags and their status are the [coverage tab § Command line](artifacts/dependency-cruiser-18.2.0-coverage.md#command-line).

## Commands

| Command | For | dependency-cruiser |
| --- | --- | --- |
| `cruise [paths]` | Extract, evaluate, report | `depcruise`, with the same flags and short forms (`-T`, `-f`, `-c`, `-I`, `-F`, `-R`, `-H`, `-x`, `-S`, `-X`, `-P`, `-p`, `-m`, `-i`), `--webpack-config` and `--init [oneshot]` |
| `fmt <result.json>` | Re-report a saved result without extracting | `depcruise-fmt`, with its short forms (`-T`, `-f`, `-I`, `-F`, `-R`, `-H`, `-x`, `-S`, `-e`, `-p`) |
| `rules [--json]` | Every rule with its family, severity and match counts | |
| `explain <rule> [--plain]` | One rule in a sentence, with its reason, `fix` and first edges | |
| `test` | Each rule's `examples` checked against the rule | |
| `can-import <from> <to>` | Would this import be allowed, from the saved graph | |
| `count --from --to [--budget] [--write]` | Direct edges that match, against a ratchet budget that may only fall | |
| `config convert <file> [--to native\|dependency-cruiser]` | Translate between the formats; says what a lossy conversion dropped | |
| `config expand <file>` | A native file with `defines` substituted and shorthands expanded | |
| `config lint` | Rules that can never match, shadowed rules, missing `fix` text | |
| `hooks install --claude-code` | The three Claude Code hooks ([agents.md](agents.md)) | |
| `summary [--format agent\|text]` | The session brief: open violations, ratchet headroom, vacuous rules | |
| `impact <file> [--depth N] [--from-hook]` | What a file is subject to, before an edit | |
| `attest [--verify]` | Write or check a receipt of the configuration, inputs and results | |
| `init [--preset LANGUAGE]` | A first configuration that passes on its first run, read from the repository rather than asked for; the languages found (or named) choose the presets | `depcruise --init`, as `cruise --init` (below) |
| `adopt` | A dependency-cruiser repository behind a green gate with a baseline, in one pull request | |
| `baseline [paths] [--baseline-mode full\|shrink-only\|format] [--expires DATE --owner NAME --reason TEXT]` | Write the current violations to a known-violations file (default `.dependency-cruiser-known-violations.json`, `-f` to change it); [below](#baselines) | `depcruise-baseline`, which has one behaviour: `full` |

`cruise --init [oneshot]` is `depcruise --init` without the questions: it writes what `init` writes, to `--config FILE` or `rulebearing.yaml`, with `--preset typescript,dotnet,python` naming the languages instead of detecting them. `yes`, a bare `--init` and any other name write the configuration; `x-scripts` also adds `rulebearing`, `rulebearing:text` and `rulebearing:focus` run scripts to `package.json` after the existing ones and, as dependency-cruiser does, leaves an existing configuration be. dependency-cruiser's graph and HTML scripts need the `dot`, `archi` and `err-html` reporters and `wrap-html`, and are not written until those exist. One language extends its own preset first, `[rulebearing:python, rulebearing:recommended]`, so its exclusions win; several extend `rulebearing:recommended` ([config.md](config.md#presets)).

`diff`, `guard`, `snapshot`, `changelog` and `serve` arrive in wave 3. Each exits 2 now and names its wave.

## Baselines

A known-violations file is a JSON array of `knownViolations` entries, each keyed by the violation's stable `id` ([ADR-0015](adr/0015-stable-violation-id.md)) and carrying `expires`, `owner` and `reason` when given. `cruise --ignore-known [file]` reports the file's findings at severity `ignore` in place of `options.knownViolations`, as dependency-cruiser does; `--no-ignore-known` applies no known violations at all, the configuration's included; the last of the two on a command line wins. `fmt` takes the same two flags over a saved result: `--ignore-known` softens what the file lists, and `--no-ignore-known` puts every softened finding back at its rule's severity from `summary.ruleSetUsed`. The file name is optional, so give the paths first: `rulebearing cruise src --ignore-known`.

| Mode | Reads | Writes | Exits |
| --- | --- | --- | --- |
| `full` (default) | the tree | every current violation; an entry already in the file keeps its `expires`, `owner` and `reason` | 0 |
| `shrink-only` | the tree and the file (or, without one, `options.knownViolations`) | the file less the entries no violation matches any more, each printed; never adds one | the number of entries that no longer occur |
| `format` | the file only | the same entries, sorted by rule, `from`, `to` and `id` | 0 |

`shrink-only` is import-linter's unmatched-ignore alerting ([design § import-linter contracts](artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them)): run it in CI and a fixed finding fails the build until its entry leaves the baseline. `--expires`, `--owner` and `--reason` fill those fields on each entry written that lacks them; an entry past its `expires` date stops applying the day after and fails the run ([ADR-0031](adr/0031-a-saved-result-carries-what-the-exit-code-counts.md)).

## Flags the query commands share

| Flag | Meaning |
| --- | --- |
| `-c, --config [FILE]` | The configuration; `-` for stdin; no value finds it ([config.md](config.md#finding-the-file)) |
| `--no-config` | Run without one |
| `--liveness strict\|warn\|off` | `cruise` only. What a rule that matches nothing does: fail (exit 2), warn, or nothing. Default `strict` for a `rulebearing.*` file, `warn` for a dependency-cruiser one ([rules.md](rules.md#liveness)). `--no-liveness` is `off` |
| `--config-format native\|dependency-cruiser` | When the name does not say |
| `--config-via-node` | Evaluate a JavaScript configuration with the local Node, not the sandbox |
| `--strict-compat` | Refuse what dependency-cruiser would refuse |
| `--require-comment-token` | A rule without `adr:NNNN` or `plan:<slug>` in its comment is a configuration error |
| `--graph FILE` | Evaluate or query this graph document instead of extracting. Without it, the query commands read `.graph/cruise.json` when it exists |

`cruise --graph FILE` evaluates the rules over any graph document, which is how this repository checks its own crate boundaries: [`scripts/cargo-graph.sh`](../scripts/cargo-graph.sh) writes the Cargo workspace as a graph, and `cruise --config rulebearing.yaml --graph target/cargo-graph.json` evaluates it.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | No error-severity violation, or a reporter that does not gate |
| 1 to 255 | The number of error-severity violations (plus expired entries and exceeded ratchets), capped at 255, from a gating reporter: `err`, `err-long`, `null`, `teamcity`, `azure-devops`, `github-annotations`, `agent` |
| 2 | The run cannot be trusted: no modules found, an unsupported file, a vacuous rule under `strict` liveness, a ratchet budget that cannot be read |
| 3 | The configuration is invalid |

A run with exactly two or three error violations also exits 2 or 3; the report says which it was ([ADR-0008](adr/0008-exit-code-contract.md), [ADR-0030](adr/0030-the-reporter-decides-the-error-count-exit.md)). `fmt` exits 0 unless `--exit-code` is given, as `depcruise-fmt` does; with it, the code comes from the saved result alone (`summary.error`, `summary.expired`, the exceeded ratchets, and 2 for `vacuousRules` or a ratchet without a budget), so a saved result gates as the cruise would have ([ADR-0031](adr/0031-a-saved-result-carries-what-the-exit-code-counts.md)). `can-import` exits 1 for "no" and 2 when the target is unknown. `attest --verify` exits 1 when a hash differs. The `--from-hook` forms of `cruise` and `impact` always exit 0 ([agents.md](agents.md#the-hooks)).

## Pipelines

The design's TypeScript pipeline runs as written ([design § Three pipelines](artifacts/design.md#three-pipelines)):

```yaml
- run: rulebearing cruise --config .dependency-cruiser.cjs --metrics --output-type json apps packages > .graph/cruise.json
- run: rulebearing fmt --exit-code --output-type err .graph/cruise.json
- run: rulebearing count --from '^apps/([^/]+)/src/app/.*/(page|route)\.tsx?$' --to '^apps/$1/src/(server|domain)/' --budget eng/routes-via-service-budget.json
```

On GitHub, the Action runs `cruise` with annotations on the pull request:

```yaml
- uses: benbahrenburg/rulebearing@v0.1.0
  with:
    args: --config rulebearing.yaml src
```

Its inputs are `version`, `args` (after `rulebearing cruise`, on one line or several), `output-type` (default `github-annotations`) and `working-directory`, the folder to run in when the configuration is not at the repository root (a monorepo's `web/`, say): module paths stay relative to it, as dependency-cruiser's are, and each annotation is still placed by its path from the root. `version` defaults to the tag the action was referenced at; an action pinned by commit SHA, as [ADR-0025](adr/0025-ci-and-supply-chain-hardening.md) asks, must name it (`version: 0.1.0`, or `latest` to float on purpose), and the step fails rather than guess. It checks the downloaded binary against the release's `SHA256SUMS` ([action.yml](../action.yml)).
