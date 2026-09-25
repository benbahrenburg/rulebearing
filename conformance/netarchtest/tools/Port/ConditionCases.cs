// NetArchTest 1.3.2 test/NetArchTest.Rules.UnitTests/ConditionTests.cs, test by test: the upstream
// chain (verbatim), the predicates before Should() (the selection), and the element rule whose
// `where` is the selection and whose `should` is the condition.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7.
// The chains are upstream's C#, which is not nullable-annotated (Assembly.GetAssembly returns a
// nullable reference), so nullable analysis is off for this transcription only.
#nullable disable
using System.Reflection;
using System.Text.Json.Nodes;
using NetArchTest.Rules;
using NetArchTest.TestStructure.Classes;
using NetArchTest.TestStructure.CustomAttributes;
using NetArchTest.TestStructure.Dependencies.Examples;
using NetArchTest.TestStructure.Dependencies.Implementation;
using NetArchTest.TestStructure.Inheritance;
using NetArchTest.TestStructure.Interfaces;
using NetArchTest.TestStructure.NameMatching.Namespace1;
using NetArchTest.TestStructure.Nested;
using static NetArchTest.TestStructure.Nested.NestedPublic;

namespace Rulebearing.Conformance.NetArchTest;

/// <summary>ConditionTests.</summary>
internal static class ConditionCases
{
    private const string NameMatching = "NetArchTest.TestStructure.NameMatching";

    private static Assembly Structure => Assembly.GetAssembly(typeof(ClassA1));

    private static Predicates That() => Types.InAssembly(Structure).That();

    private static Func<Side, JsonNode> Ns(string ns) => s => Nat.ResideInNamespace(s, ns);

    private static Func<Side, JsonNode> Both(Func<Side, JsonNode> first, Func<Side, JsonNode> second) =>
        s => Expr.All(first(s), second(s));

    /// <summary>Registers every ConditionTests case.</summary>
    internal static void Register(Oracle o)
    {
        o.Class("ConditionTests");

        o.Condition("HaveName_MatchFound_ClassesSelected",
            () => That().ResideInNamespace(NameMatching + ".Namespace2.Namespace3").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace2.Namespace3").Should().HaveName("ClassB2").GetResult(),
            Ns(NameMatching + ".Namespace2.Namespace3"), s => Nat.HaveName(s, "ClassB2"), true);

        o.Condition("NotHaveName_MatchFound_ClassesSelected",
            () => That().ResideInNamespace(NameMatching + ".Namespace1").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace1").Should().NotHaveName("ClassB2").GetResult(),
            Ns(NameMatching + ".Namespace1"), s => Nat.HaveName(s, "ClassB2", negated: true), true);

        var notNamespace3 = Both(Ns(NameMatching), s => Nat.ResideInNamespace(s, NameMatching + ".Namespace3", negated: true));
        IEnumerable<Type> NotNamespace3() => That().ResideInNamespace(NameMatching).And().DoNotResideInNamespace(NameMatching + ".Namespace3").GetTypes();

        o.Condition("HaveNameStarting_MatchesFound_ClassesSelected",
            NotNamespace3,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().DoNotResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace3").Should().HaveNameStartingWith("Class").GetResult(),
            notNamespace3, s => Nat.HaveNameStartingWith(s, "Class"), true);

        o.Condition("HaveNameStarting_UsingStringComparison_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(NameMatching + ".Namespace3").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace3").Should().HaveNameStartingWith("Some", StringComparison.Ordinal).GetResult(),
            Ns(NameMatching + ".Namespace3"), s => Nat.HaveNameStartingWith(s, "Some", ordinal: true), true);

        o.Condition("NotHaveNameStarting_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(NameMatching).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").Should().NotHaveNameStartingWith("X").GetResult(),
            Ns(NameMatching), s => Nat.HaveNameStartingWith(s, "X", negated: true), true);

        o.Condition("NotHaveNameStarting_UsingStringComparison_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(NameMatching).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").Should().NotHaveNameStartingWith("s", StringComparison.Ordinal).GetResult(),
            Ns(NameMatching), s => Nat.HaveNameStartingWith(s, "s", negated: true, ordinal: true), true);

        o.Condition("HaveNameEnding_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(NameMatching + ".Namespace2.Namespace3").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace2.Namespace3").Should().HaveNameEndingWith("B2").GetResult(),
            Ns(NameMatching + ".Namespace2.Namespace3"), s => Nat.HaveNameEndingWith(s, "B2"), true);

        o.Condition("HaveNameEnding_UsingStringComparison_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(NameMatching + ".Namespace2.Namespace3.B").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace2.Namespace3.B").Should().HaveNameEndingWith("ntity").GetResult(),
            Ns(NameMatching + ".Namespace2.Namespace3.B"), s => Nat.HaveNameEndingWith(s, "ntity"), true);

        o.Condition("NotHaveNameEnding_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(NameMatching + ".Namespace2.Namespace1").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace2.Namespace1").Should().NotHaveNameEndingWith("B2").GetResult(),
            Ns(NameMatching + ".Namespace2.Namespace1"), s => Nat.HaveNameEndingWith(s, "B2", negated: true), true);

        o.Condition("NotHaveNameEnding_UsingStringComparison_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(NameMatching + ".Namespace3").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace3").Should().NotHaveNameEndingWith("ENTITY", StringComparison.Ordinal).GetResult(),
            Ns(NameMatching + ".Namespace3"), s => Nat.HaveNameEndingWith(s, "ENTITY", negated: true, ordinal: true), true);

        o.Condition("HaveNameMatching_MatchesFound_ClassesSelected",
            NotNamespace3,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().DoNotResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace3").Should().HaveNameMatching(@"Class\w\d").GetResult(),
            notNamespace3, s => Nat.HaveNameMatching(s, @"Class\w\d"), true);

        o.Condition("NotHaveNameMatching_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(NameMatching).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").Should().NotHaveNameMatching(@"X\w").GetResult(),
            Ns(NameMatching), s => Nat.HaveNameMatching(s, @"X\w", negated: true), true);

        const string Attributes = "NetArchTest.TestStructure.CustomAttributes";
        o.Condition("HaveCustomAttribute_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Attributes).And().HaveName("AttributePresent").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.CustomAttributes").And().HaveName("AttributePresent").Should().HaveCustomAttribute(typeof(ClassCustomAttribute)).GetResult(),
            Both(Ns(Attributes), s => Nat.HaveName(s, "AttributePresent")), s => Nat.HaveCustomAttribute(s, typeof(ClassCustomAttribute)), true);

        o.Condition("NotHaveCustomAttribute_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Attributes).And().DoNotHaveName("AttributePresent").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.CustomAttributes").And().DoNotHaveName("AttributePresent").Should().NotHaveCustomAttribute(typeof(ClassCustomAttribute)).GetResult(),
            Both(Ns(Attributes), s => Nat.HaveName(s, "AttributePresent", negated: true)), s => Nat.HaveCustomAttribute(s, typeof(ClassCustomAttribute), negated: true), true);

        o.Condition("HaveInheritCustomAttribute_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Attributes).And().HaveName("InheritAttributePresent").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.CustomAttributes").And().HaveName("InheritAttributePresent").Should().HaveCustomAttributeOrInherit(typeof(ClassCustomAttribute)).GetResult(),
            Both(Ns(Attributes), s => Nat.HaveName(s, "InheritAttributePresent")), s => Nat.HaveCustomAttributeOrInherit(s, typeof(ClassCustomAttribute)), true);

        o.Condition("NotHaveInheritCustomAttribute_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Attributes).And().DoNotHaveNameEndingWith("AttributePresent").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.CustomAttributes").And().DoNotHaveNameEndingWith("AttributePresent").Should().NotHaveCustomAttributeOrInherit(typeof(ClassCustomAttribute)).GetResult(),
            Both(Ns(Attributes), s => Nat.HaveNameEndingWith(s, "AttributePresent", negated: true)), s => Nat.HaveCustomAttributeOrInherit(s, typeof(ClassCustomAttribute), negated: true), true);

        const string Inheritance = "NetArchTest.TestStructure.Inheritance";
        o.Condition("Inherit_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Inheritance).And().HaveNameStartingWith("Derived").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Inheritance").And().HaveNameStartingWith("Derived").Should().Inherit(typeof(BaseClass)).GetResult(),
            Both(Ns(Inheritance), s => Nat.HaveNameStartingWith(s, "Derived")), s => Nat.Inherit(s, typeof(BaseClass)), true);

        o.Condition("NotInherit_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Inheritance).And().DoNotHaveNameStartingWith("Derived").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Inheritance").And().DoNotHaveNameStartingWith("Derived").Should().NotInherit(typeof(BaseClass)).GetResult(),
            Both(Ns(Inheritance), s => Nat.HaveNameStartingWith(s, "Derived", negated: true)), s => Nat.Inherit(s, typeof(BaseClass), negated: true), true);

        const string Interfaces = "NetArchTest.TestStructure.Interfaces";
        o.Condition("ImplementInterface_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Interfaces).And().HaveNameStartingWith("Implements").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Interfaces").And().HaveNameStartingWith("Implements").Should().ImplementInterface(typeof(IExample)).GetResult(),
            Both(Ns(Interfaces), s => Nat.HaveNameStartingWith(s, "Implements")), s => Nat.ImplementInterface(s, typeof(IExample)), true);

        o.Condition("NotImplementInterface_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Interfaces).And().DoNotHaveNameStartingWith("Implements").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Interfaces").And().DoNotHaveNameStartingWith("Implements").Should().NotImplementInterface(typeof(IExample)).GetResult(),
            Both(Ns(Interfaces), s => Nat.HaveNameStartingWith(s, "Implements", negated: true)), s => Nat.ImplementInterface(s, typeof(IExample), negated: true), true);

        const string Abstract = "NetArchTest.TestStructure.Abstract";
        o.Condition("AreAbstract_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Abstract).And().HaveNameStartingWith("Abstract").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Abstract").And().HaveNameStartingWith("Abstract").Should().BeAbstract().GetResult(),
            Both(Ns(Abstract), s => Nat.HaveNameStartingWith(s, "Abstract")), s => Nat.BeAbstract(s), true);

        o.Condition("AreNotAbstract_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Abstract).And().DoNotHaveNameStartingWith("Abstract").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Abstract").And().DoNotHaveNameStartingWith("Abstract").Should().NotBeAbstract().GetResult(),
            Both(Ns(Abstract), s => Nat.HaveNameStartingWith(s, "Abstract", negated: true)), s => Nat.BeAbstract(s, negated: true), true);

        const string Classes = "NetArchTest.TestStructure.Classes";
        o.Condition("AreClasses_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Classes).And().HaveNameEndingWith("Class").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Classes").And().HaveNameEndingWith("Class").Should().BeClasses().GetResult(),
            Both(Ns(Classes), s => Nat.HaveNameEndingWith(s, "Class")), s => Nat.BeClass(s), true);

        o.Condition("AreNotClasses_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Classes).And().HaveNameEndingWith("Interface").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Classes").And().HaveNameEndingWith("Interface").Should().NotBeClasses().GetResult(),
            Both(Ns(Classes), s => Nat.HaveNameEndingWith(s, "Interface")), s => Nat.BeClass(s, negated: true), true);

        const string Generic = "NetArchTest.TestStructure.Generic";
        o.Condition("AreGeneric_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Generic).And().HaveNameStartingWith("Generic").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Generic").And().HaveNameStartingWith("Generic").Should().BeGeneric().GetResult(),
            Both(Ns(Generic), s => Nat.HaveNameStartingWith(s, "Generic")), s => Nat.BeGeneric(s), true);

        o.Condition("AreNotGeneric_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Generic).And().HaveNameStartingWith("NonGeneric").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Generic").And().HaveNameStartingWith("NonGeneric").Should().NotBeGeneric().GetResult(),
            Both(Ns(Generic), s => Nat.HaveNameStartingWith(s, "NonGeneric")), s => Nat.BeGeneric(s, negated: true), true);

        o.Condition("AreInterfaces_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Classes).And().HaveNameEndingWith("Interface").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Classes").And().HaveNameEndingWith("Interface").Should().BeInterfaces().GetResult(),
            Both(Ns(Classes), s => Nat.HaveNameEndingWith(s, "Interface")), s => Nat.BeInterface(s), true);

        o.Condition("AreNotInterfaces_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Classes).And().HaveNameEndingWith("Class").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Classes").And().HaveNameEndingWith("Class").Should().NotBeInterfaces().GetResult(),
            Both(Ns(Classes), s => Nat.HaveNameEndingWith(s, "Class")), s => Nat.BeInterface(s, negated: true), true);

        o.Condition("AreStatic_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Classes).And().HaveNameEndingWith("StaticClass").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Classes").And().HaveNameEndingWith("StaticClass").Should().BeStatic().GetResult(),
            Both(Ns(Classes), s => Nat.HaveNameEndingWith(s, "StaticClass")), s => Nat.BeStatic(s), true);

        o.Condition("AreNotStatic_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Classes).And().HaveName(nameof(ExampleClass)).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Classes").And().HaveName(nameof(ExampleClass)).Should().NotBeStatic().GetResult(),
            Both(Ns(Classes), s => Nat.HaveName(s, nameof(ExampleClass))), s => Nat.BeStatic(s, negated: true), true);

        const string Nested = "NetArchTest.TestStructure.Nested";
        o.Condition("AreNested_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Nested).And().HaveNameEndingWith("Class").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Nested").And().HaveNameEndingWith("Class").Should().BeNested().GetResult(),
            Both(Ns(Nested), s => Nat.HaveNameEndingWith(s, "Class")), s => Nat.BeNested(s), true);

        o.Condition("AreNestedPublic_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Nested).And().HaveName(typeof(NestedPublicClass).Name).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Nested").And().HaveName(typeof(NestedPublicClass).Name).Should().BeNestedPublic().GetResult(),
            Both(Ns(Nested), s => Nat.HaveName(s, nameof(NestedPublicClass))), s => Nat.BeNestedPublic(s), true);

        o.Condition("AreNestedPrivate_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Nested).And().HaveName("NestedPrivateClass").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Nested").And().HaveName("NestedPrivateClass").Should().BeNestedPrivate().GetResult(),
            Both(Ns(Nested), s => Nat.HaveName(s, "NestedPrivateClass")), s => Nat.BeNestedPrivate(s), true);

        o.Condition("AreNotNested_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Nested).And().HaveName(typeof(NotNested).Name).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Nested").And().HaveName(typeof(NotNested).Name).Should().NotBeNested().GetResult(),
            Both(Ns(Nested), s => Nat.HaveName(s, nameof(NotNested))), s => Nat.BeNested(s, negated: true), true);

        o.Condition("AreNotNestedPublic_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Nested).And().HaveNameStartingWith("NestedPrivate").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Nested").And().HaveNameStartingWith("NestedPrivate").Should().NotBeNestedPublic().GetResult(),
            Both(Ns(Nested), s => Nat.HaveNameStartingWith(s, "NestedPrivate")), s => Nat.BeNestedPublic(s, negated: true), true);

        o.Condition("AreNotNestedPrivate_MatchesFound_ClassesSelected",
            () => That().ResideInNamespace(Nested).And().HaveNameStartingWith("NestedPublic").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Nested").And().HaveNameStartingWith("NestedPublic").Should().NotBeNestedPrivate().GetResult(),
            Both(Ns(Nested), s => Nat.HaveNameStartingWith(s, "NestedPublic")), s => Nat.BeNestedPrivate(s, negated: true), true);

        const string Scope = "NetArchTest.TestStructure.Scope";
        o.Condition("ArePublic_MatchesFound_ClassSelected",
            () => That().ResideInNamespace(Scope).And().HaveName("PublicClass").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Scope").And().HaveName("PublicClass").Should().BePublic().GetResult(),
            Both(Ns(Scope), s => Nat.HaveName(s, "PublicClass")), s => Nat.BePublic(s), true);

        o.Condition("AreNotPublic_MatchesFound_ClassSelected",
            () => That().ResideInNamespace(Scope).And().DoNotHaveNameStartingWith("PublicClass").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Scope").And().DoNotHaveNameStartingWith("PublicClass").Should().NotBePublic().GetResult(),
            Both(Ns(Scope), s => Nat.HaveNameStartingWith(s, "PublicClass", negated: true)), s => Nat.BePublic(s, negated: true), true);

        o.Condition("AreSealed_MatchesFound_ClassSelected",
            () => That().ResideInNamespace(Scope).And().HaveName("SealedClass").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Scope").And().HaveName("SealedClass").Should().BeSealed().GetResult(),
            Both(Ns(Scope), s => Nat.HaveName(s, "SealedClass")), s => Nat.BeSealed(s), true);

        o.Condition("AreNotSealed_MatchesFound_ClassSelected",
            () => That().ResideInNamespace(Scope).And().DoNotHaveName("SealedClass").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Scope").And().DoNotHaveName("SealedClass").Should().NotBeSealed().GetResult(),
            Both(Ns(Scope), s => Nat.HaveName(s, "SealedClass", negated: true)), s => Nat.BeSealed(s, negated: true), true);

        o.Unported("AreImmutable_MatchesFound_ClassSelected", null, Gaps.Immutable);
        o.Unported("AreMutable_MatchesFound_ClassSelected", null, Gaps.Immutable);
        o.Unported("AreNullable_MatchesFound_ClassSelected", null, Gaps.Nullable);
        o.Unported("AreNonNullable_MatchesFound_ClassSelected", null, Gaps.Nullable);

        IEnumerable<Type> ClassA() => That().HaveNameStartingWith("ClassA").GetTypes();
        Func<Side, JsonNode> classA = s => Nat.HaveNameStartingWith(s, "ClassA");

        o.Condition("ResideInNamespace_MatchesFound_ClassSelected",
            ClassA,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().HaveNameStartingWith("ClassA").Should().ResideInNamespace("NetArchTest.TestStructure.NameMatching").GetResult(),
            classA, Ns(NameMatching), true);

        o.Condition("NotResideInNamespace_MatchesFound_ClassSelected",
            ClassA,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().HaveNameStartingWith("ClassA").Should().NotResideInNamespace("NetArchTest.TestStructure.Wrong").GetResult(),
            classA, s => Nat.ResideInNamespace(s, "NetArchTest.TestStructure.Wrong", negated: true), true);

        const string NamespaceMatching = "NetArchTest.TestStructure.NamespaceMatching";
        o.Condition("ResideInNamespaceMatching_MatchesFound_ClassSelected",
            () => That().ResideInNamespace(NamespaceMatching).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(@"NetArchTest.TestStructure.NamespaceMatching").Should().ResideInNamespaceMatching(@"NetArchTest.TestStructure.NamespaceMatching.Namespace\w").GetResult(),
            Ns(NamespaceMatching), s => Nat.ResideInNamespaceMatching(s, @"NetArchTest.TestStructure.NamespaceMatching.Namespace\w"), true);

        o.Condition("NotResideInNamespaceMatching_MatchesFound_ClassSelected",
            () => That().ResideInNamespace(NamespaceMatching + ".NamespaceA").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NamespaceMatching.NamespaceA").Should().NotResideInNamespaceMatching(@"NetArchTest.TestStructure.NamespaceMatching.Namespace\d").GetResult(),
            Ns(NamespaceMatching + ".NamespaceA"), s => Nat.ResideInNamespaceMatching(s, @"NetArchTest.TestStructure.NamespaceMatching.Namespace\d", negated: true), true);

        o.Condition("ResideInNamespaceStartingWith_ClassSelected",
            ClassA,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().HaveNameStartingWith("ClassA").Should().ResideInNamespaceStartingWith("NetArchTest.TestStructure.NameMatching").GetResult(),
            classA, s => Nat.ResideInNamespaceStartingWith(s, NameMatching), true);

        o.Condition("NotResideInNamespaceStartingWith_ClassSelected",
            () => That().ResideInNamespaceStartingWith(NameMatching).And().HaveNameEndingWith("1").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespaceStartingWith("NetArchTest.TestStructure.NameMatching").And().HaveNameEndingWith("1").Should().NotResideInNamespaceStartingWith("NetArchTest.TestStructure.NameMatching.Namespace2").GetResult(),
            Both(s => Nat.ResideInNamespaceStartingWith(s, NameMatching), s => Nat.HaveNameEndingWith(s, "1")),
            s => Nat.ResideInNamespaceStartingWith(s, NameMatching + ".Namespace2", negated: true), true);

        o.Condition("ResideInNamespaceEndingWith_ClassSelected",
            () => That().HaveName("ClassA1").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().HaveName("ClassA1").Should().ResideInNamespaceEndingWith(".NameMatching.Namespace1").GetResult(),
            s => Nat.HaveName(s, "ClassA1"), s => Nat.ResideInNamespaceEndingWith(s, ".NameMatching.Namespace1"), true);

        o.Condition("NotResideInNamespaceEndingWith_ClassSelected",
            ClassA,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().HaveNameStartingWith("ClassA").Should().NotResideInNamespaceEndingWith(".Namespace3").GetResult(),
            classA, s => Nat.ResideInNamespaceEndingWith(s, ".Namespace3", negated: true), true);

        o.Condition("ResideInNamespaceContaining_ClassSelected",
            ClassA,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().HaveNameStartingWith("ClassA").Should().ResideInNamespaceContaining(".NameMatching.").GetResult(),
            classA, s => Nat.ResideInNamespaceContaining(s, ".NameMatching."), true);

        o.Condition("NotResideInNamespaceContaining_ClassSelected",
            ClassA,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().HaveNameStartingWith("ClassA").Should().NotResideInNamespaceContaining("Namespace3").GetResult(),
            classA, s => Nat.ResideInNamespaceContaining(s, "Namespace3", negated: true), true);

        o.Condition("ResideInNamespace_Nested_AllClassReturned",
            () => That().HaveName("ClassB2").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().HaveName("ClassB2").Should().ResideInNamespace("NetArchTest.TestStructure.NameMatching").GetResult(),
            s => Nat.HaveName(s, "ClassB2"), Ns(NameMatching), true);

        var implementation = typeof(HasDependency).Namespace;
        var example = typeof(ExampleDependency).FullName;
        var another = typeof(AnotherExampleDependency).FullName;
        Func<Side, JsonNode> Starting(string start) => Both(Ns(implementation), s => Nat.HaveNameStartingWith(s, start));
        IEnumerable<Type> Selected(string start) => That().ResideInNamespace(implementation).And().HaveNameStartingWith(start).GetTypes();

        o.Condition("HaveDependency_MatchesFound_ClassSelected",
            () => Selected("HasDepend"),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().HaveNameStartingWith("HasDepend").Should().HaveDependencyOn(typeof(ExampleDependency).FullName).GetResult(),
            Starting("HasDepend"), s => Nat.HaveDependencyOnAny(s, [example]), true);

        o.Condition("HaveDependencyOnAny_MatchesFound_ClassSelected",
            () => Selected("Has"),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().HaveNameStartingWith("Has").Should().HaveDependencyOnAny(new[] { typeof(ExampleDependency).FullName, typeof(AnotherExampleDependency).FullName }).GetResult(),
            Starting("Has"), s => Nat.HaveDependencyOnAny(s, [example, another]), true);

        o.Condition("HaveDependencyOnAll_MatchesFound_ClassSelected",
            () => Selected("HasDependencies"),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().HaveNameStartingWith("HasDependencies").Should().HaveDependencyOnAll(new[] { typeof(ExampleDependency).FullName, typeof(AnotherExampleDependency).FullName }).GetResult(),
            Starting("HasDependencies"), s => Nat.HaveDependencyOnAll(s, [example, another]), true);

        o.Condition("OnlyHaveDependenciesOn_MatchesFound_ClassSelected",
            () => Selected("HasDependency"),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().HaveNameStartingWith("HasDependency").Should().OnlyHaveDependenciesOn(new[] { typeof(ExampleDependency).FullName, "System" }).GetResult(),
            Starting("HasDependency"), s => Nat.OnlyHaveDependenciesOn(s, [example, "System"]), true, note: Gaps.SystemNote,
            reason: Gaps.SelfReference("HasDependency", "its dependency property's getter reads its own backing field"));

        o.Condition("NotHaveDependency_MatchesFound_ClassSelected",
            () => Selected("NoDependency"),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().HaveNameStartingWith("NoDependency").Should().NotHaveDependencyOn(typeof(ExampleDependency).FullName).GetResult(),
            Starting("NoDependency"), s => Nat.HaveDependencyOnAny(s, [example], negated: true), true);

        o.Condition("NotHaveDependencyOnAny_MatchesFound_ClassSelected",
            () => Selected("NoDependency"),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().HaveNameStartingWith("NoDependency").Should().NotHaveDependencyOnAny(new[] { typeof(ExampleDependency).FullName, typeof(AnotherExampleDependency).FullName }).GetResult(),
            Starting("NoDependency"), s => Nat.HaveDependencyOnAny(s, [example, another], negated: true), true);

        o.Condition("NotHaveDependencyOnAll_MatchesFound_ClassSelected",
            () => Selected("NoDependency"),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().HaveNameStartingWith("NoDependency").Should().NotHaveDependencyOnAll(new[] { typeof(ExampleDependency).FullName, typeof(AnotherExampleDependency).FullName }).GetResult(),
            Starting("NoDependency"), s => Nat.HaveDependencyOnAll(s, [example, another], negated: true), true);

        o.Condition("HaveDependenciesOtherThan_MatchesFound_ClassSelected",
            () => Selected("HasDependencies"),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().HaveNameStartingWith("HasDependencies").Should().HaveDependenciesOtherThan(new[] { typeof(ExampleDependency).FullName, "System" }).GetResult(),
            Starting("HasDependencies"), s => Nat.OnlyHaveDependenciesOn(s, [example, "System"], negated: true), true, note: Gaps.SystemNote);

        o.Condition("MatchNotFound_ClassesReported",
            () => That().ResideInNamespace(NameMatching + ".Namespace1").GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace1").Should().HaveName("ClassA2").GetResult(),
            Ns(NameMatching + ".Namespace1"), s => Nat.HaveName(s, "ClassA2"), false, [typeof(ClassA1), typeof(ClassB1)]);

        o.Unported("MeetCustomRule_MatchesFound_ClassSelected", null, "custom-predicate");
    }
}
