# Rulebearing.Analyzer

A Roslyn analyzer that reads `rulebearing.yaml` and reports the rules it can judge from one compilation while `dotnet build` runs, at the line, with the rule's `fix` as the message. It does not replace the gate: `rulebearing cruise` over the compiled assemblies is still the check that decides, and every finding the analyzer reports is one the gate reports too ([design § Two front-ends that will matter more than the MCP server](../../docs/artifacts/design.md#two-front-ends-that-will-matter-more-than-the-mcp-server); [ADR-0021](../../docs/adr/0021-agent-surface-cli-first.md); [FR-DIST-04](../../docs/prd.md#fr-dist-04); [Wave 3, Step 21](../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#26-steps-for-sub-wave-3f-the-roslyn-analyzer-and-rb-node)).

## Use

```xml
<ItemGroup>
  <PackageReference Include="Rulebearing.Analyzer" Version="x.y.z" PrivateAssets="all" />
  <AdditionalFiles Include="$(MSBuildThisFileDirectory)../rulebearing.yaml" />
</ItemGroup>
```

The rule file is the `AdditionalFiles` item named `rulebearing.yaml` or `rulebearing.yml`. Paths in dependency rules are read relative to the folder that holds it, as the gate reads them. When the file lists `languages.dotnet.assemblies`, a project whose assembly no glob names is not judged, so test projects that sit beside the code stay quiet; and a referenced assembly that a glob names is one the gate loads too, so a type's base chain and interfaces run on through it as they do in the gate. Without the list, they stop at the project's own assembly.

## Diagnostics

| Id | Severity | Reported for |
| --- | --- | --- |
| `RB0001` | the rule's | a `forbidden` dependency rule broken by a reference from one source file to a type declared in another, at the first line that names the type |
| `RB0002` | the rule's | an element rule broken by a type, at the type's declaration in a file someone wrote ([ADR-0061](../../docs/adr/0061-a-type-is-attributed-to-a-file-its-developer-wrote.md)) |
| `RB0003` | Info | a rule, or an extended configuration, the analyzer leaves to `rulebearing cruise`, with the reason |
| `RB0009` | Error | a rule file that cannot be read, with the reason |

The message is the rule's `fix`, else its `comment`, else its name; the help link is the rule language, `docs/rules.md`. Each diagnostic carries the properties `rule`, `to` and `violationId`, the last the gate's id for the same finding (`RB-` and the first four bytes of SHA-256 over rule, from, to and the condition key).

## What is judged at compile time

| Family | Evaluated by the analyzer | Reported by `cruise` |
| --- | --- | --- |
| `forbidden` (top level or `rules.dependencies`) | `from` and `to` with `path` and `pathNot` only, `$1` substitution included | any other condition (`circular`, `orphan`, `reachable`, `dependencyTypes`, `via`, `couldNotResolve`, ...) or key |
| `allowed`, `required` | | all |
| `rules.elements` | `kind` `type`, `class`, `interface` and `attribute`; `all`, `any`, `not`; the identity, visibility (`public`, `internal`), `nested`, `nestedIn`, `abstract`, `sealed`, `static`, `record` and `immutable` flags; the `haveName*` and `haveFullName*` families; `resideInNamespace`, `resideInAssembly` and their `Matching` forms; `implementInterface`, `beAssignableTo`, `dependOnAny` with names or a nested selector | `kind` selecting members or modules, and every other predicate or condition (attributes, members, getters and setters, cycles between slices, ...) |
| `rules.slices`, `rules.diagrams` | | all |
| `extends` | | the extended configuration's rules |

A rule is evaluated whole or not at all: one condition outside the evaluated set leaves the rule to `cruise` and `RB0003` names it. Patterns follow the gate's JavaScript-compatible semantics and refuse what it refuses ([ADR-0016](../../docs/adr/0016-linear-time-regex-and-strict-compat.md)); the test suite runs the gate's compatibility table, exported from `rb-config`.

A type's facts are the compiled extractor's: metadata names (`Ns.Outer+Inner``1`), a static class is abstract, sealed and static, a record is a `class` with `record`, an enum is never immutable, and a type depends on what its signatures, attributes and bodies (lambdas included) name. Generated code is analysed, as the extractor reads every type the assembly holds.

## Parity with the gate

| Check | Where | What it compares |
| --- | --- | --- |
| Pull request | [parity/run.sh](parity/run.sh), in CI's `dotnet-adapters` job | the extractor's sample fixture under [parity/rulebearing.yaml](parity/rulebearing.yaml), as written and with every rule negated |
| Nightly | [testbeds/oracles/analyzer_parity.py](../../testbeds/oracles/analyzer_parity.py) through `testbeds/oracles/dotnet.sh` | each .NET oracle's imported rules, as written and negated, against `cruise --graph` over its compiled graph |

A finding is the pair (rule, type full name); any difference fails the check.

## Develop

```sh
dotnet test frontends/Rulebearing.Analyzer/tests/Rulebearing.Analyzer.Tests -p:Threshold=70 -p:ThresholdType=line -p:ThresholdStat=total
frontends/Rulebearing.Analyzer/parity/run.sh
RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-config the_cases_are_exported_for_the_analyzer   # regenerate regex-compatibility.json
```
