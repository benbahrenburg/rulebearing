# Rules

The rule language is dependency-cruiser 18.2.0's, whole: every rule shape and attribute it accepts is accepted here with the same meaning, and its own test suites prove it ([conformance gate 1](../conformance/README.md)). The exhaustive list, with each row's status, is the [coverage tab § Rules](artifacts/dependency-cruiser-18.2.0-coverage.md#rules); this page explains the shapes and what Rulebearing adds. Source: [design § Dependency rules](artifacts/design.md#dependency-rules-the-whole-of-dependency-cruiser-1820). Where rules live and how files are found is in [config.md](config.md).

## The four families

| Family | A violation is | Example |
| --- | --- | --- |
| `forbidden` | an edge, or a module, that matches the rule | `from: { path: "^src/domain/" }, to: { path: "^src/web/" }` |
| `allowed` | an edge that no `allowed` entry covers; one rule, `not-in-allowed`, with `allowedSeverity` | `from: { path: "^src/" }, to: { path: "^(src\|node_modules)/" }` |
| `required` | a module matching `module` that does not import (or, with `to.reachable: true`, does not reach) a module matching `to` | every page must reach `src/auth.ts` |
| `rules.ratchets` (native) | a count of matching edges above the ceiling in the budget file | route files importing server code, counted per app |

`forbidden` has three variants beside the plain `from` and `to` edge rule:

- **Cycles**: `to.circular: true`, optionally narrowed with `via`, `viaOnly`, `viaNot` or `viaSomeNot`, and `dependencyTypesNot: [type-only]` to ignore type imports.
- **Reachability**: `to.reachable: true` forbids reaching a module through any chain; `to.reachable: false` requires every matching module to be reachable from `from`.
- **Dependents**: `module` with `numberOfDependentsLessThan` or `numberOfDependentsMoreThan`, for "a shared module used by fewer than two others" and the like.

`from.orphan: true` matches modules that import nothing and are imported by nothing. `scope: folder` applies `circular` to folders rather than modules.

## Element, slice and diagram rules

Native configurations have three more families under `rules`, which carry ArchUnitNET's vocabulary to every language ([plan 0002, Steps 5 and 6](plans/pending/0002-wave-2-dotnet-python-element-rules.md#25-step-5-the-element-rule-engine-and-the-capability-table-2c)). Gate 2 holds them to ArchUnitNET's and NetArchTest's own tests ([conformance/README.md](../conformance/README.md)).

```yaml
rules:
  elements:
    - name: services-are-sealed
      comment: "adr:0004"
      select: { kind: class, where: { haveNameEndingWith: Service } }   # That()
      should: { beSealed: true }                                         # Should()
      because: services are composed, never extended
  slices:
    - name: bounded-contexts-do-not-know-each-other
      comment: "adr:0005"
      matching: "RiverBooks.(*)"
      should: [notDependOnEachOther, beFreeOfCycles]
  diagrams:
    - name: matches-the-context-diagram
      comment: "adr:0001"
      select: { kind: type, where: { resideInNamespaceMatching: "^Shop\\." } }
      adhereTo: docs/architecture/components.puml
```

- **Element rules.** `select.kind` is `type`, `class`, `interface`, `attribute`, `member`, `field`, `method`, `property`, `function` or `module`; `where` holds predicates (`arePublic`, `doNotHaveName`) and `should` conditions (`bePublic`, `notHaveName`); `all`, `any` and `not` combine them, and ArchUnitNET's `And` / `Or` chains are their left-to-right fold. A `...That` key (`dependOnAnyTypesThat`) takes a nested selector. `exist` and `notExist` may sit anywhere in a condition. An empty selection is vacuous unless `allowEmpty` is set or the conditions mention `exist`. `select.includeReferenced` also selects the types the code references but does not define ([ADR-0035](adr/0035-referenced-types-in-the-code-layer.md)).
- **Every language.** Each key's answer in .NET, TypeScript, JavaScript and Python is in the [generated reference](reference/element-rules.md). A key a language cannot answer (`beSealed` in Python) is exit 3 naming the rule, unless `select.language` leaves that language out ([ADR-0014](adr/0014-no-invented-cross-language-edges.md)).
- **Every kind.** The reference's Kinds column says which `select.kind`s each key means something for: `beSealed` for types, `beVirtual` for methods and properties, `adhereToPlantUmlDiagram` (and so a diagram rule) for types and functions. A key used on another kind is exit 3 naming the rule, the key and the kind, rather than a fixed answer; a nested selector's `where` is checked against its own kind and languages. `kind: module` selects the module layer's modules: a name (the file name), a full name (the source), dependencies (the resolved imports; `onlyDependOn` judges those on modules of the run that are neither core nor unresolved) and a namespace (a TypeScript or JavaScript module's path, a Python module's dotted name, any namespace a .NET file declares), and nothing else.
- **Slices** group .NET types by namespace and TypeScript and Python modules by path or dotted name. `(*)` and `(**)` name a slice as ArchUnitNET does, by everything after the prefix; `Ns.(**)..` names it by the first segment, and `segments: 1` keeps one segment whatever follows, which is import-linter's `acyclic_siblings` ([ADR-0034](adr/0034-slices-group-types-or-modules-and-segments.md)).
- **Diagrams** read the PlantUML component subset ArchUnitNET reads; a malformed diagram is exit 3 with ArchUnitNET's exception name.

## Keys across languages

A native dependency rule may also narrow `from` and `to` by `language`, `namespace` / `namespaceNot`, `project` / `projectNot` and `assembly` / `assemblyNot`, and `to` by `dependencyKind` / `dependencyKindNot` (`inherits`, `implements`, `attribute`, ...). They read what the extractors record on each module and edge, and a `.dependency-cruiser.*` configuration that uses one is exit 3 ([plan 0002, Step 8](plans/pending/0002-wave-2-dotnet-python-element-rules.md#28-step-8-cross-language-rule-additions-per-language-dependencytypes-license-moreunstable-2d)). `to.license` reads npm, NuGet and Python licences alike.

Not every extractor records every property:

| Property (keys) | .NET | TypeScript, JavaScript | Python |
| --- | --- | --- | --- |
| `namespaces` (`namespace`, `namespaceNot`) | the namespaces the file declares | not recorded | the dotted module name |
| `project` (`project`, `projectNot`) | the `.csproj` (or loaded assembly) path | not recorded | the top-level package |
| assembly (`assembly`, `assemblyNot`) | the assembly of the file's types | not recorded | not recorded |
| `dependencyKind` (`dependencyKind`, `dependencyKindNot`) | `inherits`, `implements`, `attribute`, ... | `import` | `import` |

A rule whose `from` or `to` narrows by a property that a language the side can select does not record is exit 3 naming the rule, the side, the key and the language, because the key would be false for every module of that language ([ADR-0014](adr/0014-no-invented-cross-language-edges.md)). Add that side's `language` to leave the language out: `from: { language: dotnet, namespace: "^Shop\\." }`. `to` can select the targets of the edges from what `from` can select, since no edge crosses languages. A core or external module, which no extractor analysed, has none of these properties and matches neither a key nor its `Not` form. The table is data in `rb-config` (`capability::records`).

## Captures

A capturing group in `from.path` is available in `to` as `$1` to `$9` (and `$0` for the whole match), escaped so it matches only itself. The common fence is one rule:

```yaml
- name: apps-are-independent
  from: { path: "^apps/([^/]+)/" }
  to: { path: "^apps/([^/]+)/", pathNot: "^apps/$1/" }
```

`rulebearing explain apps-are-independent --plain` says it back as "Files under `apps/<x>/` may not import files under `apps/<y>/` unless x = y."

## Severity and the exit code

`error`, `warn`, `info` and `ignore`. A gating reporter (`err`, `err-long`, `null`, `teamcity`, `azure-devops`, `github-annotations`, `agent`) exits with the number of `error` violations, capped at 255; `json`, `csv` and `text` exit 0, as dependency-cruiser's do, so a pipeline can save JSON in one step and gate in the next ([ADR-0008](adr/0008-exit-code-contract.md), [ADR-0030](adr/0030-the-reporter-decides-the-error-count-exit.md)). Exit 2 means the run cannot be trusted and exit 3 means the configuration is invalid, whatever the reporter.

## Liveness

A rule whose selecting side matches no module checks nothing, and a gate that reads green because a rule went stale is worse than no rule. So every rule whose `from` (or `module`) matches nothing is listed in `summary.vacuousRules` and named on stderr ([ADR-0007](adr/0007-vacuous-rules-fail-by-default.md)). What that does to the run depends on the configuration ([ADR-0032](adr/0032-liveness-follows-the-configuration-format.md)):

| Configuration | Default | A vacuous rule |
| --- | --- | --- |
| `rulebearing.*`, including one that `extends` a dependency-cruiser file | `strict` | fails the run with exit 2 |
| `.dependency-cruiser.*`, run as it is | `warn` | a warning; the exit code is dependency-cruiser's, and the entry carries `"severity": "warn"` |

`--liveness strict|warn|off` overrides the default, and `--no-liveness` is `off`, which dependency-cruiser's own test suites need. A rule that may match nothing is excused by name: `allowEmpty: true` on the rule, or, for a rule that lives in a dependency-cruiser file, its name in the native file's top-level list:

```yaml
extends: ./.dependency-cruiser.cjs
allowEmpty: [plugins-stay-apart]   # matches nothing until the first plugin lands
```

A name in that list that is no rule is a configuration error. `rulebearing adopt` writes the list for the rules that match nothing on the day it runs.

## What Rulebearing adds to a rule

| Addition | What it does |
| --- | --- |
| `fix` | The imperative printed under every finding by `err-long`, `explain`, `can-import` and the `agent` reporter |
| `examples.forbidden`, `examples.allowed` | Edges `rulebearing test` builds into a graph and checks the rule against, so a rule's regex is tested like code |
| `owner`, `expires` | Who answers for a temporary rule, and the day after which it fails the run |
| A decision token in `comment` | `adr:NNNN` or `plan:<slug>`; required by `--require-comment-token` |
| A stable `id` on every violation | `RB-` and eight hex digits from the rule, the two ends and the kind of edge ([ADR-0015](adr/0015-stable-violation-id.md)), so a baseline entry survives unrelated edits |
| `line` and `column` on every TypeScript edge | Where the import is, so a finding points at a line |

## Baselines and ratchets

**Known violations.** `options.knownViolations` lists findings that are accepted for now. An entry written by `rulebearing adopt` or `init` carries the violation's `id`, its `type` and, for a cycle or a reachability chain, the modules in it, plus `expires` and `owner`; an entry written by dependency-cruiser (`from`, `to`, `rule`, `cycle`, `via`) matches the way dependency-cruiser matches it. A known finding is reported with severity `ignore`. An entry past its `expires` date fails the run the day after. An element or slice entry without an `id` matches on its rule and whichever of `from` and `to` it names, so an entry naming only a type (`to`) or only a slice (`from`) covers every finding of it, as ArchUnitNET's frozen rules do. `rulebearing baseline` writes and maintains the file ([cli.md § Baselines](cli.md#baselines)).

**Ratchets.** A `rules.ratchets` entry counts the direct edges that match its `from` and `to` (with `$1` captures) and compares the count with `{ "ceiling": n }` in its budget file. `cruise` reports every ratchet in `summary.ratchets`: an exceeded one is one error, and a missing budget exits 2 ([ADR-0029](adr/0029-ratchets-enforced-by-cruise-and-reported-in-the-summary.md)). `rulebearing count --from ... --to ... --budget FILE --write` lowers the ceiling to the current count and refuses to raise it.

## Checking the rules themselves

| Command | Answers |
| --- | --- |
| `rulebearing test` | Does each rule flag its `forbidden` examples and pass its `allowed` ones? |
| `rulebearing config lint` | Can a rule never match? Is it shadowed by an earlier one? Does an `allowed` list admit everything? Does every rule have a `fix` that says more than its name? |
| `rulebearing rules --json` | Every rule with its family, severity, and how many modules each side matched |
| `rulebearing explain <rule>` | The rule in a sentence, its reason and fix, and the first edges it matched |
