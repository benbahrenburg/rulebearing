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

`dot`, `ddot`, `archi`, `flat`, `mermaid`, `d2`, `baseline`, `metrics`, `sarif`, `junit` and `trx` arrive in wave 2; `html`, `markdown`, `anon` and `plantuml` in wave 3. Asking for one now exits 2 and names the wave.

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
| `summary` | `inspected` (files and modules read per language), `vacuousRules`, `ratchets` ([ADR-0029](adr/0029-ratchets-enforced-by-cruise-and-reported-in-the-summary.md)), `expired` (rules and known violations past their date, [ADR-0031](adr/0031-a-saved-result-carries-what-the-exit-code-counts.md)) |

`--strict-schema` removes all of them, and the output then validates against dependency-cruiser 18.2.0's schema ([layer 4](../conformance/README.md#gate-1-layer-by-layer)). The graph document's own schema is [`schema/v1.json`](../schema/v1.json), generated from the types. Two runs over the same inputs serialise byte for byte. `optionsUsed.baseDir` is the working folder, as upstream writes it, and `teamcity` stamps each message with the time, which `SOURCE_DATE_EPOCH` pins.

## `github-annotations`

```text
::error file=src/domain/model.ts,line=1,col=1,title=domain-not-to-web::src/domain/model.ts -> src/web/view.ts: The domain stays independent of the web layer Fix: Move the shared type into src/domain
```

`warning` for `warn` and `notice` for `info`; `ignore` is not printed. `line` and `col` come from the edge; a module finding has none.

## `agent`

Each violation carries a `cost`: `edgesToMove` (one for a dependency, the steps of a cycle or a reachability path, none for a module finding) and `targetFanIn` (how many modules depend on the target), summed into `score`. Violations are ordered by `score`, then `from` and `to`; rules by their cheapest violation. Each rule group carries its `fix` and decision token, `count` keeps the total, and `budget.truncated` says whether `--max-findings` cut anything. With the hooks from [agents.md](agents.md), this is what an agent reads when a turn ends.
