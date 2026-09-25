# Conformance gate 2: NetArchTest

NetArchTest's own test project, treated as the design says: "NetArchTest's own test project is treated the same way" as ArchUnitNET's ([design § Conformance gate 2](../../docs/artifacts/design.md#conformance-gate-2-archunitnets-test-assemblies-validate-the-element-rules)). Each upstream assertion over the test structure becomes a data-driven case: the element rule that expresses the same selection and condition, and the exact passing and failing types ([plan 0002, Step 7](../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#27-step-7-gate-2-porting-to-completion-2c); [ADR-0009](../../docs/adr/0009-conformance-suites-as-specification.md)). NetArchTest is MIT, pinned at [1.3.2](PIN) (tag `v1.3.2`).

| Path | What |
| --- | --- |
| [PIN](PIN) | the upstream version |
| [scripts/build-test-assemblies.sh](scripts/build-test-assemblies.sh) | builds the fixtures at the tag, deterministically |
| [fixtures/](fixtures/README.md) | `NetArchTest.TestStructure` and the two `CrossAssemblyTest` assemblies with portable PDBs, NetArchTest's LICENSE, `SHA256SUMS` |
| `graphs/<Assembly>.json` | each fixture extracted, written by `crates/rb-extract-dotnet/tests/netarchtest_graphs.rs` (`RB_UPDATE_SNAPSHOTS=1`), so `rb-rules` never reads an assembly ([ADR-0010](../../docs/adr/0010-crate-layout-and-extractor-boundary.md)) |
| `ported/<TestClass>.yaml` | the cases, one file per upstream test class |
| `ported.json`, `unported.json` | the ratchet counts and every upstream test, or search, with no case and its reason |
| `tools/Port/` | the tool that writes the three |

## How a case is made

The verdicts are NetArchTest's own. `tools/Port` is a C# program that references NetArchTest.Rules 1.3.2 and the committed fixtures, takes the upstream test project's assembly name so the `InternalsVisibleTo` NetArchTest grants its tests reaches the same internal types and dependency search, and runs each upstream chain as upstream wrote it. For a predicate test it runs the chain with and without the predicate under test: the types the chain returns pass, the rest of the selection fails. For a condition test it runs the selection and `GetResult()`: the failing types fail, the rest pass. For a dependency search it calls the same `DependencySearch` method the upstream helper calls. Every upstream assertion (the counts, the named types, `IsSuccessful`) is checked against the result before the case is written, so a fixture that disagreed with upstream would stop the tool. Beside each verdict the tool writes the element rule that maps the chain, from the table below.

`crates/rb-rules/tests/gate2_netarchtest.rs` evaluates every case with the element engine over the graphs and fails on any difference; it shares its loader and checker with the ArchUnitNET half (`crates/rb-rules/tests/gate2_common/`).

## The mapping

NetArchTest's predicates and conditions share one function per concept (`FunctionDelegates.cs`), so one mapping serves `where` and `should`. NetArchTest compares names ignoring case where ArchUnitNET does not; the JavaScript pattern dialect has no flag for it, so a case-insensitive comparison is written as a pattern whose letters match either case (`SomeT` is `^[Ss][Oo][Mm][Ee][Tt]`).

| NetArchTest | Semantics | Element rule |
| --- | --- | --- |
| `ResideInNamespace(n)` | the full name starts with `n`, ignoring case | `haveFullNameStartingWith: n` |
| `HaveName(n)` | the name equals `n`, ignoring case | `haveName: n` |
| `HaveNameStartingWith(s)`, `HaveNameEndingWith(s)` | ignoring case; ordinal with `StringComparison.Ordinal` | `haveNameMatching` with a case-insensitive anchored pattern; `haveNameStartingWith` / `haveNameEndingWith` when ordinal |
| `HaveNameMatching(p)` | `p` with `RegexOptions.IgnoreCase` | `haveNameMatching` with `p` made case-insensitive |
| `ResideInNamespaceMatching(p)` (and `StartingWith`, `EndingWith`, `Containing`, which build `^n`, `n$`, `^.*n.*$`) | `p`, ignoring case, against the namespace, or for a nested public or private type the declaring type's full name | `any` of: not nested and `resideInNamespaceMatching`; nested public or private and `areNestedIn` a type whose full name matches |
| `HaveCustomAttribute(t)` | an attribute of type `t` | `haveAnyAttributes: [t]` |
| `HaveCustomAttributeOrInherit(t)` | an attribute of type `t` or a subclass | `haveAnyAttributes: { kind: type, where: { assignableTo: [t] } }` |
| `Inherit(t)` | `t` is in the base-class chain, the type itself excluded | `all: [assignableTo: [t], areNot: [t]]` |
| `ImplementInterface(t)` | `t` is among the interfaces the type lists | `implementInterface: [t]` |
| `AreClasses`, `AreInterfaces` | Cecil's `IsClass` (every type that is not an interface), `IsInterface` | `areNot` / `are: { kind: interface }` |
| `AreAbstract`, `AreGeneric`, `AreStatic`, `AreNested`, `ArePublic`, `AreSealed` | Cecil's flags | `areAbstract`, `areGeneric`, `areStatic`, `areNested`, `arePublic`, `areSealed` |
| `AreNestedPublic`, `AreNestedPrivate` | `IsNestedPublic`, `IsNestedPrivate` | `all: [areNested, arePublic]`, `all: [areNested, arePrivate]` |
| `HaveDependencyOn(Any)(names)` | a referenced type whose name the entry begins in whole segments (`.`, `+`, `/`, `:`), case-sensitively: a type, its nested types, or a namespace | `dependOnAny: { kind: type, where: { haveFullNameMatching: "^(?:names)(?:$\|[.+])" } }` |
| `HaveDependencyOnAll(names)` | one such dependency per distinct entry | `all` of one `dependOnAny` per entry |
| `OnlyHaveDependenciesOn(names)`, `HaveDependenciesOtherThan(names)` | every referenced type matches an entry; its negation | `onlyDependOn` / `notOnlyDependOn` with the same selector; an external entry such as `System` selects nothing, and `onlyDependOn` ignores dependencies outside the analysed code, so the verdict holds where every such dependency is on a System type, which each case's note says |
| `Or()`, `And()`, `ShouldNot()` | left-associative groups; inversion | `any`, `all`, `not` |

An empty upstream selection passes silently in NetArchTest; the case expects a vacuous selection, Rulebearing's verdict ([ADR-0007](../../docs/adr/0007-vacuous-rules-fail-by-default.md)), and its note says so.

## Counts

326 cases and 80 unported entries, 406 in all (`ported.json`). The unit is one upstream test, or one search where a test makes several (`Utils.RunDependencyTest` searches by the dependency's full name and then by its namespace, and each search is one entry).

| Upstream test class | Cases | Unported |
| --- | --- | --- |
| `PredicateTests` | 58 | 7 |
| `ConditionTests` | 59 | 6 |
| `ConditionListTests` | 10 | 0 |
| `PredicateListTests` | 3 | 0 |
| `FunctionSequenceTests` | 2 | 0 |
| `TypesTests` | 2 | 10 |
| `PolicyDefinitionTests` | 5 | 4 |
| `DependencySearch/VariousTests` | 9 | 2 |
| `DependencySearch/SearchTypeTests` | 12 | 2 |
| `DependencySearch/DependencyTypeTests` | 80 | 41 |
| `DependencySearch/DependencyLocationTests` | 86 | 6 |
| `DependencySearch/ScalabilityTests` | 0 | 2 |

Every unported entry carries its reason in [unported.json](unported.json):

| Reason | Entries | Why no element rule gives NetArchTest's verdict |
| --- | --- | --- |
| `custom-predicate` | 2 | `MeetCustomRule`, a C# predicate |
| `constructed-type` | 38 | the search names an array, pointer, by-reference or closed generic type (`ExampleDependency[]`, `ExampleDependency<int>`), which NetArchTest tells apart from its element type; the graph records dependencies on type definitions and generic arguments |
| `dependency-definition` | 16 | NetArchTest's dependency walk differs from ArchUnitNET's, which the graph document follows: it skips a type's references to its own members, reads const string field values as dependency names, and searches event attributes, closure classes and uncalled static local functions |
| `vocabulary-gap` | 8 | NetArchTest's `BeImmutable` (no public setter, every field non-public, readonly or const) is not ArchUnitNET's; `OnlyHaveNullableMembers` / `HaveSomeNonNullableMembers` ask member nullability, which the graph does not record |
| `api-only` | 16 | the test asserts NetArchTest's own API: `Types.InCurrentDomain`, `FromFile`, `FromPath`, a policy's names and descriptions, the search's timing |

## Running it

```sh
conformance/netarchtest/scripts/build-test-assemblies.sh      # once; again only when PIN changes
RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-extract-dotnet --test netarchtest_graphs   # the graphs
dotnet run --project conformance/netarchtest/tools/Port -- conformance/netarchtest   # the cases (needs NuGet)
cargo test -p rb-rules --test gate2_netarchtest
scripts/gate2-netarchtest-check.sh                             # also run by scripts/gate2-check.sh
```
