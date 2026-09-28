# Reporters

`--output-type` (`-T`) picks the reporter on `cruise` and on `fmt`, which re-reports a saved result without extracting again, as `depcruise-fmt` does. dependency-cruiser's reporters are ported and byte-compared against its own `test/report` specs ([conformance gate 1, layer 3](../conformance/README.md#gate-1-layer-by-layer)). Source: [design § Reporters](artifacts/design.md#reporters); the full list with waves is the [coverage tab § Output types](artifacts/dependency-cruiser-18.2.0-coverage.md#output-types).

## Wave 1

| Output type | Writes | Gates |
| --- | --- | --- |
| `err` | One line per violation; the default for `fmt` | yes |
| `err-long` | `err`, with each rule's comment and `fix` under its findings | yes |
| `json` | The result document ([below](#the-json-document)) | no |
| `text` | One line per edge, `from → to` | no |
| `csv` | A dependency matrix | no |
| `teamcity` | TeamCity service messages, one inspection type per rule | yes |
| `azure-devops` | Azure Pipelines logging commands and a task result | yes |
| `github-annotations` | One GitHub workflow command per violation, so each appears on its line in the pull request | yes |
| `agent` | JSON for a coding agent: violations grouped by rule, cheapest fix first, within a budget | yes |
| `null` | Nothing; the exit code only | yes |

**Gates** means the exit code is the number of error-severity violations. The others exit 0 on a trustworthy run, as in dependency-cruiser, so a pipeline can save JSON in one step and gate in the next ([ADR-0030](adr/0030-the-reporter-decides-the-error-count-exit.md)). Every reporter exits 2 on a run that cannot be trusted and 3 on an invalid configuration ([ADR-0008](adr/0008-exit-code-contract.md)).

## Wave 2

| Output type | Writes | Gates |
| --- | --- | --- |
| `baseline` | Every violation as a `knownViolations` entry, the file `rulebearing baseline` writes ([cli.md § Baselines](cli.md#baselines)) | no |
| `sarif` | SARIF 2.1.0 for code scanning ([below](#sarif)) | no |
| `junit` | JUnit XML, one test case per rule ([below](#junit-and-trx)) | no |
| `trx` | Visual Studio TRX, one unit test per rule | no |

The rest of wave 2 is the graph reporters (`dot`, `ddot`, `archi` / `cdot`, `flat` / `fdot`, `mermaid`, `d2`), `metrics` and `err-html`; none gates.

## Wave 3

| Output type | Writes | Gates |
| --- | --- | --- |
| `markdown` | The summary, the rules with their counts, and every violation as Markdown, for a pull-request comment or a job summary | no |
| `html` | The dependency matrix as one HTML page: a row and a column per module, each cell coloured by the edge and the severity of the first rule it breaks | no |
| `anon` | `json` with every module name anonymised ([below](#anon)) | no |
| `x-dot-webpage` | The `dot` graph drawn by GraphViz as SVG, in an HTML page with hover highlighting ([below](#x-dot-webpage)) | no |

Each is byte for byte dependency-cruiser 18.2.0's: gate 1 layer 3 runs upstream's `test/report/{markdown,html,anon,dot-webpage}` specs and renders every `test/report` mock through both implementations. `plantuml` arrives later in wave 3; asking for it now exits 2 and names the wave.

### `markdown`

`reporterOptions.markdown` takes dependency-cruiser's keys with its defaults: `showTitle`, `title`, `showSummary`, `showSummaryHeader`, `summaryHeader`, `showStatsSummary`, `showRulesSummary`, `includeIgnoredInSummary`, `showDetails`, `includeIgnoredInDetails`, `showDetailsHeader`, `detailsHeader`, `collapseDetails`, `collapsedMessage`, `noViolationsMessage`, `showFooter`, `footer`, `showExternalModulesUnresolved` and `showAliasedModulesUnresolved` ([coverage tab § Options](artifacts/dependency-cruiser-18.2.0-coverage.md#options)). As upstream, `showStatsSummary` is accepted and has no effect (upstream's reporter always writes the statistics line), and a key given as `null` replaces its default. The default footer names dependency-cruiser 18.2.0, whose output this reproduces, and the run's time, which `SOURCE_DATE_EPOCH` pins.

### `anon`

Each path element that is not a common folder or file name (`src`, `lib`, `test`, `index.ts` and the rest of upstream's list) has its part before the first dot replaced by the next word of `reporterOptions.anon.wordlist`, the same part by the same word throughout. dependency-cruiser bundles no word list; with none, or when the words run out, a part becomes a string of its shape (letters for letters with their case, digits for digits, `-`, `_` and `.` kept). Upstream draws that string at random on every run; Rulebearing draws it from the part itself, so two runs over the same result print the same bytes. That is the one difference, and a word list as long as the result needs removes it.

### `x-dot-webpage`

The module-level `dot` output, with `reporterOptions.dot`, is drawn by the GraphViz `dot` on `PATH`, as dependency-cruiser draws it, and wrapped in upstream's page. This output type is the only one that starts GraphViz ([ADR-0053](adr/0053-x-dot-webpage-draws-with-graphviz-dot.md)). Without GraphViz, or when `dot` fails, the run exits 2 with dependency-cruiser's message. `rulebearing wrap-html` writes the same page around an SVG drawn elsewhere ([cli.md § wrap-html](cli.md#wrap-html)).

## `diff` renderings

`rulebearing diff` renders its own document, not a cruise result, and takes its own three output types ([cli.md § diff](cli.md#diff)):

| Output type | Writes |
| --- | --- |
| `json` | The diff document: `base`, `head`, `addedEdges`, `removedEdges`, `newViolations`, `resolvedViolations`, `ratchets`, pretty-printed |
| `markdown` | A heading, the two sides when known, a line of counts, then one table per section and one line for an empty section; the body of the pull-request comment |
| `agent` | One line per new violation (`new <id> <severity> <rule>: <from:line:column> -> <to>. Fix: <fix>`), then one line of counts |

## Options

| Option | Reporter | Effect |
| --- | --- | --- |
| `reporterOptions.err.showExternalModulesUnresolved`, `showAliasedModulesUnresolved` | `err`, `err-long` | Print the specifier rather than the resolved path for an unresolved import |
| `reporterOptions.text.highlightFocused` | `text` | Mark the modules `--focus` selected |
| `--prefix`, `--suffix` | all that link | Around each module path in a link |
| `--strict-schema` | `json` | Strip every Rulebearing addition |
| `--max-findings N` | `agent` | Show at most N violations per rule |
| `--color auto\|always\|never` | `err`, `err-long`, `text` | Colour; `auto` honours `NO_COLOR` |

## The JSON document

`json` writes dependency-cruiser's `cruise-result` document with its field names unchanged, plus additions that are only ever added ([ADR-0004](adr/0004-graph-document-is-cruise-result-superset.md)):

| Where | Additions |
| --- | --- |
| module | `language` |
| dependency | `line`, `column`, `dependencyKind` |
| violation | `id` (stable, [ADR-0015](adr/0015-stable-violation-id.md)), `fix`, `decision` |
| `summary` | `inspected` (files and modules read per language), `vacuousRules` (each with `"severity": "warn"` when the run only warned, [ADR-0032](adr/0032-liveness-follows-the-configuration-format.md)), `ratchets` ([ADR-0029](adr/0029-ratchets-enforced-by-cruise-and-reported-in-the-summary.md)), `expired` (rules and known violations past their date, [ADR-0031](adr/0031-a-saved-result-carries-what-the-exit-code-counts.md)) |

`--strict-schema` removes all of them, and the output then validates against dependency-cruiser 18.2.0's schema ([layer 4](../conformance/README.md#gate-1-layer-by-layer)). The graph document's own schema is [`schema/v1.json`](../schema/v1.json), generated from the types. Two runs over the same inputs serialise byte for byte. `optionsUsed.baseDir` is the working folder, as upstream writes it, and `teamcity` stamps each message with the time, which `SOURCE_DATE_EPOCH` pins.

## `github-annotations`

```text
::error file=src/domain/model.ts,line=1,col=1,title=domain-not-to-web::src/domain/model.ts -> src/web/view.ts: The domain stays independent of the web layer Fix: Move the shared type into src/domain
```

`warning` for `warn` and `notice` for `info`; `ignore` is not printed. `line` and `col` come from the edge; a module finding has none.

## `sarif`

One SARIF rule per configuration rule: the name as `shortDescription`, the comment as `help.text`, and the comment followed by the `fix` in `help.markdown`, which code scanning shows as the recommendation. One result per violation at the rule's level (`warning` for `warn`, `note` for `info`), located at `from` with the edge's line and column, and fingerprinted with `partialFingerprints["rulebearing/v1"]`, the stable id, so an alert survives line churn. A known violation is reported with an external suppression; a vacuous rule and an expired entry are configuration notifications. Paths are from the repository root, as for `github-annotations`. The output validates against the OASIS SARIF 2.1.0 schema ([crates/rb-report/tests/sarif_schema.rs](../crates/rb-report/tests/sarif_schema.rs)).

## `junit` and `trx`

One test case per rule of every family, and one per ratchet. An error-severity violation fails the case: the message is the `fix` and the first five violations with their id, `from`, `to` and line, and the body lists every violation, one per object for an element rule. A vacuous rule, an expired rule or known violation and a ratchet without a budget are errors, not failures, because the rule could not be checked. Warn, info and known findings are listed in the case's output without failing it. The receipt (`summary.inspected`) is the test suite's properties in `junit` and the run's output in `trx`. `junit` validates against the Jenkins xUnit plugin's `junit-10.xsd`, and `trx` has the structure Visual Studio's `vstst.xsd` requires ([crates/rb-report/tests/xml_schemas.rs](../crates/rb-report/tests/xml_schemas.rs)).

## `agent`

Each violation carries a `cost`: `edgesToMove` (one for a dependency, the steps of a cycle or a reachability path, none for a module finding) and `targetFanIn` (how many modules depend on the target), summed into `score`. Violations are ordered by `score`, then `from` and `to`; rules by their cheapest violation. Each rule group carries its `fix` and decision token, `count` keeps the total, and `budget.truncated` says whether `--max-findings` cut anything. With the hooks from [agents.md](agents.md), this is what an agent reads when a turn ends.
