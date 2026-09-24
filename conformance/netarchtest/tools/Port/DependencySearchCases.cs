// NetArchTest 1.3.2 test/NetArchTest.Rules.UnitTests/DependencySearch/*.cs, test by test. Each
// Utils.RunDependencyTest call is two searches (by the dependency's full name, then by its
// namespace), and each search is one case, or one unported entry when it names a constructed type
// or an external namespace (Oracle.cs says why).
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7.
// The chains are upstream's C#, which is not nullable-annotated (Assembly.GetAssembly returns a
// nullable reference), so nullable analysis is off for this transcription only.
#nullable disable
using System.Reflection;
using System.Text.Json.Nodes;
using NetArchTest.Rules;
using NetArchTest.Rules.Dependencies;
using NetArchTest.TestStructure.Dependencies.Example;
using NetArchTest.TestStructure.Dependencies.Examples;
using NetArchTest.TestStructure.Dependencies.Implementation;
using NetArchTest.TestStructure.Dependencies.Search;
using NetArchTest.TestStructure.Dependencies.Search.DependencyLocation;
using NetArchTest.TestStructure.Dependencies.Search.DependencyType;
using NetArchTest.TestStructure.Dependencies.TypeOfSearch;
using NetArchTest.TestStructure.FalsePositives.NamespaceMatch;
using Array = NetArchTest.TestStructure.Dependencies.Search.DependencyType.Array;
using Pointer = NetArchTest.TestStructure.Dependencies.Search.DependencyType.Pointer;
using TypeDefinition = Mono.Cecil.TypeDefinition;

namespace Rulebearing.Conformance.NetArchTest;

/// <summary>VariousTests, SearchTypeTests, DependencyTypeTests, DependencyLocationTests, ScalabilityTests.</summary>
internal static class DependencySearchCases
{
    private const string ConstantReason =
        "dependency-definition: NetArchTest's HaveDependencyOnAny also reads the value of every const string field as a " +
        "dependency name (TypeDefinitionCheckingContext, serachForDependencyInFieldConstant), so ConstStringFieldValue " +
        "depends on the type its constant names; a string constant is not a dependency in the graph document, and no " +
        "element-rule key reads field values";

    private const string AnyViaObject =
        "The list's System entry matches System.Object, which every class inherits, so the System part of NetArchTest's " +
        "'a dependency on every entry' holds for every subject and the rule asks for the other two entries; onlyDependOn " +
        "ignores dependencies outside the analysed code, and every such dependency of the subjects is on a System type.";

    private static Assembly Structure => Assembly.GetAssembly(typeof(HasDependency));

    /// <summary>Registers every DependencySearch case.</summary>
    internal static void Register(Oracle o)
    {
        Various(o);
        SearchType(o);
        DependencyType(o);
        DependencyLocation(o);
        o.Class("DependencySearch.ScalabilityTests");
        const string Timing =
            "api-only: times NetArchTest's own search over in-memory stub Cecil type definitions to show it grows " +
            "linearly; no test-structure type is involved and the verdict is a timing (Rulebearing's scale target is " +
            "the nightly benchmark, docs/perf.md)";
        o.Unported("FindTypesWithAllDependencies_TimeGrowsLinearly", null, Timing);
        o.Unported("FindTypesWithAnyDependencies_TimeGrowsLinearly", null, Timing);
    }

    private static void Various(Oracle o)
    {
        o.Class("DependencySearch.VariousTests");
        o.RunDependencyTest("DependencySearch_IndirectReference_NotFound", typeof(IndirectReference), false);
        o.RunDependencyTestNames("DependencySearch_Garbage_NotFound", [typeof(IndirectReference)],
            ["System.Object::.ctor()", "T", "T1", "T2", "ctor()", "!1)", "::.ctor(!0"], false,
            "Utils.RunDependencyTest(Utils.GetTypesThatResideInTheSameNamespaceButWithoutGivenType(typeof(IndirectReference)), new List<string> { \"System.Object::.ctor()\", \"T\", \"T1\", \"T2\", \"ctor()\", \"!1)\", \"::.ctor(!0\" }, false)",
            "No analysed type has a full name the strings begin in whole segments, so the selector selects none.");
        o.RunDependencyTestExcept("DependencySearch_PartiallyMatchingDependency_NotFound", [typeof(IndirectReference)], typeof(ExampleDep), false, true, Gaps.SearchSubjects);
        o.RunDependencyTestExcept("DependencySearch_PartiallyMatchingNamespace_NotFound", [typeof(IndirectReference)], typeof(ExampleDependencyInPartiallyMatchingNamespace), false, false);
        o.RunDependencyTestExcept("DependencySearch_DependencyWithDifferentCaseOfCharacters_NotFound", [typeof(IndirectReference)], typeof(ExampleDEPENDENCY), false, true, Gaps.SearchSubjects);

        foreach (var (test, example, entry) in new[]
        {
            ("FindTypesWithAllDependencies_PatternMatchedClasses_NotReturned", "ClassMatchingExample", typeof(PatternMatch).FullName),
            ("FindTypesWithAllDependencies_PatternMatchedNamespaces_NotReturned", "NamespaceMatchingExample", typeof(PatternMatch).Namespace),
        })
        {
            o.SearchWith(test,
                () => Types.InAssembly(Assembly.GetAssembly(typeof(HasDependency))).That().HaveName(example).GetTypeDefinitions(),
                inputs => new DependencySearch().FindTypesThatHaveDependencyOnAll(inputs, [entry]),
                (_, hits) => hits.Count == 0,
                s => Nat.HaveName(s, example),
                s => Nat.HaveDependencyOnAll(s, [entry]),
                $"Types.InAssembly(Assembly.GetAssembly(typeof(HasDependency))).That().HaveName(\"{example}\").GetTypeDefinitions()",
                $"FindTypesThatHaveDependencyOnAll(typeList, {Oracle.Quoted([entry])})");
        }
    }

    private static void SearchType(Oracle o)
    {
        o.Class("DependencySearch.SearchTypeTests");
        var implementation = typeof(HasDependency).Namespace;
        IEnumerable<TypeDefinition> Implementation() => Types.InAssembly(Structure).That().ResideInNamespace(implementation).GetTypeDefinitions();
        Func<Side, JsonNode> inImplementation = s => Nat.ResideInNamespace(s, implementation);
        const string ImplementationText = "Types.InAssembly(Assembly.GetAssembly(typeof(HasDependency))).That().ResideInNamespace(typeof(HasDependency).Namespace).GetTypeDefinitions()";

        string[][] any =
        [
            ["NetArchTest.TestStructure.Dependencies.Examples.ExampleDependency", "NetArchTest.TestStructure.Dependencies.Examples.AnotherExampleDependency"],
            ["NetArchTest.TestStructure.Dependencies.Examples.ExampleDependency", "NetArchTest.TestStructure.Dependencies.Examples.AnotherExampleDependency", "NetArchTest.TestStructure.Dependencies"],
            ["NetArchTest.TestStructure.Dependencies"],
            ["NetArchTest.TestStructure.Dependencies", "NetArchTest.TestStructure.Dependencies"],
            ["NetArchTest.TestStructure.Dependencies", "NetArchTest.TestStructure.Dependencies.Examples"],
        ];
        string[] anyFound = [Oracle.Name(typeof(HasAnotherDependency)), Oracle.Name(typeof(HasDependencies)), Oracle.Name(typeof(HasDependency))];
        for (var i = 0; i < any.Length; i++)
        {
            var entries = any[i];
            o.SearchWith($"FindTypesWithAnyDependencies_Found[{i + 1}]", Implementation,
                inputs => new DependencySearch().FindTypesThatHaveDependencyOnAny(inputs, entries),
                (_, hits) => hits.Select(Oracle.Name).SequenceEqual(anyFound),
                inImplementation, s => Nat.HaveDependencyOnAny(s, entries), ImplementationText,
                $"FindTypesThatHaveDependencyOnAny(typeList, {Oracle.Quoted(entries)})");
        }

        string[][] all =
        [
            ["NetArchTest.TestStructure.Dependencies.Examples.ExampleDependency", "NetArchTest.TestStructure.Dependencies.Examples.AnotherExampleDependency"],
            ["NetArchTest.TestStructure.Dependencies.Examples.ExampleDependency", "NetArchTest.TestStructure.Dependencies.Examples.AnotherExampleDependency", "NetArchTest.TestStructure.Dependencies"],
            ["NetArchTest.TestStructure.Dependencies.Examples.ExampleDependency", "NetArchTest.TestStructure.Dependencies.Examples.AnotherExampleDependency", "NetArchTest.TestStructure.Dependencies.Examples.AnotherExampleDependency"],
            ["NetArchTest.TestStructure.Dependencies.Examples.ExampleDependency", "NetArchTest.TestStructure.Dependencies.Examples.AnotherExampleDependency", "NetArchTest.TestStructure.Dependencies", "NetArchTest.TestStructure.Dependencies.Examples"],
        ];
        for (var i = 0; i < all.Length; i++)
        {
            var entries = all[i];
            o.SearchWith($"FindTypesWithAllDependencies_Found[{i + 1}]", Implementation,
                inputs => new DependencySearch().FindTypesThatHaveDependencyOnAll(inputs, entries),
                (_, hits) => hits.Select(Oracle.Name).SequenceEqual([Oracle.Name(typeof(HasDependencies))]),
                inImplementation, s => Nat.HaveDependencyOnAll(s, entries), ImplementationText,
                $"FindTypesThatHaveDependencyOnAll(typeList, {Oracle.Quoted(entries)})");
        }

        var typeOfSearch = typeof(Class_A).Namespace;
        IEnumerable<TypeDefinition> Classes() => Types.InAssembly(Structure).That().ResideInNamespace(typeOfSearch).And().HaveNameStartingWith("Class").GetTypeDefinitions();
        Func<Side, JsonNode> classes = s => Expr.All(Nat.ResideInNamespace(s, typeOfSearch), Nat.HaveNameStartingWith(s, "Class"));
        const string ClassesText = "Types.InAssembly(Assembly.GetAssembly(typeof(Class_A))).That().ResideInNamespace(typeof(Class_A).Namespace).And().HaveNameStartingWith(\"Class\").GetTypeDefinitions()";
        string[] two = [Oracle.Name(typeof(Dependency_1)), Oracle.Name(typeof(Dependency_2))];
        string[] withSystem = [.. two, "System"];
        static Func<List<TypeDefinition>, IReadOnlyList<TypeDefinition>, bool> Returns(params Type[] types) =>
            (_, hits) => hits.Select(Oracle.Name).SequenceEqual(types.Select(Oracle.Name));

        o.SearchWith("FindTypesThatHaveDependencyOnAny_Found", Classes,
            inputs => new DependencySearch().FindTypesThatHaveDependencyOnAny(inputs, two),
            Returns(typeof(Class_C), typeof(Class_D), typeof(Class_E), typeof(Class_F), typeof(Class_G), typeof(Class_H)),
            classes, s => Nat.HaveDependencyOnAny(s, two), ClassesText,
            $"FindTypesThatHaveDependencyOnAny(typeList, {Oracle.Quoted(two)})");
        o.SearchWith("FindTypesThatHaveDependencyOnAll_Found", Classes,
            inputs => new DependencySearch().FindTypesThatHaveDependencyOnAll(inputs, two),
            Returns(typeof(Class_G), typeof(Class_H)),
            classes, s => Nat.HaveDependencyOnAll(s, two), ClassesText,
            $"FindTypesThatHaveDependencyOnAll(typeList, {Oracle.Quoted(two)})");
        o.SearchWith("FindTypesThatOnlyHaveDependenciesOnAnyOrNone_Found", Classes,
            inputs => new DependencySearch().FindTypesThatOnlyHaveDependenciesOnAnyOrNone(inputs, withSystem),
            Returns(typeof(Class_A), typeof(Class_C), typeof(Class_E), typeof(Class_G)),
            classes, s => Nat.OnlyHaveDependenciesOn(s, withSystem), ClassesText,
            $"FindTypesThatOnlyHaveDependenciesOnAnyOrNone(typeList, {Oracle.Quoted(withSystem)})", Gaps.SystemNote,
            Gaps.SelfReference("Class_A", "a method reads its own stringField"));
        o.SearchWith("FindTypesThatOnlyHaveDependenciesOnAny_Found", Classes,
            inputs => new DependencySearch().FindTypesThatOnlyHaveDependenciesOnAny(inputs, withSystem),
            Returns(typeof(Class_A), typeof(Class_C), typeof(Class_E), typeof(Class_G)),
            classes, s => Nat.OnlyHaveDependenciesOn(s, withSystem), ClassesText,
            $"FindTypesThatOnlyHaveDependenciesOnAny(typeList, {Oracle.Quoted(withSystem)})", AnyViaObject,
            Gaps.SelfReference("Class_A", "a method reads its own stringField"));
        o.SearchWith("FindTypesThatOnlyOnlyHaveDependenciesOnAll_Found", Classes,
            inputs => new DependencySearch().FindTypesThatOnlyOnlyHaveDependenciesOnAll(inputs, withSystem),
            Returns(typeof(Class_G)),
            classes, s => Expr.All(Nat.OnlyHaveDependenciesOn(s, withSystem), Nat.HaveDependencyOnAll(s, two)), ClassesText,
            $"FindTypesThatOnlyOnlyHaveDependenciesOnAll(typeList, {Oracle.Quoted(withSystem)})", AnyViaObject);
    }

    private static void DependencyType(Oracle o)
    {
        o.Class("DependencySearch.DependencyTypeTests");
        o.RunDependencyTest("DependencySearch_Array_Found", typeof(Array), typeof(ExampleDependency[]), true, true);
        o.RunDependencyTestExcept("DependencySearch_Array_NotFound", [typeof(Array), typeof(ArrayJagged)], typeof(ExampleDependency[]), false, true);
        o.RunDependencyTest("DependencySearch_ArrayNested_Found", typeof(Array));
        o.RunDependencyTest("DependencySearch_ArrayJagged_Found", typeof(ArrayJagged), typeof(ExampleDependency[][]), true, true);
        o.RunDependencyTestExcept("DependencySearch_ArrayJagged_NotFound", [typeof(ArrayJagged)], typeof(ExampleDependency[][]), false, true);
        o.RunDependencyTest("DependencySearch_ArrayJaggedArray_Found", typeof(ArrayJagged), typeof(ExampleDependency[]), true, true);
        o.RunDependencyTest("DependencySearch_ArrayJaggedNested_Found", typeof(ArrayJagged));
        o.RunDependencyTest("DependencySearch_ArrayMultidimensional_Found", typeof(ArrayMultidimensional), typeof(ExampleDependency[,,,]), true, true);
        o.RunDependencyTestExcept("DependencySearch_ArrayMultidimensional_NotFound", [typeof(ArrayMultidimensional)], typeof(ExampleDependency[,]), false, true);
        o.RunDependencyTest("DependencySearch_ArrayMultidimensionalNested_Found", typeof(ArrayMultidimensional));
        o.RunDependencyTest("DependencySearch_ArrayGeneric_Found", typeof(ArrayOfGenerics), typeof(ExampleDependency<int>[]), true, true);
        o.RunDependencyTestExcept("DependencySearch_ArrayGeneric_NotFound", [typeof(StaticGenericClass)], typeof(ExampleDependency<string>[]), false, true);
        o.RunDependencyTest("DependencySearch_ArrayGenericNested_Found", typeof(ArrayOfGenerics), typeof(ExampleDependency<int>), true, true);
        o.RunDependencyTestExcept("DependencySearch_ArrayGenericNested_NotFound", [typeof(StaticGenericClass)], typeof(ExampleDependency<string>), false, true);
        o.RunDependencyTest("DependencySearch_ArrayGenericNestedOpen_Found", typeof(ArrayOfGenerics), typeof(ExampleDependency<>), true, true);
        o.RunDependencyTest("DependencySearch_ArrayGenericNot_NotFound", typeof(ArrayOfGenerics), typeof(ExampleDependency), false, true);
        o.RunDependencyTest("DependencySearch_ArrayOfGenericsTypeArgument_Found", typeof(ArrayOfGenericsTypeArgument));
        o.RunDependencyTest("DependencySearch_ArrayOfGenericsTypeArgument_NotFound", typeof(ArrayOfGenericsTypeArgument), typeof(ExampleDependency[]), false, true);
        o.RunDependencyTest("DependencySearch_MethodParameter_NotFound[MethodParameterIn]", typeof(MethodParameterIn));
        o.RunDependencyTest("DependencySearch_MethodParameter_NotFound[MethodParameterOut]", typeof(MethodParameterOut));
        o.RunDependencyTest("DependencySearch_MethodParameter_NotFound[MethodParameterRef]", typeof(MethodParameterRef));
        o.RunDependencyTest("DependencySearch_NestedDependencyClass_Found", typeof(NestedDependencyClass), typeof(NestedDependencyTree.NestedLevel1.NestedLevel2.NestedDependency), true, true);
        o.RunDependencyTestExcept("DependencySearch_NestedDependency_NotFound", [typeof(NestedDependencyClass)], typeof(NestedDependencyTree.NestedLevel1.NestedLevel2.NestedDependency), false, true);
        o.RunDependencyTest("DependencySearch_NestedDependencyClassGeneric_Found", typeof(NestedDependencyClassGeneric), typeof(NestedDependencyTree.NestedLevel1.NestedLevel2.NestedDependency<int>), true, true);
        o.RunDependencyTestExcept("DependencySearch_NestedDependencyClassGeneric1_NotFound", [typeof(NestedDependencyClassGeneric)], typeof(NestedDependencyTree.NestedLevel1.NestedLevel2.NestedDependency<int>), false, true);
        o.RunDependencyTestExcept("DependencySearch_NestedDependencyClassGeneric2_NotFound", [typeof(StaticGenericClass)], typeof(NestedDependencyTree.NestedLevel1.NestedLevel2.NestedDependency<double>), false, true);
        o.RunDependencyTest("DependencySearch_NestedDependencyClassGenericLevel2Generic_Found", typeof(NestedDependencyClassGenericLevel2Generic), typeof(NestedDependencyTree.NestedLevel1.NestedLevel2<int>.NestedDependency<int>), true, true);
        o.RunDependencyTestExcept("DependencySearch_NestedDependencyClassGenericLevel2Generic1_NotFound", [typeof(NestedDependencyClassGenericLevel2Generic)], typeof(NestedDependencyTree.NestedLevel1.NestedLevel2<int>.NestedDependency<int>), false, true);
        o.RunDependencyTestExcept("DependencySearch_NestedDependencyClassGenericLevel2Generic2_NotFound", [typeof(StaticGenericClass)], typeof(NestedDependencyTree.NestedLevel1.NestedLevel2<int>.NestedDependency<double>), false, true);
        o.RunDependencyTestExcept("DependencySearch_NestedDependencyClassGenericLevel2Generic3_NotFound", [typeof(StaticGenericClass)], typeof(NestedDependencyTree.NestedLevel1.NestedLevel2<double>.NestedDependency<int>), false, true);
        o.RunDependencyTest("DependencySearch_NestedDependencyClassLevel2Generic_Found", typeof(NestedDependencyClassLevel2Generic), typeof(NestedDependencyTree.NestedLevel1.NestedLevel2<int>.NestedDependency), true, true);
        o.RunDependencyTestExcept("DependencySearch_NestedDependencyClassLevel2Generic1_NotFound", [typeof(NestedDependencyClassLevel2Generic)], typeof(NestedDependencyTree.NestedLevel1.NestedLevel2<int>.NestedDependency), false, true);
        o.RunDependencyTestExcept("DependencySearch_NestedDependencyClassLevel2Generic2_NotFound", [typeof(StaticGenericClass)], typeof(NestedDependencyTree.NestedLevel1.NestedLevel2<double>.NestedDependency), false, true);
        o.RunDependencyTest("DependencySearch_Pointer_Found", typeof(Pointer), typeof(StructDependency).MakePointerType(), true, true);
        o.RunDependencyTest("DependencySearch_Pointer_NotFound", typeof(PointerNot), typeof(StructDependency).MakePointerType(), false, true);
        o.RunDependencyTest("DependencySearch_PointerNested_Found", typeof(Pointer), typeof(StructDependency), true, true);
        o.RunDependencyTest("DependencySearch_StaticGenericClass_Found", typeof(StaticGenericClass), typeof(StaticGenericDependency<>), true, true);
        o.RunDependencyTest("DependencySearch_Variable_Found", typeof(Variable));
        o.RunDependencyTest("DependencySearch_VariableGeneric_Found", typeof(VariableGeneric), typeof(ExampleDependency<>), true, true);
        o.RunDependencyTest("DependencySearch_VariableGenericSimple_NotFound", typeof(VariableGeneric), typeof(ExampleDependency), false, true);
        o.RunDependencyTestExcept("DependencySearch_VariableGeneric_NotFound", [typeof(ArrayOfGenerics), typeof(VariableGeneric)], typeof(ExampleDependency<>).MakeByRefType(), false, true);
        o.RunDependencyTest("DependencySearch_VariableGenericClosed_Found", typeof(VariableGeneric), typeof(ExampleDependency<int>), true, true);
        o.Unported("DependencySearch_VariableGenericAsString_Found",
            $"Utils.RunDependencyTest(typeof(VariableGeneric), new[] {{ typeof(ExampleDependency<int>).ToString() }}, true): FindTypesThatHaveDependencyOnAny(subjects, [\"{typeof(ExampleDependency<int>)}\"])",
            Oracle.ConstructedReason);
        o.RunDependencyTest("DependencySearch_VariableGenericClosed_NotFound", typeof(VariableGeneric), typeof(ExampleDependency<string>), false, true);
        o.RunDependencyTest("DependencySearch_VariableGenericTypeArgument_Found", typeof(VariableGenericTypeArgument));
        o.RunDependencyTest("DependencySearch_VariableGenericTypeArgument_NotFound", typeof(VariableGenericTypeArgument), typeof(List<ExampleDependency>), false, false);
        o.RunDependencyTest("DependencySearch_VariableGenericTypeArgumentNested_Found", typeof(VariableGenericTypeArgumentNested), typeof(GenericClass<GenericClass<ExampleDependency>>), true, true);
        o.RunDependencyTest("DependencySearch_VariableGenericTypeArgumentNestedNested_Found", typeof(VariableGenericTypeArgumentNested));
        o.RunDependencyTest("DependencySearch_VariableRef_Found", typeof(VariableRef), typeof(ExampleDependency).MakeByRefType(), true, true);
        o.RunDependencyTest("DependencySearch_VariableRefNested_Found", typeof(VariableRef));
        o.RunDependencyTestExcept("DependencySearch_VariableRef_NotFound", [typeof(VariableRef), typeof(VariableRefGenericTypeArgument), typeof(MethodParameterIn), typeof(MethodParameterOut), typeof(MethodParameterRef)], typeof(ExampleDependency).MakeByRefType(), false, true);
        o.RunDependencyTest("DependencySearch_VariableRefGeneric_Found", typeof(VariableRefGenericTypeArgument), typeof(GenericClass<ExampleDependency>).MakeByRefType(), true, true);
        o.RunDependencyTest("DependencySearch_VariableRefArrayOfGenericsTypeArgumentByRef_Found", typeof(VariableRefArrayOfGenericsTypeArgument), typeof(GenericClass<ExampleDependency>[]).MakeByRefType(), true, true);
        o.RunDependencyTest("DependencySearch_VariableRefArrayOfGenericsTypeArgument_Found", typeof(VariableRefArrayOfGenericsTypeArgument), typeof(GenericClass<ExampleDependency>[]), true, true);
        o.RunDependencyTest("DependencySearch_VariableRefArrayOfGenericsTypeArgumentNested_Found", typeof(VariableRefArrayOfGenericsTypeArgument), typeof(GenericClass<ExampleDependency>), true, true);
        o.RunDependencyTest("DependencySearch_VariableTuple_Found", typeof(VariableTuple), typeof(Tuple<int, ExampleDependency>), true, true);
        o.RunDependencyTest("DependencySearch_VariableTupleNested_Found", typeof(VariableTuple));
        o.RunDependencyTest("DependencySearch_VariableTuple_NotFound", typeof(VariableTuple), typeof(Tuple<int, double>), false, true);
        o.RunDependencyTest("DependencySearch_ConstFieldString_Found", typeof(ConstStringFieldValue), typeof(Array), true, true,
            classReason: ConstantReason, namespaceReason: ConstantReason);
        o.RunDependencyTest("DependencySearch_ConstFieldString_NotFound", typeof(ConstStringFieldValue), typeof(ArrayJagged), false, true,
            namespaceReason: ConstantReason);
        o.RunDependencyTest("DependencySearch_BaseCtorCall_Found", typeof(BaseCtorCall), typeof(StaticType), true, true);
    }

    private static void DependencyLocation(Oracle o)
    {
        o.Class("DependencySearch.DependencyLocationTests");
        o.RunDependencyTest("DependencySearch_AsyncMethod_Found", typeof(AsyncMethod));
        foreach (var input in new[] { typeof(AttributeOnClass), typeof(AttributeOnEvent), typeof(AttributeOnField), typeof(AttributeOnMethod), typeof(AttributeOnParameter), typeof(AttributeOnProperty), typeof(AttributeOnReturnValue) })
        {
            var eventReason = input == typeof(AttributeOnEvent) ? Gaps.EventAttribute : null;
            o.RunDependencyTest($"DependencySearch_Attribute_Found[{input.Name}]", input, typeof(AttributeDependency), true, true,
                eventReason, eventReason, inputText: $"typeof({input.Name})");
        }
        o.RunDependencyTest("DependencySearch_ConstructorPrivate_Found", typeof(ConstructorPrivate));
        o.RunDependencyTest("DependencySearch_ConstructorPublic_Found", typeof(ConstructorPublic));
        o.RunDependencyTest("DependencySearch_DefaultInterfaceMethodBody_Found", typeof(DefaultInterfaceMethodBody));
        o.RunDependencyTest("DependencySearch_DelegateDeclaration_Found", typeof(DelegateDeclaration));
        o.RunDependencyTest("DependencySearch_EventAdd_Found", typeof(EventAdd));
        o.RunDependencyTest("DependencySearch_EventPublic_Found", typeof(EventPublic));
        o.RunDependencyTest("DependencySearch_EventRemove_Found", typeof(EventRemove));
        o.RunDependencyTest("DependencySearch_FieldPrivate_Found", typeof(FieldPrivate));
        o.RunDependencyTest("DependencySearch_FieldPublic_Found", typeof(FieldPublic));
        o.RunDependencyTest("DependencySearch_GenericConstraintClass_Found", typeof(GenericConstraintClass<>));
        o.RunDependencyTest("DependencySearch_GenericConstraintMethod_Found", typeof(GenericConstraintMethod));
        o.RunDependencyTest("DependencySearch_GenericMethodTypeArgument_Found[GenericMethodTypeArgument]", typeof(GenericMethodTypeArgument));
        o.RunDependencyTest("DependencySearch_GenericMethodTypeArgument_Found[GenericMethodTypeArgumentOneOpenOneClosedTypeArgument]", typeof(GenericMethodTypeArgumentOneOpenOneClosedTypeArgument<>));
        o.RunDependencyTest("DependencySearch_ImplementedInterface_Found", typeof(ImplementedInterface));
        o.RunDependencyTest("DependencySearch_IndexerPublic_Found", typeof(IndexerPublic));
        o.RunDependencyTest("DependencySearch_InheritedBaseClass_Found", typeof(InheritedBaseClass));
        o.RunDependencyTest("DependencySearch_Instruction_Found[InstructionCtor]", typeof(InstructionCtor));
        o.RunDependencyTest("DependencySearch_Instruction_Found[InstructionStaticClassTypeArgument]", typeof(InstructionStaticClassTypeArgument));
        o.RunDependencyTest("DependencySearch_Instruction_Found[InstructionStaticMethodTypeArgument]", typeof(InstructionStaticMethodTypeArgument));
        o.RunDependencyTest("DependencySearch_InstructionThrow_Found", typeof(InstructionThrow), typeof(ExceptionDependency), true, true);
        o.RunDependencyTest("DependencySearch_LambdaCapturedVariable_Found", typeof(LambdaCapturedVariable), reason: Gaps.Closure);
        o.RunDependencyTest("DependencySearch_MethodArgument_Found", typeof(MethodArgument));
        o.RunDependencyTest("DependencySearch_MethodParameter_Found", typeof(MethodParameter));
        o.RunDependencyTest("DependencySearch_MethodPrivateBody_Found", typeof(MethodPrivateBody));
        o.RunDependencyTest("DependencySearch_MethodReturnType_Found", typeof(MethodReturnType));
        o.RunDependencyTest("DependencySearch_PropertyPrivate_Found", typeof(PropertyPrivate));
        o.RunDependencyTest("DependencySearch_PropertyPublic_Found", typeof(PropertyPublic));
        o.RunDependencyTest("DependencySearch_PInvoke_Found", typeof(PInvoke));
        o.RunDependencyTest("DependencySearch_PropertyGetter_Found", typeof(PropertyGetter));
        o.RunDependencyTest("DependencySearch_PropertySetter_Found", typeof(PropertySetter));
        o.RunDependencyTest("DependencySearch_StaticLocalFunctions_Found", typeof(StaticLocalFunction), reason: Gaps.LocalFunction);
        o.RunDependencyTest("DependencySearch_SwitchPatternMatching_Found", typeof(SwitchPatternMatching));
        o.RunDependencyTest("DependencySearch_TryCatch_Found", typeof(TryCatch), typeof(ExceptionDependency), true, true);
        o.RunDependencyTest("DependencySearch_TryCatchBlock_Found", typeof(TryCatchBlock), typeof(ExceptionDependency), true, true);
        o.RunDependencyTest("DependencySearch_TryCatchExceptionFilter_Found", typeof(TryCatchExceptionFilter), typeof(ExceptionDependency), true, true);
        o.RunDependencyTest("DependencySearch_TryFinallyBlock_Found", typeof(TryFinallyBlock));
        o.RunDependencyTest("DependencySearch_UsingStatement_Found", typeof(UsingStatement), typeof(DisposableDependency), true, true);
        o.RunDependencyTest("DependencySearch_Yield_Found", typeof(Yield));
    }
}
