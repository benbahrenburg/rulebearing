// NetArchTest 1.3.2 test/NetArchTest.Rules.UnitTests/{ConditionListTests,PredicateListTests,
// FunctionSequenceTests}.cs: the And / Or / ShouldNot combinators, as `all`, `any` and `not`.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7.
// The chains are upstream's C#, which is not nullable-annotated (Assembly.GetAssembly returns a
// nullable reference), so nullable analysis is off for this transcription only.
#nullable disable
using System.Reflection;
using System.Text.Json.Nodes;
using NetArchTest.Rules;
using NetArchTest.TestStructure.Abstract;
using NetArchTest.TestStructure.Generic;
using NetArchTest.TestStructure.Interfaces;
using NetArchTest.TestStructure.NameMatching.Namespace1;
using NetArchTest.TestStructure.NameMatching.Namespace2;
using NetArchTest.TestStructure.NameMatching.Namespace2.Namespace3;

namespace Rulebearing.Conformance.NetArchTest;

/// <summary>ConditionListTests, PredicateListTests and FunctionSequenceTests.</summary>
internal static class ListCases
{
    private const string NameMatching = "NetArchTest.TestStructure.NameMatching";

    private static Assembly Structure => Assembly.GetAssembly(typeof(ClassA1));

    private static Predicates That() => Types.InAssembly(Structure).That();

    private static Func<Side, JsonNode> Ns(string ns) => s => Nat.ResideInNamespace(s, ns);

    /// <summary>Registers every combinator case.</summary>
    internal static void Register(Oracle o)
    {
        ConditionList(o);
        PredicateList(o);
        FunctionSequence(o);
    }

    private static void ConditionList(Oracle o)
    {
        o.Class("ConditionListTests");
        IEnumerable<Type> All() => That().ResideInNamespace(NameMatching).GetTypes();
        IEnumerable<Type> NotNamespace3() => That().ResideInNamespace(NameMatching).And().DoNotResideInNamespace(NameMatching + ".Namespace3").GetTypes();
        Func<Side, JsonNode> notNamespace3 = s => Expr.All(Nat.ResideInNamespace(s, NameMatching), Nat.ResideInNamespace(s, NameMatching + ".Namespace3", negated: true));

        o.Predicate("Or_AppliedToConditions_SelectCorrectTypes",
            All,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").Should().HaveNameStartingWith("ClassA").Or().HaveNameEndingWith("1").Or().HaveNameEndingWith("2").GetTypes(),
            Ns(NameMatching),
            s => Expr.Any(Nat.HaveNameStartingWith(s, "ClassA"), Nat.HaveNameEndingWith(s, "1"), Nat.HaveNameEndingWith(s, "2")),
            5, [typeof(ClassA1), typeof(ClassA2), typeof(ClassA3), typeof(ClassB1), typeof(ClassB2)]);

        o.Predicate("And_AppliedToConditions_SelectCorrectTypes",
            All,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").Should().HaveNameStartingWith("Class").And().HaveNameEndingWith("1").And().BeClasses().GetTypes(),
            Ns(NameMatching),
            s => Expr.All(Nat.HaveNameStartingWith(s, "Class"), Nat.HaveNameEndingWith(s, "1"), Nat.BeClass(s)),
            2, [typeof(ClassA1), typeof(ClassB1)]);

        o.Predicate("Or_MultipleInstances_TreatedAsSeparateGroups",
            () => That().ResideInNamespace(NameMatching + ".Namespace2").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace2").Should().HaveNameStartingWith("ClassA").And().HaveNameEndingWith("3").Or().HaveNameStartingWith("ClassB").And().HaveNameEndingWith("2").GetTypes(),
            Ns(NameMatching + ".Namespace2"),
            s => Expr.Any(
                Expr.All(Nat.HaveNameStartingWith(s, "ClassA"), Nat.HaveNameEndingWith(s, "3")),
                Expr.All(Nat.HaveNameStartingWith(s, "ClassB"), Nat.HaveNameEndingWith(s, "2"))),
            2, [typeof(ClassA3), typeof(ClassB2)]);

        // ShouldNot() inverts the conditions that follow: `should: { not: ... }`. Upstream asserts
        // that each pair of results agrees; each result is one case.
        const string Interfaces = "NetArchTest.TestStructure.Interfaces";
        IEnumerable<Type> Implements() => That().ResideInNamespace(Interfaces).And().HaveNameStartingWith("Implements").GetTypes();
        Func<Side, JsonNode> implements = s => Expr.All(Nat.ResideInNamespace(s, Interfaces), Nat.HaveNameStartingWith(s, "Implements"));
        o.Condition("ShouldNot_FollowingConditions_Inversed",
            Implements,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Interfaces").And().HaveNameStartingWith("Implements").Should().ImplementInterface(typeof(IExample)).GetResult(),
            implements, s => Nat.ImplementInterface(s, typeof(IExample)), true);
        o.Condition("ShouldNot_FollowingConditions_Inversed",
            Implements,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Interfaces").And().HaveNameStartingWith("Implements").ShouldNot().NotImplementInterface(typeof(IExample)).GetResult(),
            implements, s => Expr.Not(Nat.ImplementInterface(s, typeof(IExample), negated: true)), true);
        const string Spaced = " NetArchTest.TestStructure.NameMatching.Namespace1";
        IEnumerable<Type> SpacedSelection() => That().ResideInNamespace(Spaced).GetTypes();
        o.Condition("ShouldNot_FollowingConditions_Inversed",
            SpacedSelection,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(" NetArchTest.TestStructure.NameMatching.Namespace1").Should().HaveNameStartingWith("ClassA").Or().HaveNameStartingWith("ClassB").GetResult(),
            Ns(Spaced), s => Expr.Any(Nat.HaveNameStartingWith(s, "ClassA"), Nat.HaveNameStartingWith(s, "ClassB")), true);
        o.Condition("ShouldNot_FollowingConditions_Inversed",
            SpacedSelection,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(" NetArchTest.TestStructure.NameMatching.Namespace1").ShouldNot().NotHaveNameStartingWith("ClassA").Or().NotHaveNameStartingWith("ClassB").GetResult(),
            Ns(Spaced), s => Expr.Not(Expr.Any(Nat.HaveNameStartingWith(s, "ClassA", negated: true), Nat.HaveNameStartingWith(s, "ClassB", negated: true))), true);

        o.Condition("GetResult_Failed_ReturnFailedTypes",
            NotNamespace3,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().DoNotResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace3").Should().HaveNameStartingWith("ClassA").GetResult(),
            notNamespace3, s => Nat.HaveNameStartingWith(s, "ClassA"), false, [typeof(ClassB1), typeof(ClassB2)]);

        o.Condition("GetResult_FailedShouldNot_ReturnFailedTypes",
            All,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").ShouldNot().HaveNameStartingWith("ClassA").GetResult(),
            Ns(NameMatching), s => Expr.Not(Nat.HaveNameStartingWith(s, "ClassA")), false, [typeof(ClassA1), typeof(ClassA2), typeof(ClassA3)]);

        o.Condition("GetResult_Success_ReturnNullFailedTypes",
            NotNamespace3,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().DoNotResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace3").Should().HaveNameStartingWith("ClassA").Or().HaveNameEndingWith("1").Or().HaveNameEndingWith("2").GetResult(),
            notNamespace3, s => Expr.Any(Nat.HaveNameStartingWith(s, "ClassA"), Nat.HaveNameEndingWith(s, "1"), Nat.HaveNameEndingWith(s, "2")), true);
    }

    private static void PredicateList(Oracle o)
    {
        o.Class("PredicateListTests");
        IEnumerable<Type> Everything() => Types.InAssembly(Structure).GetTypes();

        o.Predicate("Or_AppliedToPredicates_SelectCorrectTypes",
            Everything,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace1").Or().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace2").Or().ResideInNamespace("NetArchTest.TestStructure.Generic").GetTypes(),
            null,
            s => Expr.Any(Nat.ResideInNamespace(s, NameMatching + ".Namespace1"), Nat.ResideInNamespace(s, NameMatching + ".Namespace2"), Nat.ResideInNamespace(s, "NetArchTest.TestStructure.Generic")),
            7, [typeof(ClassA1), typeof(ClassA2), typeof(ClassA3), typeof(ClassB1), typeof(ClassB2), typeof(GenericType<>), typeof(NonGenericType)]);

        o.Predicate("And_AppliedToPredicates_SelectCorrectTypes",
            Everything,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace1").And().HaveNameStartingWith("Class").And().HaveNameEndingWith("1").GetTypes(),
            null,
            s => Expr.All(Nat.ResideInNamespace(s, NameMatching + ".Namespace1"), Nat.HaveNameStartingWith(s, "Class"), Nat.HaveNameEndingWith(s, "1")),
            2, [typeof(ClassA1), typeof(ClassB1)]);

        o.Predicate("Or_MultipleInstances_TreatedAsSeparateGroups",
            Everything,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace1").And().HaveNameStartingWith("ClassA").Or().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace2").And().HaveNameStartingWith("ClassB").GetTypes(),
            null,
            s => Expr.Any(
                Expr.All(Nat.ResideInNamespace(s, NameMatching + ".Namespace1"), Nat.HaveNameStartingWith(s, "ClassA")),
                Expr.All(Nat.ResideInNamespace(s, NameMatching + ".Namespace2"), Nat.HaveNameStartingWith(s, "ClassB"))),
            3, [typeof(ClassA1), typeof(ClassA2), typeof(ClassB2)]);
    }

    private static void FunctionSequence(Oracle o)
    {
        o.Class("FunctionSequenceTests");
        {
            var sequence = new global::NetArchTest.Rules.FunctionSequence();
            sequence.AddFunctionCall(FunctionDelegates.BeAbstract, true, true);
            var types = Types.InAssembly(Assembly.GetAssembly(typeof(AbstractClass))).That().ResideInNamespace("NetArchTest.TestStructure.Abstract").GetTypeDefinitions().ToList();
            var selected = sequence.Execute(types).Select(Oracle.Name).ToList();
            var notSelected = sequence.Execute(types, selected: false).Select(Oracle.Name).ToList();
            Assert(selected.SequenceEqual([Oracle.Name(typeof(AbstractClass))]) && notSelected.SequenceEqual([Oracle.Name(typeof(ConcreteClass))]), "Execute_SelectedFalse_ReturnsFailedTypes");
            o.Split("Execute_SelectedFalse_ReturnsFailedTypes", types.Select(Oracle.Name), selected,
                s => Nat.ResideInNamespace(s, "NetArchTest.TestStructure.Abstract"), s => Nat.BeAbstract(s),
                "sequence.AddFunctionCall(FunctionDelegates.BeAbstract, true, true); sequence.Execute(Types.InAssembly(Assembly.GetAssembly(typeof(AbstractClass))).That().ResideInNamespace(\"NetArchTest.TestStructure.Abstract\").GetTypeDefinitions(), selected: false)");
        }
        {
            var sequence = new global::NetArchTest.Rules.FunctionSequence();
            sequence.AddFunctionCall(FunctionDelegates.HaveNameStartingWith, "ClassA", true);
            sequence.CreateGroup();
            sequence.AddFunctionCall(FunctionDelegates.HaveName, "ClassB1", true);
            var types = Types.InAssembly(Assembly.GetAssembly(typeof(ClassB2))).That().ResideInNamespace(NameMatching).And().DoNotResideInNamespace(NameMatching + ".Namespace3").GetTypeDefinitions().ToList();
            var selected = sequence.Execute(types).Select(Oracle.Name).ToList();
            var notSelected = sequence.Execute(types, selected: false).Select(Oracle.Name).ToList();
            Assert(selected.Count == 4 && !selected.Contains(Oracle.Name(typeof(ClassB2))) && notSelected.SequenceEqual([Oracle.Name(typeof(ClassB2))]), "Execute_SelectedFalseOrStatements_ReturnsFailedTypes");
            o.Split("Execute_SelectedFalseOrStatements_ReturnsFailedTypes", types.Select(Oracle.Name), selected,
                s => Expr.All(Nat.ResideInNamespace(s, NameMatching), Nat.ResideInNamespace(s, NameMatching + ".Namespace3", negated: true)),
                s => Expr.Any(Nat.HaveNameStartingWith(s, "ClassA"), Nat.HaveName(s, "ClassB1")),
                "sequence.AddFunctionCall(FunctionDelegates.HaveNameStartingWith, \"ClassA\", true); sequence.CreateGroup(); sequence.AddFunctionCall(FunctionDelegates.HaveName, \"ClassB1\", true); sequence.Execute(Types.InAssembly(Assembly.GetAssembly(typeof(ClassB2))).That().ResideInNamespace(\"NetArchTest.TestStructure.NameMatching\").And().DoNotResideInNamespace(\"NetArchTest.TestStructure.NameMatching.Namespace3\").GetTypeDefinitions(), selected: false)");
        }
    }

    private static void Assert(bool holds, string test)
    {
        if (!holds)
        {
            throw new InvalidOperationException($"{test}: the upstream assertion does not hold over the fixture");
        }
    }
}
