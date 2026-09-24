// NetArchTest 1.3.2 test/NetArchTest.Rules.UnitTests/PredicateTests.cs, test by test: the upstream
// chain (verbatim), the same chain without the predicate under test (the selection), and the
// element rule whose `where` is the selection and whose `should` is the predicate under test.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7.
// The chains are upstream's C#, which is not nullable-annotated (Assembly.GetAssembly returns a
// nullable reference), so nullable analysis is off for this transcription only.
#nullable disable
using System.Reflection;
using System.Text.Json.Nodes;
using NetArchTest.CrossAssemblyTest.A;
using NetArchTest.CrossAssemblyTest.B;
using NetArchTest.Rules;
using NetArchTest.TestStructure.Abstract;
using NetArchTest.TestStructure.Classes;
using NetArchTest.TestStructure.CustomAttributes;
using NetArchTest.TestStructure.Dependencies.Examples;
using NetArchTest.TestStructure.Dependencies.Implementation;
using NetArchTest.TestStructure.Generic;
using NetArchTest.TestStructure.Inheritance;
using NetArchTest.TestStructure.Interfaces;
using NetArchTest.TestStructure.NameMatching.Namespace1;
using NetArchTest.TestStructure.NameMatching.Namespace2;
using NetArchTest.TestStructure.NameMatching.Namespace2.Namespace3;
using NetArchTest.TestStructure.NameMatching.Namespace3.A;
using NetArchTest.TestStructure.NameMatching.Namespace3.B;
using NetArchTest.TestStructure.NamespaceMatching.Namespace1;
using NetArchTest.TestStructure.NamespaceMatching.NamespaceA;
using NetArchTest.TestStructure.Nested;
using NetArchTest.TestStructure.Scope;
using NetArchTest.TestStructure.Sealed;

namespace Rulebearing.Conformance.NetArchTest;

/// <summary>PredicateTests.</summary>
internal static class PredicateCases
{
    private const string NameMatching = "NetArchTest.TestStructure.NameMatching";

    private static Assembly Structure => Assembly.GetAssembly(typeof(ClassA1));

    private static IEnumerable<Type> In(string ns) => Types.InAssembly(Structure).That().ResideInNamespace(ns).GetTypes();

    private static Func<Side, JsonNode> Ns(string ns) => s => Nat.ResideInNamespace(s, ns);

    /// <summary>Registers every PredicateTests case.</summary>
    internal static void Register(Oracle o)
    {
        o.Class("PredicateTests");

        o.Predicate("HaveName_MatchFound_ClassesSelected",
            () => In(NameMatching),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().HaveName("ClassA1").GetTypes(),
            Ns(NameMatching), s => Nat.HaveName(s, "ClassA1"), 1, [typeof(ClassA1)]);

        o.Predicate("DoNotHaveName_MatchFound_ClassesSelected",
            () => In(NameMatching),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().DoNotHaveName("ClassA1").GetTypes(),
            Ns(NameMatching), s => Nat.HaveName(s, "ClassA1", negated: true), 8,
            [typeof(ClassA2), typeof(ClassB1), typeof(ClassB2), typeof(ClassA3), typeof(SomeThing), typeof(SomethingElse), typeof(SomeEntity), typeof(SomeIdentity)]);

        o.Predicate("HaveNameStarting_MatchesFound_ClassesSelected",
            () => In(NameMatching),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().HaveNameStartingWith("SomeT").GetTypes(),
            Ns(NameMatching), s => Nat.HaveNameStartingWith(s, "SomeT"), 2, [typeof(SomeThing), typeof(SomethingElse)]);

        o.Predicate("HaveNameStarting_UsingExplicitStringComparison_MatchesFound_ClassesSelected",
            () => In(NameMatching),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(SomeThing))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().HaveNameStartingWith("SomeT", StringComparison.Ordinal).GetTypes(),
            Ns(NameMatching), s => Nat.HaveNameStartingWith(s, "SomeT", ordinal: true), 1, [typeof(SomeThing)]);

        o.Predicate("DoNotHaveNameStarting_MatchesFound_ClassesSelected",
            () => In(NameMatching),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().DoNotHaveNameStartingWith("ClassA").GetTypes(),
            Ns(NameMatching), s => Nat.HaveNameStartingWith(s, "ClassA", negated: true), 6,
            [typeof(ClassB1), typeof(ClassB2), typeof(SomeThing), typeof(SomethingElse), typeof(SomeEntity), typeof(SomeIdentity)]);

        o.Predicate("DoNotHaveNameStarting_UsingExplicitStringComparison_MatchesFound_ClassesSelected",
            () => In(NameMatching + ".Namespace3"),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(SomeThing))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace3").And().DoNotHaveNameStartingWith("SomeT", StringComparison.Ordinal).GetTypes(),
            Ns(NameMatching + ".Namespace3"), s => Nat.HaveNameStartingWith(s, "SomeT", negated: true, ordinal: true), 3,
            [typeof(SomethingElse), typeof(SomeEntity), typeof(SomeIdentity)]);

        o.Predicate("HaveNameEnding_MatchesFound_ClassesSelected",
            () => In(NameMatching),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().HaveNameEndingWith("Entity").GetTypes(),
            Ns(NameMatching), s => Nat.HaveNameEndingWith(s, "Entity"), 2, [typeof(SomeEntity), typeof(SomeIdentity)]);

        o.Predicate("HaveNameEnding_UsingExplicitStringComparison_MatchesFound_ClassesSelected",
            () => In(NameMatching),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().HaveNameEndingWith("Entity", StringComparison.Ordinal).GetTypes(),
            Ns(NameMatching), s => Nat.HaveNameEndingWith(s, "Entity", ordinal: true), 1, [typeof(SomeEntity)]);

        o.Predicate("DoNotHaveNameEnding_MatchesFound_ClassesSelected",
            () => In(NameMatching),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().DoNotHaveNameEndingWith("A1").GetTypes(),
            Ns(NameMatching), s => Nat.HaveNameEndingWith(s, "A1", negated: true), 8,
            [typeof(ClassA2), typeof(ClassA3), typeof(ClassB1), typeof(ClassB2), typeof(SomeThing), typeof(SomethingElse), typeof(SomeEntity), typeof(SomeIdentity)]);

        o.Predicate("HaveNameMatching_MatchesFound_ClassesSelected",
            () => In(NameMatching),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().HaveNameMatching(@"Class\w1").GetTypes(),
            Ns(NameMatching), s => Nat.HaveNameMatching(s, @"Class\w1"), 2, [typeof(ClassA1), typeof(ClassB1)]);

        o.Predicate("DoNotHaveNameMatching_MatchesFound_ClassesSelected",
            () => In(NameMatching),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().DoNotHaveNameMatching(@"Class\w1").GetTypes(),
            Ns(NameMatching), s => Nat.HaveNameMatching(s, @"Class\w1", negated: true), 7,
            [typeof(ClassA2), typeof(ClassA3), typeof(ClassB2), typeof(SomeThing), typeof(SomethingElse), typeof(SomeEntity), typeof(SomeIdentity)]);

        const string Attributes = "NetArchTest.TestStructure.CustomAttributes";
        o.Predicate("HaveCustomAttribute_MatchesFound_ClassesSelected",
            () => In(Attributes),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.CustomAttributes").And().HaveCustomAttribute(typeof(ClassCustomAttribute)).GetTypes(),
            Ns(Attributes), s => Nat.HaveCustomAttribute(s, typeof(ClassCustomAttribute)), 1, [typeof(AttributePresent)]);

        o.Predicate("DoNotHaveCustomAttribute_MatchesFound_ClassesSelected",
            () => In(Attributes),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.CustomAttributes").And().DoNotHaveCustomAttribute(typeof(ClassCustomAttribute)).GetTypes(),
            Ns(Attributes), s => Nat.HaveCustomAttribute(s, typeof(ClassCustomAttribute), negated: true), 4,
            [typeof(NoAttributes), typeof(ClassCustomAttribute), typeof(InheritAttributePresent), typeof(InheritClassCustomAttribute)]);

        o.Predicate("HaveInheritCustomAttribute_MatchesFound_ClassesSelected",
            () => In(Attributes),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.CustomAttributes").And().HaveCustomAttributeOrInherit(typeof(ClassCustomAttribute)).GetTypes(),
            Ns(Attributes), s => Nat.HaveCustomAttributeOrInherit(s, typeof(ClassCustomAttribute)), 2,
            [typeof(AttributePresent), typeof(InheritAttributePresent)]);

        o.Predicate("DoNotHaveInheritCustomAttribute_MatchesFound_ClassesSelected",
            () => In(Attributes),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.CustomAttributes").And().DoNotHaveCustomAttributeOrInherit(typeof(ClassCustomAttribute)).GetTypes(),
            Ns(Attributes), s => Nat.HaveCustomAttributeOrInherit(s, typeof(ClassCustomAttribute), negated: true), 3,
            [typeof(NoAttributes), typeof(ClassCustomAttribute), typeof(InheritClassCustomAttribute)]);

        const string Inheritance = "NetArchTest.TestStructure.Inheritance";
        o.Predicate("Inherit_MatchesFound_ClassesSelected",
            () => In(Inheritance),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Inheritance").And().Inherit(typeof(BaseClass)).GetTypes(),
            Ns(Inheritance), s => Nat.Inherit(s, typeof(BaseClass)), 2, [typeof(DerivedClass), typeof(DerivedDerivedClass)]);

        o.Predicate("Inherit_MatchesFound_ClassesSelected_AcrossAssemblies",
            () => Types.InAssembly(Assembly.GetAssembly(typeof(DerivedClassFromB))).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(DerivedClassFromB))).That().Inherit(typeof(BaseClassFromA)).GetTypes(),
            null, s => Nat.Inherit(s, typeof(BaseClassFromA)), 2, [typeof(DerivedClassFromB), typeof(AnotherDerivedClassFromB)],
            architecture: ["NetArchTest.CrossAssemblyTest.B"]);

        o.Predicate("DoNotInherit_MatchesFound_ClassesSelected",
            () => In(Inheritance),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Inheritance").And().DoNotInherit(typeof(BaseClass)).GetTypes(),
            Ns(Inheritance), s => Nat.Inherit(s, typeof(BaseClass), negated: true), 2, [typeof(BaseClass), typeof(NotDerivedClass)]);

        const string Interfaces = "NetArchTest.TestStructure.Interfaces";
        o.Predicate("ImplementInterface_MatchesFound_ClassesSelected",
            () => In(Interfaces),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Interfaces").And().ImplementInterface(typeof(IExample)).GetTypes(),
            Ns(Interfaces), s => Nat.ImplementInterface(s, typeof(IExample)), 1, [typeof(ImplementsExampleInterface)]);

        o.Predicate("DoNotImplementInterface_MatchesFound_ClassesSelected",
            () => In(Interfaces),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Interfaces").And().DoNotImplementInterface(typeof(IExample)).GetTypes(),
            Ns(Interfaces), s => Nat.ImplementInterface(s, typeof(IExample), negated: true), 2, [typeof(IExample), typeof(DoesNotImplementInterface)]);

        const string Abstract = "NetArchTest.TestStructure.Abstract";
        o.Predicate("AreAbstract_MatchesFound_ClassesSelected",
            () => In(Abstract),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Abstract").And().AreAbstract().GetTypes(),
            Ns(Abstract), s => Nat.BeAbstract(s), 1, [typeof(AbstractClass)]);

        o.Predicate("AreNotAbstract_MatchesFound_ClassesSelected",
            () => In(Abstract),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Abstract").And().AreNotAbstract().GetTypes(),
            Ns(Abstract), s => Nat.BeAbstract(s, negated: true), 1, [typeof(ConcreteClass)]);

        const string Classes = "NetArchTest.TestStructure.Classes";
        o.Predicate("AreClasses_MatchesFound_ClassesSelected",
            () => In(Classes),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Classes").And().AreClasses().GetTypes(),
            Ns(Classes), s => Nat.BeClass(s), 2, [typeof(ExampleClass), typeof(ExampleStaticClass)]);

        o.Predicate("AreNotClasses_MatchesFound_ClassesSelected",
            () => In(Classes),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Classes").And().AreNotClasses().GetTypes(),
            Ns(Classes), s => Nat.BeClass(s, negated: true), 1, [typeof(IExampleInterface)]);

        const string Generic = "NetArchTest.TestStructure.Generic";
        o.Predicate("AreGeneric_MatchesFound_ClassesSelected",
            () => In(Generic),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Generic").And().AreGeneric().GetTypes(),
            Ns(Generic), s => Nat.BeGeneric(s), 1, [typeof(GenericType<>)]);

        o.Predicate("AreNotGeneric_MatchesFound_ClassesSelected",
            () => In(Generic),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Generic").And().AreNotGeneric().GetTypes(),
            Ns(Generic), s => Nat.BeGeneric(s, negated: true), 1, [typeof(NonGenericType)]);

        o.Predicate("AreInterfaces_MatchesFound_ClassesSelected",
            () => In(Classes),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Classes").And().AreInterfaces().GetTypes(),
            Ns(Classes), s => Nat.BeInterface(s), 1, [typeof(IExampleInterface)]);

        o.Predicate("AreNotInterfaces_MatchesFound_ClassesSelected",
            () => In(Classes),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Classes").And().AreNotInterfaces().GetTypes(),
            Ns(Classes), s => Nat.BeInterface(s, negated: true), 2, [typeof(ExampleClass), typeof(ExampleStaticClass)]);

        o.Predicate("AreStatic_MatchesFound_ClassesSelected",
            () => In(Classes),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Classes").And().AreStatic().GetTypes(),
            Ns(Classes), s => Nat.BeStatic(s), 1, [typeof(ExampleStaticClass)]);

        o.Predicate("AreNotStatic_MatchesFound_ClassesSelected",
            () => In(Classes),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Classes").And().AreNotStatic().GetTypes(),
            Ns(Classes), s => Nat.BeStatic(s, negated: true), 2, [typeof(ExampleClass), typeof(IExampleInterface)]);

        const string Nested = "NetArchTest.TestStructure.Nested";
        o.Predicate("AreNested_MatchesFound_ClassesSelected",
            () => In(Nested),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Nested").And().AreNested().GetTypes(),
            Ns(Nested), s => Nat.BeNested(s), 2, [typeof(NestedPublic.NestedPublicClass)]);

        o.Predicate("AreNestedPublic_MatchesFound_ClassesSelected",
            () => In(Nested),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Nested").And().AreNestedPublic().GetTypes(),
            Ns(Nested), s => Nat.BeNestedPublic(s), 1, [typeof(NestedPublic.NestedPublicClass)]);

        o.Predicate("AreNestedPrivate_MatchesFound_ClassesSelected",
            () => In(Nested),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Nested").And().AreNestedPrivate().GetTypes(),
            Ns(Nested), s => Nat.BeNestedPrivate(s), 1);

        o.Predicate("AreNotNested_MatchesFound_ClassesSelected",
            () => In(Nested),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Nested").And().AreNotNested().GetTypes(),
            Ns(Nested), s => Nat.BeNested(s, negated: true), 3, [typeof(NestedPrivate), typeof(NestedPublic), typeof(NotNested)]);

        o.Predicate("AreNotNestedPublic_MatchesFound_ClassesSelected",
            () => In(Nested),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Nested").And().AreNotNestedPublic().GetTypes(),
            Ns(Nested), s => Nat.BeNestedPublic(s, negated: true), 4);

        o.Predicate("AreNotNestedPrivate_MatchesFound_ClassesSelected",
            () => In(Nested),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Nested").And().AreNotNestedPrivate().GetTypes(),
            Ns(Nested), s => Nat.BeNestedPrivate(s, negated: true), 4, [typeof(NestedPublic.NestedPublicClass)]);

        const string Scope = "NetArchTest.TestStructure.Scope";
        o.Predicate("ArePublic_MatchesFound_ClassSelected",
            () => In(Scope),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Scope").And().ArePublic().GetTypes(),
            Ns(Scope), s => Nat.BePublic(s), 2, [typeof(PublicClass), typeof(PublicClass.PublicClassInternal)]);

        o.Predicate("AreNotPublic_MatchesFound_ClassSelected",
            () => In(Scope),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Scope").And().AreNotPublic().GetTypes(),
            Ns(Scope), s => Nat.BePublic(s, negated: true), 2, [typeof(InternalClass), typeof(InternalClass.InternalClassNested)]);

        const string Sealed = "NetArchTest.TestStructure.Sealed";
        o.Predicate("AreSealed_MatchesFound_ClassSelected",
            () => In(Sealed),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Sealed").And().AreSealed().GetTypes(),
            Ns(Sealed), s => Nat.BeSealed(s), 1, [typeof(SealedClass)]);

        o.Predicate("AreNotSealed_MatchesFound_ClassSelected",
            () => In(Sealed),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.Sealed").And().AreNotSealed().GetTypes(),
            Ns(Sealed), s => Nat.BeSealed(s, negated: true), 1, [typeof(NotSealedClass)]);

        o.Unported("AreImmutable_MatchesFound_ClassSelected", null, Gaps.Immutable);
        o.Unported("AreMutable_MatchesFound_ClassSelected", null, Gaps.Immutable);
        o.Unported("AreNullable_MatchesFound_ClassSelected", null, Gaps.Nullable);
        o.Unported("AreNonNullable_MatchesFound_ClassSelected", null, Gaps.Nullable);

        o.Predicate("ResideInNamespace_MatchesFound_ClassSelected",
            () => Types.InAssembly(Structure).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace1").GetTypes(),
            null, s => Nat.ResideInNamespace(s, NameMatching + ".Namespace1"), 3, [typeof(ClassA1), typeof(ClassA2), typeof(ClassB1)]);

        o.Predicate("DoNotResideInNamespace_MatchesFound_ClassSelected",
            () => In(NameMatching),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().DoNotResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace2").GetTypes(),
            Ns(NameMatching), s => Nat.ResideInNamespace(s, NameMatching + ".Namespace2", negated: true), 7,
            [typeof(ClassA1), typeof(ClassA2), typeof(ClassB1), typeof(SomeThing), typeof(SomethingElse), typeof(SomeEntity), typeof(SomeIdentity)]);

        const string NamespaceMatching = "NetArchTest.TestStructure.NamespaceMatching";
        o.Predicate("ResideInNamespaceMatching_MatchesFound_ClassSelected",
            () => Types.InAssembly(Structure).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespaceMatching(@"NetArchTest.TestStructure.NamespaceMatching.Namespace\d").GetTypes(),
            null, s => Nat.ResideInNamespaceMatching(s, @"NetArchTest.TestStructure.NamespaceMatching.Namespace\d"), 1, [typeof(Match1)]);

        o.Predicate("DoNotResideInNamespaceMatching_MatchesFound_ClassSelected",
            () => In(NamespaceMatching),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NamespaceMatching").And().DoNotResideInNamespaceMatching(@"NetArchTest.TestStructure.NamespaceMatching.Namespace\d").GetTypes(),
            Ns(NamespaceMatching), s => Nat.ResideInNamespaceMatching(s, @"NetArchTest.TestStructure.NamespaceMatching.Namespace\d", negated: true), 1, [typeof(MatchA)]);

        IEnumerable<Type> StartingWith() => Types.InAssembly(Structure).That().ResideInNamespaceStartingWith(NameMatching).GetTypes();
        Func<Side, JsonNode> startingWith = s => Nat.ResideInNamespaceStartingWith(s, NameMatching);

        o.Predicate("ResideInNamespaceStartingWith_ClassSelected",
            () => Types.InAssembly(Structure).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespaceStartingWith("NetArchTest.TestStructure.NameMatching").GetTypes(),
            null, startingWith, 9,
            [typeof(ClassA1), typeof(ClassA2), typeof(ClassA3), typeof(ClassB2), typeof(SomeThing), typeof(SomethingElse), typeof(SomeEntity), typeof(SomeIdentity)]);

        o.Predicate("DoNotResideInNamespaceStartingWith_ClassSelected",
            StartingWith,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespaceStartingWith("NetArchTest.TestStructure.NameMatching").And().DoNotResideInNamespaceStartingWith("NetArchTest.TestStructure.NameMatching.Namespace2").GetTypes(),
            startingWith, s => Nat.ResideInNamespaceStartingWith(s, NameMatching + ".Namespace2", negated: true), 7,
            [typeof(ClassA1), typeof(ClassA2), typeof(ClassB1), typeof(SomeThing), typeof(SomethingElse), typeof(SomeEntity), typeof(SomeIdentity)]);

        o.Predicate("ResideInNamespaceEndingWith_ClassSelected",
            () => Types.InAssembly(Structure).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespaceEndingWith(".NameMatching.Namespace1").GetTypes(),
            null, s => Nat.ResideInNamespaceEndingWith(s, ".NameMatching.Namespace1"), 3, [typeof(ClassA1)]);

        // Upstream's test of that name repeats ResideInNamespaceStartingWith and asserts its nine types.
        o.Predicate("DoNotResideInNamespaceEndingWith_ClassSelected",
            () => Types.InAssembly(Structure).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespaceStartingWith("NetArchTest.TestStructure.NameMatching").GetTypes(),
            null, startingWith, 9,
            [typeof(ClassA1), typeof(ClassA2), typeof(ClassA3), typeof(ClassB1), typeof(ClassB2), typeof(SomeThing), typeof(SomethingElse), typeof(SomeEntity), typeof(SomeIdentity)]);

        o.Predicate("ResideInNamespaceContaining_ClassSelected",
            () => Types.InAssembly(Structure).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespaceContaining(".NameMatching.").GetTypes(),
            null, s => Nat.ResideInNamespaceContaining(s, ".NameMatching."), 9,
            [typeof(ClassA1), typeof(ClassA2), typeof(ClassA3), typeof(ClassB1), typeof(ClassB2), typeof(SomeThing), typeof(SomethingElse), typeof(SomeEntity), typeof(SomeIdentity)]);

        o.Predicate("ResideInNamespaceContaining_NestedClassSelected",
            () => Types.InAssembly(Structure).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespaceContaining("Nested").GetTypes(),
            null, s => Nat.ResideInNamespaceContaining(s, "Nested"), 19, [typeof(NestedPublic.NestedPublicClass)]);

        o.Predicate("DoNotResideInNamespaceContaining_ClassSelected",
            StartingWith,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespaceStartingWith("NetArchTest.TestStructure.NameMatching").And().DoNotResideInNamespaceContaining("Namespace2").GetTypes(),
            startingWith, s => Nat.ResideInNamespaceContaining(s, "Namespace2", negated: true), 7,
            [typeof(ClassA1), typeof(ClassA2), typeof(ClassB1), typeof(SomeThing), typeof(SomethingElse), typeof(SomeEntity), typeof(SomeIdentity)]);

        o.Predicate("ResideInNamespace_Nested_AllClassReturned",
            () => Types.InAssembly(Structure).GetTypes(),
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").GetTypes(),
            null, Ns(NameMatching), 9,
            [typeof(ClassA1), typeof(ClassA2), typeof(ClassA3), typeof(ClassB1), typeof(ClassB2), typeof(SomeThing), typeof(SomethingElse), typeof(SomeEntity), typeof(SomeIdentity)]);

        var implementation = typeof(HasDependency).Namespace;
        var example = typeof(ExampleDependency).FullName;
        var another = typeof(AnotherExampleDependency).FullName;
        IEnumerable<Type> Implementation() => In(implementation);
        Func<Side, JsonNode> inImplementation = s => Nat.ResideInNamespace(s, implementation);

        o.Predicate("HaveDepencency_MatchesFound_ClassSelected",
            Implementation,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().HaveDependencyOn(typeof(ExampleDependency).FullName).GetTypes(),
            inImplementation, s => Nat.HaveDependencyOnAny(s, [example]), 2, [typeof(HasDependencies), typeof(HasDependency)]);

        o.Predicate("HaveDepencencyOnAny_MatchesFound_ClassSelected",
            Implementation,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().HaveDependencyOnAny(new[] { typeof(ExampleDependency).FullName, typeof(AnotherExampleDependency).FullName }).GetTypes(),
            inImplementation, s => Nat.HaveDependencyOnAny(s, [example, another]), 3,
            [typeof(HasAnotherDependency), typeof(HasDependencies), typeof(HasDependency)]);

        o.Predicate("HaveDepencencyOnAll_MatchesFound_ClassSelected",
            Implementation,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().HaveDependencyOnAll(new[] { typeof(ExampleDependency).FullName, typeof(AnotherExampleDependency).FullName }).GetTypes(),
            inImplementation, s => Nat.HaveDependencyOnAll(s, [example, another]), 1, [typeof(HasDependencies)]);

        o.Predicate("OnlyHaveDependenciesOn_MatchesFound_ClassSelected",
            Implementation,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().OnlyHaveDependenciesOn(new[] { typeof(ExampleDependency).FullName, "System" }).GetTypes(),
            inImplementation, s => Nat.OnlyHaveDependenciesOn(s, [example, "System"]), 2,
            [typeof(HasDependency), typeof(NoDependency)], note: Gaps.SystemNote,
            reason: Gaps.SelfReference("HasDependency", "its dependency property's getter reads its own backing field"));

        o.Predicate("DoNotHaveDepencency_MatchesFound_ClassSelected",
            Implementation,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().DoNotHaveDependencyOn(typeof(ExampleDependency).FullName).GetTypes(),
            inImplementation, s => Nat.HaveDependencyOnAny(s, [example], negated: true), 2, [typeof(HasAnotherDependency), typeof(NoDependency)]);

        o.Predicate("DoNotHaveDependencyOnAny_MatchesFound_ClassSelected",
            Implementation,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().DoNotHaveDependencyOnAny(new[] { typeof(ExampleDependency).FullName, typeof(AnotherExampleDependency).FullName }).GetTypes(),
            inImplementation, s => Nat.HaveDependencyOnAny(s, [example, another], negated: true), 1, [typeof(NoDependency)]);

        o.Predicate("DoNotHaveDependencyOnAll_MatchesFound_ClassSelected",
            Implementation,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().DoNotHaveDependencyOnAll(new[] { typeof(ExampleDependency).FullName, typeof(AnotherExampleDependency).FullName }).GetTypes(),
            inImplementation, s => Nat.HaveDependencyOnAll(s, [example, another], negated: true), 3,
            [typeof(HasAnotherDependency), typeof(HasDependency), typeof(NoDependency)]);

        o.Predicate("HaveDependenciesOtherThan_MatchesFound_ClassSelected",
            Implementation,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace(typeof(HasDependency).Namespace).And().HaveDependenciesOtherThan(new[] { typeof(ExampleDependency).FullName, "System" }).GetTypes(),
            inImplementation, s => Nat.OnlyHaveDependenciesOn(s, [example, "System"], negated: true), 2,
            [typeof(HasAnotherDependency), typeof(HasDependencies)], note: Gaps.SystemNote,
            reason: Gaps.SelfReference("HasDependency", "its dependency property's getter reads its own backing field"));

        o.Unported("MeetCustomRule_MatchesFound_ClassSelected", null, "custom-predicate");
    }
}
