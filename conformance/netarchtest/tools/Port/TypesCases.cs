// NetArchTest 1.3.2 test/NetArchTest.Rules.UnitTests/{TypesTests,PolicyDefinitionTests}.cs: the
// tests that assert over the test structure become cases, the ones that assert NetArchTest's own
// loading and policy API are unported with the reason, and Gaps holds the vocabulary-gap texts.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7.
// The chains are upstream's C#, which is not nullable-annotated (Assembly.GetAssembly returns a
// nullable reference), so nullable analysis is off for this transcription only.
#nullable disable
using System.Reflection;
using System.Runtime.CompilerServices;
using NetArchTest.Rules;
using NetArchTest.TestStructure.NameMatching.Namespace1;
using NetArchTest.TestStructure.NameMatching.Namespace2;
using NetArchTest.TestStructure.NameMatching.Namespace2.Namespace3;
using NetArchTest.TestStructure.NameMatching.Namespace3.A;
using NetArchTest.TestStructure.NameMatching.Namespace3.B;

namespace Rulebearing.Conformance.NetArchTest;

/// <summary>The reasons shared by several unported entries and notes.</summary>
internal static class Gaps
{
    /// <summary>NetArchTest's immutability is not ArchUnitNET's.</summary>
    internal const string Immutable =
        "vocabulary-gap: NetArchTest's BeImmutable holds when no property has a public setter and every field is " +
        "non-public, readonly or const, static members included; the immutable key follows ArchUnitNET, which wants " +
        "every instance field readonly and no setter of any visibility, so ImmutableClass2 (a private setter) and " +
        "ImmutableClass3 (a protected property, a private field) take opposite verdicts, and no element-rule key " +
        "quantifies over a type's members to express NetArchTest's definition";

    /// <summary>Member nullability is not in the graph document.</summary>
    internal const string Nullable =
        "vocabulary-gap: OnlyHaveNullableMembers and HaveSomeNonNullableMembers ask whether every field and property " +
        "is of a reference type or Nullable<T>; the graph document records no member's type and no element-rule key " +
        "asks it";

    /// <summary>Why a list entry naming the System namespace keeps NetArchTest's verdict.</summary>
    internal const string SystemNote =
        "The list's System entry names a namespace outside the analysed assembly: onlyDependOn ignores dependencies " +
        "on types outside the analysed code, and every such dependency of the selected types is on a System type, so " +
        "the verdict is NetArchTest's.";

    /// <summary>
    /// A type's references to its own members: NetArchTest skips them, ArchUnitNET's definition,
    /// which the graph document follows, counts them.
    /// </summary>
    internal static string SelfReference(string type, string how) =>
        "dependency-definition: NetArchTest does not count a type's references to its own fields and methods " +
        "(TypeDefinitionCheckingContext skips a member reference whose declaring type is the type checked); " +
        $"ArchUnitNET's definition, which the graph document and onlyDependOn follow, does, so {type} ({how}) " +
        "depends on itself and fails onlyDependOn, and no element-rule key names the selected type itself as an " +
        "allowed dependency";

    /// <summary>Attributes on an event.</summary>
    internal const string EventAttribute =
        "dependency-definition: NetArchTest counts an attribute on an event as a dependency of the declaring type; " +
        "ArchUnitNET's definition, which the graph document follows, loads no event members and so no event " +
        "attributes, and no element-rule key reads them";

    /// <summary>A lambda's closure class.</summary>
    internal const string Closure =
        "dependency-definition: NetArchTest searches the compiler-generated closure class a lambda captures into " +
        "(<>c__DisplayClass), so the captured local's type is a dependency; ArchUnitNET's definition, which the " +
        "graph document follows, leaves compiler-generated types out and follows only the methods a body calls, " +
        "and no element-rule key reads a closure's fields";

    /// <summary>An uncalled static local function.</summary>
    internal const string LocalFunction =
        "dependency-definition: NetArchTest searches every method of the type, including the compiler-generated " +
        "method a static local function compiles to, which nothing calls here; ArchUnitNET's definition, which the " +
        "graph document follows, loads no compiler-generated member and reaches one only through a call, so the " +
        "local function's body is not a dependency";

    /// <summary>A search over every type under Dependencies.Search.</summary>
    internal const string SearchSubjects =
        "dependency-definition: the subjects, every type under Dependencies.Search but IndirectReference, include " +
        "AttributeOnEvent, LambdaCapturedVariable and StaticLocalFunction, whose dependency on the Examples namespace " +
        "NetArchTest finds through an event attribute, a closure field and an uncalled static local function, three " +
        "places ArchUnitNET's definition, which the graph document follows, does not read (their own entries say how)";

    /// <summary>Types.InCurrentDomain.</summary>
    internal const string CurrentDomain =
        "api-only: Types.InCurrentDomain() loads every assembly of the running AppDomain and filters NetArchTest's " +
        "exclusion list; which assemblies those are is a property of the test host, and a rule's scope is the " +
        "assemblies its configuration names (languages.dotnet.assemblies)";

    /// <summary>Types.FromFile and FromPath.</summary>
    internal const string Loading =
        "api-only: Types.FromFile and Types.FromPath locate assemblies on disk beside the test host; locating " +
        "assemblies is the configuration's languages.dotnet.assemblies, not a rule";

    /// <summary>A policy's names and descriptions.</summary>
    internal const string Policy =
        "api-only: asserts the names and descriptions a NetArchTest policy attaches to its results; a rule's name, " +
        "comment and because are configuration fields, not verdicts";
}

/// <summary>TypesTests and PolicyDefinitionTests.</summary>
internal static class TypesCases
{
    private const string NameMatching = "NetArchTest.TestStructure.NameMatching";

    private static Assembly Structure => Assembly.GetAssembly(typeof(ClassA1));

    /// <summary>Registers every TypesTests case.</summary>
    internal static void Register(Oracle o)
    {
        o.Class("TypesTests");
        o.Unported("InCurrentDomain_SystemTypesExcluded", null, Gaps.CurrentDomain);
        o.Unported("InCurrentDomain_TypesWithPrefixSystemInclude", null, Gaps.CurrentDomain);
        o.Unported("InCurrentDomain_TypesWithPrefixModuleInclude", null, Gaps.CurrentDomain);
        o.Unported("InCurrentDomain_SystemTypesExcludedModule", null, Gaps.CurrentDomain);
        o.Unported("InCurrentDomain_NetArchTestTypesExcluded", null, Gaps.CurrentDomain);
        o.Unported("InCurrentDomain_NestedPublicTypesPresent_Returned", null, Gaps.CurrentDomain);
        o.Unported("InCurrentDomain_NestedPrivateTypesPresent_Returned", null, Gaps.CurrentDomain);

        // InNamespace reads the top-level types of every assembly loaded in the AppDomain whose
        // namespace starts with the name, ignoring case, and their nested types; of the loaded
        // assemblies only the test structure has types in the namespace.
        o.Predicate("InNamespace_TypesReturned",
            () => Types.InAssembly(Structure).GetTypes(),
            () => Types.InNamespace("NetArchTest.TestStructure.NameMatching").GetTypes(),
            null, s => Expr.Test("resideInNamespaceMatching", s, false, "^" + Patterns.IgnoreCase(Patterns.Escape(NameMatching))), 9,
            [typeof(ClassA1), typeof(ClassA2), typeof(ClassA3), typeof(ClassB1), typeof(ClassB2), typeof(SomeThing), typeof(SomethingElse), typeof(SomeEntity), typeof(SomeIdentity)],
            note: "Types.InNamespace reads every assembly loaded in the AppDomain; only NetArchTest.TestStructure has types in the namespace.");

        o.Unported("FromFile_TypesReturned", null, Gaps.Loading);
        o.Unported("FromPath_TypesReturned", null, Gaps.Loading);
        o.Unported("FromFile_BadImage_CaughtAndEmptyListReturned", null,
            "api-only: Types.FromFile on a PDB swallows the BadImageFormatException and returns no types; Rulebearing's reader refuses a file that is not an assembly with exit 2 and a named reason (docs/architecture.md, Security posture)");

        const string Search = "NetArchTest.TestStructure.Dependencies.Search";
        var searchTypes = Types.InNamespace(Search).GetTypes().ToList();
        var compilerGenerated = typeof(CompilerGeneratedAttribute).FullName;
        o.Split("InNamespace_CompilerGeneratedClasses_NotReturned",
            searchTypes.Select(Oracle.Name),
            searchTypes.Where(r => !r.CustomAttributes.Any(x => x?.AttributeType?.FullName == compilerGenerated)).Select(Oracle.Name),
            s => Expr.Test("resideInNamespaceMatching", s, false, "^" + Patterns.IgnoreCase(Patterns.Escape(Search))),
            s => Expr.Test("haveAnyAttributes", s, true, new System.Text.Json.Nodes.JsonArray(compilerGenerated)),
            "Types.InNamespace(\"NetArchTest.TestStructure.Dependencies.Search\").GetTypes().Any(r => r.CustomAttributes.Any(x => x?.AttributeType?.FullName == typeof(CompilerGeneratedAttribute).FullName))");

        Policies(o);
    }

    private static void Policies(Oracle o)
    {
        o.Class("PolicyDefinitionTests");
        // Each rule a policy adds is a rule over the test structure; the case is that rule's result.
        IEnumerable<Type> Namespace3() => Types.InAssembly(Structure).That().ResideInNamespace(NameMatching + ".Namespace2.Namespace3").GetTypes();
        IEnumerable<Type> NotNamespace3() => Types.InAssembly(Structure).That().ResideInNamespace(NameMatching).And().DoNotResideInNamespace(NameMatching + ".Namespace3").GetTypes();
        IEnumerable<Type> All() => Types.InAssembly(Structure).That().ResideInNamespace(NameMatching).GetTypes();
        Func<Side, System.Text.Json.Nodes.JsonNode> notNamespace3 = s => Expr.All(Nat.ResideInNamespace(s, NameMatching), Nat.ResideInNamespace(s, NameMatching + ".Namespace3", negated: true));

        o.Condition("Evaluate_RuleAdded_ExecutedWhenEvaluated",
            Namespace3,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace2.Namespace3").Should().HaveName("XXXXXX").GetResult(),
            s => Nat.ResideInNamespace(s, NameMatching + ".Namespace2.Namespace3"), s => Nat.HaveName(s, "XXXXXX"), false, [typeof(ClassB2)]);

        o.Condition("Evaluate_MultipleRulesAdded_Aggregated",
            NotNamespace3,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().DoNotResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace3").Should().HaveNameStartingWith("Class").GetResult(),
            notNamespace3, s => Nat.HaveNameStartingWith(s, "Class"), true);
        o.Condition("Evaluate_MultipleRulesAdded_Aggregated",
            All,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").Should().BeClasses().GetResult(),
            s => Nat.ResideInNamespace(s, NameMatching), s => Nat.BeClass(s), true);

        o.Condition("Evaluate_MultipleCalls_MultipleResults",
            NotNamespace3,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").And().DoNotResideInNamespace("NetArchTest.TestStructure.NameMatching.Namespace3").Should().HaveNameStartingWith("Class").GetResult(),
            notNamespace3, s => Nat.HaveNameStartingWith(s, "Class"), true);
        o.Condition("Evaluate_MultipleCalls_MultipleResults",
            All,
            () => Types.InAssembly(Assembly.GetAssembly(typeof(ClassA1))).That().ResideInNamespace("NetArchTest.TestStructure.NameMatching").Should().BeSealed().GetResult(),
            s => Nat.ResideInNamespace(s, NameMatching), s => Nat.BeSealed(s), false);

        o.Unported("Evaluate_NAmeAndDescription_Optional", null, Gaps.Policy);
        o.Unported("Evaluate_EmptyPolicy_EvaluateToEmptyResults", null,
            "api-only: a policy with no rules evaluates to no results; a configuration with no rules has nothing to evaluate");
        o.Unported("Evaluate_RuleNameAndDescription_AssociatedWithResult", null, Gaps.Policy);
        o.Unported("Evaluate_PolicyNameAndDescription_AssociatedWIthResultSet", null, Gaps.Policy);
    }
}
