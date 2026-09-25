# ArchUnitNET 0.13.4 coverage

Every public method of ArchUnitNET's fluent API, taken from its [predicate and condition definition sources](https://github.com/TNG/ArchUnitNET/tree/master/ArchUnitNET/Fluent/Syntax/Elements) and [ArchLoader](https://github.com/TNG/ArchUnitNET/blob/master/ArchUnitNET/Loader/ArchLoader.cs), with its declarative key in a Rulebearing element rule. A method with a `DoNot` / `AreNot` / `NotBe` / `NotHave` twin is listed once; the negation is `not:` around the key. **Parity** means the key exists with the same semantics on .NET; **Cross** means it also answers for TypeScript and Python where the concept exists; **Stays** means it remains in ArchUnitNET.

## Selectors (`select.kind`)

| ArchUnitNET | Rulebearing | Languages |
| --- | --- | --- |
| `Types()` | `kind: type` | .NET, TS (class, interface, enum, type alias), Python (class) |
| `Classes()` | `kind: class` | all |
| `Interfaces()` | `kind: interface` | .NET, TS |
| `Attributes()` | `kind: attribute` | .NET; TS and Python decorators |
| `Members()` | `kind: member` | all |
| `FieldMembers()` | `kind: field` | .NET, TS class fields, Python class attributes |
| `MethodMembers()` | `kind: method` | all |
| `PropertyMembers()` | `kind: property` | .NET, TS accessors, Python `@property` |
| (none) | `kind: function`, `kind: module` | TS, Python additions |

## Predicates and conditions shared by every element (`ObjectPredicatesDefinition`, `ObjectConditionsDefinition`)

| ArchUnitNET (predicate / condition) | Rulebearing key | Status |
| --- | --- | --- |
| `Are` / `Be`, `AreNot` / `NotBe` (a type, a list, or a selector) | `are`, `be` | Parity, Cross |
| `Exist` / `NotExist` | `exist` | Parity |
| `HaveName`, `HaveNameMatching`, `HaveNameStartingWith`, `HaveNameEndingWith`, `HaveNameContaining` (+ `DoNot`, `Not`) | `haveName`, `haveNameMatching`, `haveNameStartingWith`, `haveNameEndingWith`, `haveNameContaining` | Parity, Cross |
| `HaveFullName`, `HaveFullNameMatching`, `...StartingWith`, `...EndingWith`, `...Containing` | `haveFullName*` | Parity, Cross (dotted module path) |
| `HaveAssemblyQualifiedName` and its four variants | `haveAssemblyQualifiedName*` | Parity (.NET); TS and Python: package-qualified name |
| `ArePublic`, `ArePrivate`, `AreProtected`, `AreInternal`, `AreProtectedInternal`, `ArePrivateProtected` / `Be...` | `arePublic` ... `arePrivateProtected`; `be...` | Parity; Cross for `public` / `private` / `protected` (TS), `public` / `private` by underscore (Python) |
| `DependOnAny` / `DoNotDependOnAny` / `NotDependOnAny`, `DependOnAnyTypesThat` | `dependOnAny` (value: names, regex, or nested selector) | Parity, Cross |
| `OnlyDependOn` / `OnlyDependOnTypesThat` | `onlyDependOn` | Parity, Cross |
| `CallAny` / `DoNotCallAny` / `NotCallAny` | `callAny` | Parity (.NET IL); TS and Python: resolved call expressions |
| `HaveAnyAttributes`, `HaveAnyAttributesThat`, `OnlyHaveAttributes`, `OnlyHaveAttributesThat` | `haveAnyAttributes`, `onlyHaveAttributes` | Parity, Cross (decorators) |
| `HaveAttributeWithArguments`, `HaveAttributeWithNamedArguments`, `HaveAnyAttributesWithArguments`, `HaveAnyAttributesWithNamedArguments` | `haveAttributeWithArguments`, `haveAttributeWithNamedArguments`, `haveAnyAttributesWithArguments`, `haveAnyAttributesWithNamedArguments` | Parity; Cross for literal decorator arguments |
| `FollowCustomPredicate` / `FollowCustomCondition` | none | Stays |

## Type predicates and conditions (`TypePredicatesDefinition`, `TypeConditionsDefinition`)

| ArchUnitNET | Rulebearing key | Status |
| --- | --- | --- |
| `ResideInNamespace`, `ResideInNamespaceMatching` (+ `DoNot` / `Not`) | `resideInNamespace`, `resideInNamespaceMatching` | Parity, Cross (dotted module path; TS folder path) |
| `ResideInAssembly`, `ResideInAssemblyMatching` | `resideInAssembly`, `resideInAssemblyMatching` | Parity; Cross as package / distribution |
| `AreAssignableTo` / `BeAssignableTo`, `BeAssignableToTypesThat` | `areAssignableTo`, `beAssignableTo` | Parity; Cross for `extends` chains |
| `ImplementInterface`, `ImplementAny`, `ImplementAnyInterfacesThat` | `implementInterface`, `implementAny` | Parity; TS `implements` |
| `AreEnums`, `AreStructs`, `AreValueTypes` / `Be...` | `areEnums`, `areStructs`, `areValueTypes` | Parity; `areEnums` Cross (TS enum, Python `Enum`) |
| `AreNested`, `AreNestedIn` / `BeNested`, `BeNestedIn` | `areNested`, `areNestedIn` | Parity, Cross |
| `HaveMemberWithName`, `HaveFieldMemberWithName`, `HaveMethodMemberWithName`, `HavePropertyMemberWithName` | `haveMemberWithName`, `haveFieldMemberWithName`, `haveMethodMemberWithName`, `havePropertyMemberWithName` | Parity, Cross |
| `BeTypesThat` | nested selector under `be` | Parity |
| `AdhereToPlantUmlDiagram` | `adhereTo: <file>.puml` (diagram rule) | Parity, Cross |

## Class and attribute predicates and conditions

| ArchUnitNET | Rulebearing key | Status |
| --- | --- | --- |
| `AreAbstract` / `BeAbstract` (classes, attributes) | `areAbstract`, `beAbstract` | Parity; TS `abstract`, Python `ABC` |
| `AreSealed` / `BeSealed` (classes, attributes) | `areSealed`, `beSealed` | Parity; TS and Python: validation error |
| `AreRecord` / `BeRecord` | `areRecord`, `beRecord` | Parity; Python `@dataclass(frozen=True)` as `areImmutable` |
| `AreImmutable` / `BeImmutable` | `areImmutable`, `beImmutable` | Parity |

## Member predicates and conditions

| ArchUnitNET | Rulebearing key | Status |
| --- | --- | --- |
| `AreDeclaredIn`, `BeDeclaredIn`, `BeDeclaredInTypesThat` (+ `Not`) | `areDeclaredIn`, `declaredInTypesThat` | Parity, Cross |
| `AreStatic` / `BeStatic` | `areStatic`, `beStatic` | Parity, Cross (TS `static`, Python `@staticmethod` / `@classmethod`) |
| `AreReadOnly` / `BeReadOnly` | `areReadOnly`, `beReadOnly` | Parity; TS `readonly` |
| `AreImmutable` / `BeImmutable` | `areImmutable` | Parity |
| Method: `AreConstructors` / `BeConstructor`, `AreNoConstructors` | `areConstructors` | Parity, Cross |
| Method: `AreVirtual` / `BeVirtual` | `areVirtual` | Parity |
| Method: `HaveReturnType` / `DoNotHaveReturnType` | `haveReturnType` | Parity; TS annotated return types |
| Method: `HaveDependencyInMethodBodyTo` / `DoNotHave...` | `haveDependencyInMethodBodyTo` | Parity; TS and Python: references inside the function body |
| Method: `AreCalledBy` / `BeCalledBy` / `NotBeCalledBy` | `areCalledBy` | Parity; Cross where calls resolve |
| Method: `BeMethodMembersThat` | nested selector | Parity |
| Property: `HaveGetter`, `HaveNoGetter`, `HaveSetter`, `HaveNoSetter`, `HaveInitSetter` | `haveGetter`, `haveSetter`, `haveInitSetter` | Parity; TS accessors |
| Property: `HavePublicGetter`, `HavePrivateGetter`, `HaveProtectedGetter`, `HaveInternalGetter`, `HaveProtectedInternalGetter`, `HavePrivateProtectedGetter` and the six setter twins | `getterVisibility: public`, `setterVisibility: private`, and so on | Parity |
| Property: `AreVirtual` / `BeVirtual` | `areVirtual` | Parity |

## Combinators and rule operations

| ArchUnitNET | Rulebearing | Status |
| --- | --- | --- |
| `That()`, `Should()` | `select.where`, `should` | Parity |
| `And()`, `Or()` in predicates; `AndShould()`, `OrShould()` in conditions | `all: [...]`, `any: [...]` | Parity |
| `Because(reason)` | `because` (and `comment` for the decision token) | Parity |
| `Check(architecture)`, `Evaluate(architecture)` returning results per object | `rulebearing cruise` / `fmt`; the `junit`, `trx` and `agent` reporters carry the per-object results | Parity |
| `WithoutRequiringPositiveResults()` | `allowEmpty: true` (default false, as in ArchUnitNET) | Parity |
| Combined rules (`rule1.And(rule2)`, `Or`) | two rules, or `all` / `any` inside one | Parity |

## Slices (`SliceRuleDefinition`)

| ArchUnitNET | Rulebearing | Status |
| --- | --- | --- |
| `Slices().Matching("Ns.(*)")` | `matching: "Ns.(*)"` | Parity, Cross (path or module pattern) |
| `Slices().MatchingWithPackages("Ns.(**)")` | `matching: "Ns.(**)"` | Parity, Cross |
| `Should().NotDependOnEachOther()` | `should: notDependOnEachOther` | Parity, Cross |
| `Should().BeFreeOfCycles()` | `should: beFreeOfCycles` | Parity, Cross |
| slice filtering (`Where`, ignoring named slices) | `ignore: [...]`, `where` | Parity |

## PlantUML

| ArchUnitNET | Rulebearing | Status |
| --- | --- | --- |
| `Types().Should().AdhereToPlantUmlDiagram(file)` (component diagram with `<<..pattern..>>` stereotypes, aliases, associations) | `diagrams[]` rule: `adhereTo` | Parity, Cross |
| `PlantUmlDefinition.ComponentDiagram().WithDependenciesFromSlices(...)` / `WithDependenciesFromTypes` / `WithDependenciesFromNamespaces`, `WriteToFile` | `--output-type plantuml` with \`--from slices | types |
| `GenerationOptions`: `LimitDependencies`, `C4Style`, `FocusOn`, `IncludeDependenciesToOther`, `DependencyFilters` | reporter options of the same names | Parity |
| Import exceptions (`IllegalDiagramException`, `ComponentIntersectionException`, ...) | config-lint errors with the same meaning | Parity |

## Loader and caches

| ArchUnitNET | Rulebearing | Status |
| --- | --- | --- |
| `LoadAssembly`, `LoadAssemblies`, `LoadAssemblyIncludingDependencies`, `LoadAssembliesIncludingDependencies`, `LoadAssembliesRecursively` | `languages.dotnet.assemblies` (globs) and `includeDependencies: true` | Parity |
| `LoadFilteredDirectory(dir, filter)`, `LoadFilteredDirectoryIncludingDependencies` | `languages.dotnet.directories` with `filter` | Parity |
| `LoadNamespacesWithinAssembly(assembly, namespaces...)` | `languages.dotnet.namespaces` | Parity |
| `Build()` | implicit | Parity |
| `WithoutArchitectureCache`, `WithoutRuleEvaluationCache` | `--no-cache` | Parity |
| solution-driven loading (none in ArchUnitNET) | `languages.dotnet.solution` finds the built assemblies of every project | Addition |

## Test framework adapters

| ArchUnitNET | Rulebearing | Status |
| --- | --- | --- |
| `ArchUnitNET.xUnit`, `.xUnitV3`, `.NUnit`, `.MSTestV2`, `.MSTestV4`, `.TUnit` (`rule.Check(arch)` throws a framework assertion) | `Rulebearing.TestAdapter`: a `[Theory]` / `[TestCaseSource]` data source that runs the binary and yields one test per rule with the `fix` text in the failure message; `pytest-rulebearing` and a vitest reporter do the same for Python and TypeScript | Parity in effect |

## Stays in ArchUnitNET

- `FollowCustomPredicate` / `FollowCustomCondition` and `IPredicate<T>` / `ICondition<T>` implementations: arbitrary C#. A repo that needs one keeps a small ArchUnitNET test project beside its `rulebearing.yaml`, and `rulebearing docs` lists both.
- Reading assemblies that were never built or have no portable PDB: ArchUnitNET reads them without file attribution; Rulebearing reads them too, but element rules that need a file path report `attribution: none` and path-based rules skip those types with a warning.
