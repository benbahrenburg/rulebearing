# Command line

One binary, `rulebearing`. `rulebearing --help` and `rulebearing <command> --help` are the reference for every flag; this page says what each command is for. The help text is a committed snapshot ([crates/rb-cli/tests/help.txt](../crates/rb-cli/tests/help.txt)), because flag parity with dependency-cruiser is a promise ([ADR-0024](adr/0024-test-quality-gates.md)). Source: [design § The subcommands a guard reaches for](artifacts/design.md#the-subcommands-a-guard-reaches-for); the dependency-cruiser flags and their status are the [coverage tab § Command line](artifacts/dependency-cruiser-18.2.0-coverage.md#command-line).

## Commands

| Command | For | dependency-cruiser |
| --- | --- | --- |
| `cruise [paths]` | Extract, evaluate, report | `depcruise`, with the same flags and short forms (`-T`, `-f`, `-c`, `-I`, `-F`, `-R`, `-H`, `-x`, `-S`, `-X`, `-P`, `-p`, `-m`, `-i`, `-A`), `--webpack-config`, `--affected [revision]` and `--init [oneshot]` |
| `fmt <result.json>` | Re-report a saved result without extracting | `depcruise-fmt`, with its short forms (`-T`, `-f`, `-I`, `-F`, `-R`, `-H`, `-x`, `-S`, `-e`, `-p`) |
| `rules [--json]` | Every rule with its family, severity, lifecycle fields and match counts | |
| `rules --unused [--releases N]` | The rules that matched nothing on either side in each of the last `N` snapshots; [below](#snapshots-changelog-and-unused-rules) | |
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
| `init [--preset PRESET]` | A first configuration that passes on its first run, read from the repository rather than asked for; the languages found (or named) choose the presets, and a framework preset (`nextjs`, `clean-architecture`, `django`, `fastapi`, `vertical-slices`) is added only when named | `depcruise --init`, as `cruise --init` (below) |
| `adopt` | A dependency-cruiser repository behind a green gate with a baseline, in one pull request | |
| `baseline [paths] [--baseline-mode full\|shrink-only\|format] [--expires DATE --owner NAME --reason TEXT]` | Write the current violations to a known-violations file (default `.dependency-cruiser-known-violations.json`, `-f` to change it); [below](#baselines) | `depcruise-baseline`, which has one behaviour: `full` |
| `diff <old.json> <new.json>`, `diff --base <ref> [paths]` | Added and removed edges, new and resolved violations and moved ratchets, as `json`, `markdown` or `agent`; [below](#diff) | |
| `snapshot [--version V]` | A summary of the architecture at a release, under `.graph/snapshots/`; [below](#snapshots-changelog-and-unused-rules) | |
| `changelog --since V [--to V]` | New edges across boundaries, retired rules and ratchets that fell between two snapshots, as `markdown` or `json`; [below](#snapshots-changelog-and-unused-rules) | |
| `wrap-html` | An SVG read from stdin, written between the header and the footer of the page `x-dot-webpage` writes; [below](#wrap-html) | `depcruise-wrap-stream-in-html` |

`cruise --init [oneshot]` is `depcruise --init` without the questions: it writes what `init` writes, to `--config FILE` or `rulebearing.yaml`, with `--preset typescript,dotnet,python` naming the languages instead of detecting them. `yes`, a bare `--init` and any other name write the configuration; `x-scripts` also adds `rulebearing`, `rulebearing:text` and `rulebearing:focus` run scripts to `package.json` after the existing ones and, as dependency-cruiser does, leaves an existing configuration be. dependency-cruiser's graph and HTML scripts need the `dot`, `archi` and `err-html` reporters and `wrap-html`, and are not written until those exist. One language extends its own preset first, `[rulebearing:python, rulebearing:recommended]`, so its exclusions win; several extend `rulebearing:recommended` ([config.md](config.md#presets)).

`--preset` also takes the framework presets, which are opinions and off unless named ([presets/frameworks](../presets/frameworks/README.md)): `init --preset nextjs` keeps the languages found and adds `rulebearing:nextjs` after them, `init --preset python,django` names both. A framework preset's rules get init's usual treatment: a rule whose `from` side matches nothing in the repository is left out with a `severity: ignore` entry that names it (delete the entry to turn it on), and every current finding is baselined, so the configuration passes on its first run.

`guard` and `serve` arrive later in wave 3. Each exits 2 now and names its wave.

## Baselines

A known-violations file is a JSON array of `knownViolations` entries, each keyed by the violation's stable `id` ([ADR-0015](adr/0015-stable-violation-id.md)) and carrying `expires`, `owner` and `reason` when given. `cruise --ignore-known [file]` reports the file's findings at severity `ignore` in place of `options.knownViolations`, as dependency-cruiser does; `--no-ignore-known` applies no known violations at all, the configuration's included; the last of the two on a command line wins. `fmt` takes the same two flags over a saved result: `--ignore-known` softens what the file lists, and `--no-ignore-known` puts every softened finding back at its rule's severity from `summary.ruleSetUsed`. The file name is optional, so give the paths first: `rulebearing cruise src --ignore-known`.

| Mode | Reads | Writes | Exits |
| --- | --- | --- | --- |
| `full` (default) | the tree | every current violation; an entry already in the file keeps its `expires`, `owner` and `reason` | 0 |
| `shrink-only` | the tree and the file (or, without one, `options.knownViolations`) | the file less the entries no violation matches any more, each printed; never adds one | the number of entries that no longer occur |
| `format` | the file only | the same entries, sorted by rule, `from`, `to` and `id` | 0 |

`shrink-only` is import-linter's unmatched-ignore alerting ([design § import-linter contracts](artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them)): run it in CI and a fixed finding fails the build until its entry leaves the baseline. `--expires`, `--owner` and `--reason` fill those fields on each entry written that lacks them; an entry past its `expires` date stops applying the day after and fails the run ([ADR-0031](adr/0031-a-saved-result-carries-what-the-exit-code-counts.md)).

## diff

`diff` answers what a change did to the architecture, for a review comment ([design § The subcommands a guard reaches for](artifacts/design.md#the-subcommands-a-guard-reaches-for)). `diff old.json new.json` compares two saved results, Rulebearing's or dependency-cruiser's, and reads nothing else. `diff --base <ref> [paths]` compares the cruise of a revision with the cruise of the working tree:

```sh
rulebearing cruise -T json -f base.json      # on the base branch
rulebearing cruise -T json -f head.json      # on the change
rulebearing diff base.json head.json -T markdown
rulebearing diff --base main -T agent        # the same in one step, against the working tree
```

| Section | Holds |
| --- | --- |
| `addedEdges`, `removedEdges` | Each edge (`from` the module, `to` the resolved dependency) on one side only, with the line and column of its first import on the side that has it |
| `newViolations`, `resolvedViolations` | Each violation whose stable id ([ADR-0015](adr/0015-stable-violation-id.md)) is on one side only, with `rule`, `severity`, `from`, `to`, the edge's position and the rule's `fix`. A result without ids (dependency-cruiser's) has them computed as the engine computes them; since it carries no `dependencyKind` either, when either side lacks ids both sides are matched on the id over the rule and the two ends with an empty kind, so dependency-cruiser's result and Rulebearing's of the same tree agree. Each finding still reports its own side's id. Violations at severity `ignore` (known violations) are not findings and are left out |
| `ratchets` | Each ratchet of `summary.ratchets[]` ([ADR-0029](adr/0029-ratchets-enforced-by-cruise-and-reported-in-the-summary.md)) whose count changed, with `before` and `after`; a ratchet on one side only has the other count absent. Unchanged ratchets are not listed |
| `base`, `head` | The revision and commit of each side when known: `--base` gives both for the base and the commit of `HEAD` for the working tree; a saved result gives the commit its `revisionData.SHA1` records. A side with nothing known is left out, never `null` |

```json
{
  "base": { "revision": "main", "sha": "3f2a..." },
  "head": { "sha": "8e1d..." },
  "addedEdges": [ { "from": "src/routes/b.ts", "to": "src/web/view.ts", "line": 1, "column": 1 } ],
  "removedEdges": [ { "from": "src/routes/b.ts", "to": "src/db/store.ts", "line": 1, "column": 1 } ],
  "newViolations": [ { "id": "RB-915449e6", "rule": "routes-not-to-web", "severity": "error", "from": "src/routes/b.ts", "to": "src/web/view.ts", "line": 1, "column": 1, "fix": "Return data from the route and render it in src/web" } ],
  "resolvedViolations": [ { "id": "RB-e37cce41", "rule": "routes-not-to-db", "severity": "error", "from": "src/routes/b.ts", "to": "src/db/store.ts", "line": 1, "column": 1, "fix": "Call the store through src/services instead of importing src/db" } ],
  "ratchets": [ { "name": "routes-via-service", "before": 2, "after": 1 } ]
}
```

Every list is sorted (edges by `from` and `to`, violations by rule, `from`, `to` and id, ratchets by name), so two runs print the same bytes. `-T markdown` writes a table per section and one line for an empty one; it is the body of the pull-request comment. `-T agent` writes one line per new violation with its position and `fix`, then one line of counts. The committed renderings of one change are in [crates/rb-cli/tests/fixtures/diff](../crates/rb-cli/tests/fixtures/diff/expected.md).

With `--base`, both sides run the whole cruise (extraction, evaluation, ratchets) under one configuration, the one the configuration flags find in the working tree, so the diff shows what the code changed rather than what an edit to the rules did. Liveness is off on both sides, since a rule that matches nothing is `cruise`'s finding. The base graph is read from the cache (`.graph/cache`) when an entry for that commit, configuration and build exists. Otherwise the revision is checked out with `git worktree add --detach` into a folder the command creates for itself under the system temporary directory (a fresh name, private to the user, never an entry that was already there), with git hooks off (`core.hooksPath` is an empty folder the command creates for itself, private to the user, and removes afterwards), extracted from the same subfolder the command runs in, and removed again, on an error too; the extracted graph is then cached for the next run. A path given that the base does not have yet, or a base with no module at all, is an empty base graph, so everything on the working-tree side is added. The working tree is never touched. `--no-cache` checks the base out without reading or writing the entry. The configuration flags and `--no-cache` apply to `--base` only; with two saved results they are refused (exit 3) rather than ignored.

A checkout holds what git tracks and nothing else. Edges into installed packages (`node_modules`) resolve differently there than in a working tree where the packages are installed, and a .NET solution has no assemblies to read until it is built (the base side then exits 2 with the extractor's reason). For those repositories, cruise each revision where it is installed and built, and compare the two results.

`diff` is a report and exits 0, as the reporters that do not gate do ([ADR-0030](adr/0030-the-reporter-decides-the-error-count-exit.md)). `--exit-code` (`-e`) makes it gate on what the change introduced: the exit code is the number of new error-severity violations, capped at 255, and `--exit-code-mode strict` makes it 10 plus that number, as on `cruise` and `fmt`, so 2 and 3 are never a count. An input that cannot be read or is not a cruise result, an unknown revision, a folder outside a git repository with `--base`, and a side that cannot be cruised exit 2 with the reason; an invalid configuration, an output type other than the three, or a wrong number of results exit 3.

## Snapshots, changelog and unused rules

A rule file grows and never shrinks unless something shows which rules stopped earning their place; a pull request shows one change, and drift shows only across releases ([design § The architect's hat](artifacts/design.md#the-architects-hat-across-repos-and-across-time)). Three commands read the history a repository keeps under `.graph/snapshots/`, one snapshot per release ([plan 0003, Steps 12 and 13](plans/pending/0003-wave-3-operations-surface-inner-loop.md#23-steps-for-sub-wave-3c-presets-lifecycle-fields-snapshot-and-changelog)):

```sh
rulebearing snapshot --version 1.3.0                 # at the release; commit .graph/snapshots/1.3.0.json
rulebearing changelog --since 1.2.0                  # 1.2.0 to the newest snapshot, as Markdown
rulebearing changelog --since 1.2.0 --to 1.3.0 -T json
rulebearing rules --unused --releases 3              # rules that matched nothing in each of the last 3
```

**`snapshot [--version V] [paths]`** extracts the paths afresh (the working directory by default), or reads the result `--graph FILE` names, and evaluates it with the configuration the flags find, liveness off and the folder metrics on. Unlike the query commands it never falls back to a saved `.graph/cruise.json`, which may come from any commit: a release record describes the tree at its commit. Pass `--graph` only with a result of that release. It writes two files:

| File | Holds |
| --- | --- |
| `.graph/snapshots/<V>.json` | The snapshot below |
| `.graph/snapshots/<V>.cruise.json` | The cruise result, as `cruise -T json` writes it; `changelog` takes the edges from it |

```json
{ "version": "1.3.0", "sha": "...", "counts": { "modules": 5574, "dependencies": 21930, "violations": { "error": 0, "warn": 3, "info": 0 } },
  "instability": { "apps/web": 0.42, "src/Domain": 0.05 },
  "rules": { "no-cross-app-imports": { "fromMatches": 412, "toMatches": 412, "violations": 0 } },
  "ratchets": { "routes-via-service": 11 } }
```

The shape is exactly this. `counts.violations` has `error`, `warn` and `info`, each present and 0 when there are none; `sha` is `null` when the commit is not known. Both go beyond the plan's first sketch (`{ error, warn }`, a string `sha`) and are additive, as the graph document's additions are ([ADR-0004](adr/0004-graph-document-is-cruise-result-superset.md)). `rules` holds the dependency rules with the statistics `rules --json` prints; element, slice and diagram rules have no match counts and are not in it. `instability` is each folder's from `folders[]`; `ratchets` is each ratchet's edge count. `sha` is the commit the graph records (`revisionData.SHA1`), else `HEAD`, else `null`. The version is `--version`, or else the git tag at `HEAD` (the latest when there are several, so `v1.3.0` wins over `v1.3.0-rc.1`); with neither the command exits 3. The version names the files, so it is 1 to 128 letters, digits, `.`, `_`, `+` and `-`, does not start with `.` or `-` and does not end in `.cruise`. Every map is sorted and a metric prints as JavaScript prints it, so the same graph writes the same bytes; writing a version again replaces its files. A snapshot must record the version its file is named for: `changelog` and `rules --unused` refuse, exit 2 naming the file, a snapshot whose `version` differs from its file name (it would be paired with another release's cruise result) or is not a valid version (a path such as `../x` would reach outside the folder). `snapshot` exits 0, 2 when the graph cannot be read or extracted, and 3 for an invalid version or configuration.

**Versions and their order.** A version is any string. When every version in play is semver (`1.2.0`, `v1.2.0`, `1.3.0-rc.1`), they are ordered as semver: a prerelease before its release, build metadata ignored. Otherwise they are ordered naturally, numbers by value, so `2026.10` follows `2026.9` ([`rb_config::version`](../crates/rb-config/src/version.rs)). "The newest" and "the last N" mean in that order, never by file name.

**`changelog --since V [--to V]`** reads the snapshot of `--since` and that of `--to`, by default the newest. It does not cruise: to compare a release with the working tree, snapshot the working tree first. The Markdown has four sections, and `-T json` the same content:

| Section | Holds |
| --- | --- |
| Counts | Modules, dependencies and violations by severity at each release, and the change |
| New edges across boundaries | The edges [`diff`](#diff) finds added between the two cruise results whose ends fall in different layers of a `layers` entry, or in different slices of a slice rule that slices paths (a `matching` with `/`). A slice rule over namespaces or dotted module names is not applied, since a cruise result's edge joins two files. When either `<V>.cruise.json` is missing the section says which, and the JSON has `newEdgesAcrossBoundaries: null` |
| Retired rules | Rules the older snapshot records and the newer does not, and the rules, `layers` and `independence` entries and ratchets of the configuration whose `deprecated` release is after `--since` and not after `--to`, with their `replacedBy`. A `layers` entry is listed once, not once per rule it expands to. `v1.1.0` and `1.1.0` are the same release here |
| Ratchets that fell | Ratchets both snapshots record whose count is lower at `--to` |

The layers, slices and lifecycle fields come from the configuration in the working tree. A committed example is [crates/rb-cli/tests/fixtures/lifecycle/expected-changelog.md](../crates/rb-cli/tests/fixtures/lifecycle/expected-changelog.md). `changelog` is a report and exits 0; a missing snapshot or an unreadable cruise result exits 2 naming the file, and an invalid configuration or output type exits 3.

**`rules --unused [--releases N]`** reads the last `N` snapshots (default 3) and lists the dependency rules of the current configuration that each of them records with zero `fromMatches` and zero `toMatches`. A rule a snapshot does not record did not exist at that release and is not listed. With fewer than `N` snapshots it prints `insufficient history` and exits 0 rather than guess. `--json` prints `releases`, `insufficientHistory` and the `unused` rules as `rules --json` prints them. Listing a rule changes nothing about it: a rule that matches nothing still fails `cruise` as vacuous unless it has `allowEmpty` ([ADR-0007](adr/0007-vacuous-rules-fail-by-default.md)). Delete it, or mark it `deprecated` with a `replacedBy` ([config.md § Rule metadata](config.md#rule-metadata)) so the next changelog says so. `--graph` and paths are refused with `--unused` (exit 3), since it reads no graph.

**Committing the snapshots.** The snapshots are the history, so they belong in version control, while the rest of `.graph/` (the cache, `cruise.json`) does not. A repository that ignores `.graph/` keeps the snapshots with an exception; the cruise results are larger, and committing them is what lets a later `changelog` list the new edges:

```gitignore
/.graph/*
!/.graph/snapshots/
# Leave this line out to keep the edges for changelog:
/.graph/snapshots/*.cruise.json
```

## wrap-html

`wrap-html` is `depcruise-wrap-stream-in-html`: it writes the header of the `x-dot-webpage` page (its stylesheet and hint box), then standard input unchanged, then the footer (the highlighting script), so a graph drawn by GraphViz becomes the interactive page without running a reporter ([coverage tab § Command line](artifacts/dependency-cruiser-18.2.0-coverage.md#command-line)):

```sh
rulebearing cruise src -T dot | dot -T svg | rulebearing wrap-html > dependency-graph.html
```

The input is streamed, not held in memory, and copied byte for byte. The page is byte-identical to the one dependency-cruiser 18.2.0 writes for the same input ([crates/rb-cli/tests/fixtures/wrap-html](../crates/rb-cli/tests/fixtures/wrap-html/expected.html)). It exits 0, or 2 when standard input or output fails.

## Affected runs

`cruise --affected [revision]` (`-A`) reports only the modules changed since `revision` (default `main`) and every module that reaches them. What it reports follows the configuration format ([ADR-0052](adr/0052-affected-is-upstreams-reaches-filter.md)), as liveness does.

With a dependency-cruiser configuration, or none, it does what dependency-cruiser does ([coverage § Command line](artifacts/dependency-cruiser-18.2.0-coverage.md#command-line), row `--affected [revision]`). The changes are what `git diff <revision> --name-status` and the untracked files of `git status --porcelain` list, with the extensions dependency-cruiser lists; they become the `reaches` expression, so `summary.optionsUsed.reaches` and the report are dependency-cruiser's. That has three consequences worth knowing: an edge from a changed module to an unchanged one that does not reach a changed module is not in the report, so neither is a violation on it; a deleted file is not in the expression; and a new file in a new, untracked folder counts once it is staged, because `git status` reports the folder rather than the file.

With a `rulebearing.*` configuration, the rules are evaluated over the whole graph and the report keeps the closure's modules with every edge they have, and every violation that touches the closure: its `from` module is in it, or, for a cycle or reachability violation, a module of its path. An edited file's import of an unchanged, forbidden module is reported. Each file of a new untracked folder counts, every changed file that is a module counts whatever its extension, the importers of a deleted or renamed file are read from `.graph/cruise.json` when it exists, and so are the modules a changed file used to import, which join the report without their dependents (dropping a module's last importer makes it an orphan). A slice violation is reported when one of its member edges touches the closure. No `reaches` expression is set.

| Addition, in both modes | What it does |
| --- | --- |
| .NET and Python | A changed Python module, a changed .NET module, and a changed file the PDB attributes a type to (either file of a partial class) count as changed modules |
| `--affected-depth N` | Only the modules that reach a changed one in at most `N` steps, by the shortest path; `0`, the default, keeps them all |
| Git settings | `core.quotePath` and `diff.relative` do not change what counts, so a non-ASCII path counts where dependency-cruiser misses it; a revision that is not a commit (a directory name, say) exits 2 |
| `summary.affected` | The receipt: `revision`, every `changed` path (deleted files included), the `closure` the report kept, and `depth` when given; `--strict-schema` strips it |
| Paths | Relative to the directory the cruise runs in, as module names are; dependency-cruiser keeps git's repository-relative paths, so its cruise from a subdirectory matches nothing |

A gating reporter exits with the error count of what the report kept. A revision git does not know, or a directory outside a git repository, exits 2. `options.affected` in a `rulebearing.*` configuration applies as the flag would; in a dependency-cruiser configuration it is ignored with a warning, as dependency-cruiser ignores it.

## The cache

`cruise --cache [folder]` keeps the extraction and, on the next run, reads again only what changed ([coverage § Command line](artifacts/dependency-cruiser-18.2.0-coverage.md#command-line) rows `--cache [folder]`, `--cache-strategy`, `--no-cache`; [Wave 3, Steps 1 and 2](plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)). The rules are evaluated afresh on every run, so a cached run reports exactly what a cold run reports.

| Flag | Meaning |
| --- | --- |
| `-C, --cache [FOLDER]` | Use the cache in `FOLDER`, relative to the working directory. Without a value: `.graph/cache`, or `node_modules/.cache/dependency-cruiser` under a dependency-cruiser configuration, as dependency-cruiser does. Replaces `options.cache` ([config.md](config.md#the-cache)) |
| `--cache-strategy metadata\|content` | How a change is found. `metadata` (the default): `git` lists what changed since the recorded commit, and only files whose size or modification time moved are hashed. `content`: every input is hashed. Turns the cache on by itself |
| `--no-cache` | No cache, whatever `options.cache` or `--cache` say |

The folder holds `manifest.json` (the build's version, a `sha256:` hash of the configuration and the extraction settings, the worktree, `HEAD`, the strategy, and a `sha256:` hash per input) and the extraction it names. Before anything is reused the manifest must match this build, this configuration, this worktree and this strategy, and the extraction must match its recorded hash; otherwise the entry is ignored and rewritten, never trusted, and a folder that cannot be written is a warning. What happens next depends on what changed:

| What changed | What runs |
| --- | --- |
| nothing an extractor reads | nothing is read: the stored extraction is used |
| TypeScript or Python sources | those files are read again; the rest come from the cache |
| an assembly or a PDB | the .NET graph is read again whole, so edges between assemblies stay exact |
| anything else the run read or looked for: a file added or deleted, a manifest (`package.json` anywhere, the tsconfig and every file its `extends` chain and references name, the Babel configuration, project, solution and lock files), an entry added to or removed from the folder a relative import points into, a package installed under `node_modules` (its `.package-lock.json`, `.modules.yaml` or `.yarn-state.yml`), a .NET project built since, the Python environment (its `site-packages` and distributions), or git unable to say | everything is read again |

The inputs are recorded after the extraction that read them. A file modified after the run started (an editor saving mid-run, say) is recorded as unsettled and read again on the next run, rather than trusted as the bytes the extraction saw.

When nothing an extractor reads changed, the entry can also answer with the evaluated run, as dependency-cruiser's cache does: `evaluated.json` names the run as the reporter receives it, keyed on the extraction and everything evaluation reads besides (the configuration, the options after the flags and `optionsUsed`, the known violations in force, the liveness mode, the paths, today's date, every ratchet budget and every diagram rule's `.puml`), and `rendered.json` with `rendered.out` keeps the reporter's output for the output type and report options last used (the timestamp counts only for `err-html`, `junit`, `trx` and `teamcity`, which print it). Either is used only when its key matches; a changed input misses it and the run evaluates again from the cached extraction. `--affected` does not use the evaluated layer, because its changed files come from version control rather than from the inputs the cache records.

**The cache folder is trusted input.** A run that finds a matching entry reports from it, so an entry forged or carried from elsewhere can make a gate pass. Keep the folder private to the machine that wrote it: do not commit it, and do not restore it in CI from a cache another branch, fork or pull request could have written. `.graph/` belongs in `.gitignore`. A damaged entry is a miss, never a panic, and no file of an entry is read past 512 MiB, compressed or inflated ([SECURITY.md](../SECURITY.md#what-the-tool-does-and-does-not-do)).

`summary.cache` records `{ "hit": true | false, "strategy": ... }`, so a JSON result from a warm run differs from a cold run's in that field alone; `--strict-schema` removes it. `--progress` names how the extract stage was served. Outside a git repository the `metadata` strategy lists files and compares their size and modification time instead of stopping as dependency-cruiser does. A file edited without changing its size or time, and not listed by git, is only seen by `content`.

```sh
rulebearing cruise --cache -T err src                    # .graph/cache, metadata strategy
rulebearing cruise --cache /tmp/rb --cache-strategy content -T json src
```

Under `--sidecar node` a CoffeeScript or LiveScript file is one of the files read again when it changes: the sidecar is run for the changed ones only, and the rest are reused. The flag is part of the entry's key, so an entry written with it is never used without it, nor the reverse.

## CoffeeScript and LiveScript: `--sidecar node`

`cruise --sidecar node` extracts `.coffee`, `.litcoffee`, `.coffee.md`, `.ls`, `.cjsx` and `.csx` files by running the repository's own dependency-cruiser with Node, and merges what it finds into the graph ([coverage § Extraction and resolution](artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution), row "CoffeeScript, LiveScript"; [ADR-0017](adr/0017-coffeescript-livescript-sidecar.md); [Wave 3, Step 10](plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)). It is, with `--config-via-node`, the only way the binary starts Node, and it never does without the flag.

| Situation | Result |
| --- | --- |
| No flag, and the walk reaches such a file | exit 2: `<file>: unsupported-file-needs-sidecar: ...`, naming `--sidecar node` or excluding the file as the fix |
| A `.csx` file, with or without the flag | the same, and the reason adds that `.csx` is also the C# script extension (dotnet-script): a C# script is not CoffeeScript, so the fix is to exclude it (`options.exclude`, or `--exclude "\.csx$"`), not to run the sidecar. The file is never skipped silently ([ADR-0017](adr/0017-coffeescript-livescript-sidecar.md)) |
| No flag, and the file is only an unfollowed dependency | nothing: the file is never read |
| The flag, no `node_modules/dependency-cruiser` in the repository or above it | exit 2, naming the `npm install --save-dev dependency-cruiser@18.2.0 coffeescript` that fixes it |
| The flag, no Node (`node` on the path, or `$RULEBEARING_NODE`) | exit 2, naming Node 22 or later and `RULEBEARING_NODE` |
| The flag, and dependency-cruiser cannot load `coffeescript` or `livescript` | exit 2, naming the package to install; dependency-cruiser would otherwise read the file as JavaScript without saying so |
| The flag, and a dependency-cruiser other than 18.2.0 | the run goes on with a warning; the edges are proven against 18.2.0 only |

dependency-cruiser is found as Node would find it from the working directory: `node_modules/dependency-cruiser` there or in a folder above, or the folder itself when it is a dependency-cruiser checkout. It runs its own command line with `--output-type json`, then `--` and the files the walk reached (a file whose name starts with `-` is also passed as `./<file>`, so no file name is ever read as an option), and a configuration written with mode 0600 into a folder of its own, created fresh with mode 0700 under the system's temporary folder and removed after the run: the run's TypeScript options, whether the configuration was a dependency-cruiser file or a `rulebearing.yaml` (whose `languages.typescript` block uses dependency-cruiser's names), with `maxDepth` `0` and none of the run's rules. The run's rules are evaluated by Rulebearing over the merged graph, as for every other file.

The walk stays Rulebearing's: each CoffeeScript or LiveScript file is extracted by dependency-cruiser, and the walk continues natively from its dependencies, so a JavaScript file a CoffeeScript file imports is read once, by `oxc`. Every dependency of a sidecar file carries `sidecar: true` and no `line` or `column`; no other dependency has the field. `summary.sidecar` records `{ "tool": "dependency-cruiser", "version": "18.2.0", "files": N }`, N being the files of the graph the sidecar extracted. `--strict-schema` removes both. The flag is not a dependency-cruiser option, so it is not in `optionsUsed`, and a configuration file cannot set it.

```sh
npm install --save-dev dependency-cruiser@18.2.0 coffeescript   # livescript for .ls
rulebearing cruise --sidecar node -T err src
```

## .NET without a build: `--mode source`

`cruise --mode source` (or `languages.dotnet.mode: source`) reads .NET from the `.cs` files with `tree-sitter-c-sharp` instead of the built assemblies: nothing needs to be built, every .NET edge is namespace-level and marked `approximate: true`, and `summary.inspected.dotnet` records `mode: source`. Compiled mode stays the gate ([ADR-0011](adr/0011-read-dotnet-assemblies-not-source.md)); [source-mode.md](source-mode.md) says what is read, how names are resolved and how close the result is to a compiled one.

```sh
rulebearing cruise --mode source --cache -T agent
```

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

A run with exactly two or three error violations also exits 2 or 3; the report says which it was ([ADR-0008](adr/0008-exit-code-contract.md), [ADR-0030](adr/0030-the-reporter-decides-the-error-count-exit.md)). `--exit-code-mode strict`, on `cruise` and on `fmt --exit-code`, removes the ambiguity for a pipeline that needs it:

| Outcome | `default` | `strict` |
| --- | --- | --- |
| no error-severity violation | 0 | 0 |
| `n` error-severity violations | `n`, capped at 255 | `10 + n`, capped at 255 |
| the run cannot be trusted | 2 | 2 |
| the configuration is invalid | 3 | 3 |

`fmt` exits 0 unless `--exit-code` is given, as `depcruise-fmt` does; with it, the code comes from the saved result alone (`summary.error`, `summary.expired`, the exceeded ratchets, and 2 for `vacuousRules` or a ratchet without a budget), so a saved result gates as the cruise would have ([ADR-0031](adr/0031-a-saved-result-carries-what-the-exit-code-counts.md)). `diff` exits 0 unless `--exit-code` is given, then with the number of new error-severity violations, or 10 plus it with `--exit-code-mode strict` ([above](#diff)). `can-import` exits 1 for "no" and 2 when the target is unknown. `attest --verify` exits 1 when a hash differs. The `--from-hook` forms of `cruise` and `impact` always exit 0 ([agents.md](agents.md#the-hooks)).

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
