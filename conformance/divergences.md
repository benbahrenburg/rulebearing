# Documented divergences

Conformance gate 1, layer 5 compares dependency-cruiser's cruise result with Rulebearing's for the same repository, tree, configuration and roots ([scripts/run-layer-5.sh](dependency-cruiser/scripts/run-layer-5.sh), [harness/zero-diff.mjs](dependency-cruiser/harness/zero-diff.mjs)). The design allows a difference only as "a documented divergence" with a reason ([design § Test beds](../docs/artifacts/design.md#test-beds-open-source-repositories-to-validate-against), item 1; [plan 0001, Step 18](../docs/plans/implemented/0001-wave-1-typescript-parity.md#step-18-gate-1-layer-5-the-mutation-branch-oracle-zero-diff-1g)). This file is that record, and the gate reads it: a difference whose key matches a row's `Match` expression, for that row's repository (or `*`), is reported as documented; any other difference fails the gate.

The keys `zero-diff.mjs` prints, one per difference:

| Key | Means |
| --- | --- |
| `module-missing <source>`, `module-extra <source>` | a module only dependency-cruiser, or only Rulebearing, reports |
| `module-field <source> <field>` | a module-level field differs |
| `dependency-missing <source> -> <resolved>`, `dependency-extra ...` | an edge only one tool reports |
| `dependency-field <source> -> <resolved> <field>` | a field of an edge differs |
| `violation-missing <rule> <from> -> <to>`, `violation-extra ...` | a `summary.violations` entry only one tool reports |

Rulebearing's additions are never compared, because dependency-cruiser has nothing to compare them with ([ADR-0004](../docs/adr/0004-graph-document-is-cruise-result-superset.md)): `line`, `column`, `dependencyKind` and `language` on a module or dependency, `id`, `fix` and `decision` on a violation, and `summary.inspected`, `summary.vacuousRules` and `summary.ratchets`.

A row is either **permanent** (an upstream behaviour Rulebearing deliberately does not reproduce, with the upstream issue or the test that contradicts it) or **open** (a known Rulebearing gap with its owner and the change that closes it; the row is deleted in the change that closes it). The table may only shrink as open rows close.

## Divergences

None. The table's shape, for when one is needed:

| Repository | Match | What differs | Reason | Link |
| --- | --- | --- | --- | --- |

## Gate 1: configurations no oracle runs

Differences found outside the oracles' own configurations, so layer 5 never sees them. This table is not read by layer 5 (its second column is not a match expression). An oracle that adopts one of these configurations needs a gate row above that names the affected files.

| Kind | Configuration | dependency-cruiser | Rulebearing | Reason |
| --- | --- | --- | --- | --- |
| permanent | tsPreCompilationDeps not `true`, parser not `tsc` or `swc`, and a tsconfig that sets `jsx` | parses every `.ts` file as TSX (`transpileModule` is given no file name, so TypeScript names the input `module.tsx`). A generic arrow or an angle-bracket assertion becomes a JSX parse error, and imports used only in what tsc's recovery swallows are dropped: 14 edges in 6 files on langfuse; on the fixture `crates/rb-extract-ts/tests/options/ts-jsx`, `src/a.ts` | parses a file by its extension and reports every import (`a_ts_file_is_typescript_whatever_the_tsconfig_jsx`) | [ADR-0064](../docs/adr/0064-a-ts-file-is-parsed-as-typescript-when-jsx-is-set.md): matching it would mean reproducing tsc's error recovery to drop edges that exist |

## Gate 2: .NET extractor divergences

Conformance gate 2 compares the element engine with ArchUnitNET over the committed graphs of its test assemblies ([conformance/README.md](README.md), [ADR-0009](../docs/adr/0009-conformance-suites-as-specification.md)). Every ported case reproduces upstream; the rows below are what the .NET extractor (`crates/rb-extract-dotnet`) reads differently from ArchUnitNET's loader where no ported case asserts it. They surface in the PlantUML generator's case `PlantUmlFileBuilderTest.BuildUmlByTypesIncludingDependenciesToOtherTest#1` (`archunitnet/ported/PlantUmlFileBuilderTest.yaml`, `graphDiffers`), which proves these lines and only these are the difference. This table is not read by layer 5 (its second column is not a match expression).

| Kind | What differs | ArchUnitNET | Rulebearing | Reason |
| --- | --- | --- | --- | --- |
| permanent | A method's generic type parameter as a dependency target | one target named by the method's full signature, `System.Void Ns.Type::Method(ArchUnitNET.Fluent.IArchRule)<T>+<T>` | the parameter as `Method+<T>` (`Declarer+<T>`), and, where the signature blob names it only by position, a second target `!!0`; neither is a referenced type (`is_generic_parameter` in `rb-extract-dotnet/src/lib.rs`), so no rule can name one | ArchUnitNET's name spells the method's return and parameter types in Mono.Cecil's syntax, a string no rule compares with; reproducing it would change the target names every element rule over the committed graphs reads for a name nothing selects |
| open | Interfaces an enum or a delegate inherits from its runtime base type (`System.Enum`: `IComparable`, `IConvertible`, `IFormattable`, `ISpanFormattable`; `System.MulticastDelegate`: `ICloneable`, `ISerializable`) | listed among the type's dependencies and interfaces, read from the runtime assembly that defines the base type | absent: the extractor reads only the analysed assemblies and records the base type itself, so `implementInterface: System.IComparable` over an enum does not hold where ArchUnitNET's `ImplementInterface` does | the interfaces come from `System.Private.CoreLib` of the target framework (which ones differs by version: `ISpanFormattable` is .NET 6 and later), and the extractor does not read runtime metadata ([ADR-0011](../docs/adr/0011-read-dotnet-assemblies-not-source.md)); a built-in list would be an invented edge ([ADR-0014](../docs/adr/0014-no-invented-cross-language-edges.md)). Closes when the extractor resolves base types in the referenced framework assemblies (`includeDependencies`); owner: the .NET extractor, no plan names the change yet |
