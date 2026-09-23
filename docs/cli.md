# Command line

One binary, `rulebearing`. `rulebearing --help` and `rulebearing <command> --help` are the reference for every flag; this page says what each command is for. The help text is a committed snapshot ([crates/rb-cli/tests/help.txt](../crates/rb-cli/tests/help.txt)), because flag parity with dependency-cruiser is a promise ([ADR-0024](adr/0024-test-quality-gates.md)). Source: [design § The subcommands a guard reaches for](artifacts/design.md#the-subcommands-a-guard-reaches-for); the dependency-cruiser flags and their status are the [coverage tab § Command line](artifacts/dependency-cruiser-18.2.0-coverage.md#command-line).

## Commands

| Command | For | dependency-cruiser |
| --- | --- | --- |
| `cruise [paths]` | Extract, evaluate, report | `depcruise`, with the same flags and short forms (`-T`, `-f`, `-c`, `-I`, `-F`, `-R`, `-x`, `-X`, `-P`, `-p`, `-m`, `-i`) |
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
| `init` | A first configuration that passes on its first run, read from the repository rather than asked for | (`depcruise --init`, which asks questions, is a wave 2 row) |
| `adopt` | A dependency-cruiser repository behind a green gate with a baseline, in one pull request | |

`baseline`, `place`, `docs`, `import`, `propose` and `decisions` arrive in wave 2; `diff`, `guard`, `snapshot`, `changelog` and `serve` in wave 3. Each exits 2 now and names its wave.

## Flags the query commands share

| Flag | Meaning |
| --- | --- |
| `-c, --config [FILE]` | The configuration; `-` for stdin; no value finds it ([config.md](config.md#finding-the-file)) |
| `--no-config` | Run without one |
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
| 2 | The run cannot be trusted: no modules found, an unsupported file, a vacuous rule, a ratchet budget that cannot be read |
| 3 | The configuration is invalid |

A run with exactly two or three error violations also exits 2 or 3; the report says which it was ([ADR-0008](adr/0008-exit-code-contract.md), [ADR-0030](adr/0030-the-reporter-decides-the-error-count-exit.md)). `fmt` exits 0 unless `--exit-code` is given, as `depcruise-fmt` does. `can-import` exits 1 for "no". `attest --verify` exits 1 when a hash differs.

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

Its inputs are `version` (default: the version the action was referenced at), `args` (after `rulebearing cruise`) and `output-type` (default `github-annotations`). It checks the downloaded binary against the release's `SHA256SUMS` ([action.yml](../action.yml)).
