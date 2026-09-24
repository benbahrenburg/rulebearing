// The oracle: every ported case's verdict is what NetArchTest 1.3.2 itself returns over the
// committed fixtures, and every upstream assertion is checked against it before it is written.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7.
// Decision: docs/adr/0009-conformance-suites-as-specification.md (the pinned suite is the
// specification; what the passing upstream code returns is by definition what it expects).
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Mono.Cecil;
using NetArchTest.Rules;
using NetArchTest.Rules.Dependencies;
using NetArchTest.TestStructure.Dependencies.Examples;

namespace Rulebearing.Conformance.NetArchTest;

/// <summary>One upstream test's case, or one of its searches, or an entry with no case.</summary>
internal sealed record Entry(
    string File,
    string Test,
    string? CSharp,
    JsonObject? Rule,
    JsonObject? Expect,
    string? Search,
    string? Note,
    string[]? Architecture,
    string? Reason);

/// <summary>The registry of ported and unported entries, and the oracle that fills in each verdict.</summary>
internal sealed partial class Oracle
{
    /// <summary>The fixture every case loads unless it names another.</summary>
    internal const string Structure = "NetArchTest.TestStructure";

    private string file = string.Empty;

    /// <summary>Every entry, in upstream order.</summary>
    internal List<Entry> Entries { get; } = [];

    /// <summary>A type's full name as the graph spells it (<c>Outer+Nested</c>, <c>Box`1</c>).</summary>
    internal static string Name(Type type) =>
        type.FullName ?? throw new InvalidOperationException($"{type} has no full name");

    /// <summary>A Cecil type's full name as the graph spells it: Cecil writes <c>Outer/Nested</c>.</summary>
    internal static string Name(TypeDefinition type) => type.FullName.Replace('/', '+');

    /// <summary>The upstream test class the following entries belong to.</summary>
    internal void Class(string name) => file = name;

    private static void Check(bool holds, string test, string what)
    {
        if (!holds)
        {
            throw new InvalidOperationException($"{test}: the upstream assertion does not hold over the fixture: {what}");
        }
    }

    private static string Chain(string csharp) =>
        WhiteSpace().Replace(csharp.StartsWith("() => ", StringComparison.Ordinal) ? csharp[6..] : csharp, " ").Trim();

    [GeneratedRegex(@"\s+")]
    private static partial Regex WhiteSpace();

    /// <summary>
    /// Records a case: the selection, the objects that pass, and the rule. The failing objects are
    /// the rest of the selection; an empty selection is vacuous. With <paramref name="reason"/>,
    /// the verdict is still checked, and the entry is unported for that reason.
    /// </summary>
    internal void Split(
        string test,
        IEnumerable<string> selection,
        IEnumerable<string> pass,
        Func<Side, JsonNode>? where,
        Func<Side, JsonNode> should,
        string csharp,
        string? search = null,
        string? note = null,
        string[]? architecture = null,
        string? reason = null)
    {
        var selected = new SortedSet<string>(selection, StringComparer.Ordinal);
        var passing = new SortedSet<string>(pass, StringComparer.Ordinal);
        Check(passing.IsSubsetOf(selected), test, "a passing type outside the selection");
        if (reason is not null)
        {
            // Checked against upstream, but no element rule gives NetArchTest's verdict.
            Unported(test, search is null ? Chain(csharp) : $"{Chain(csharp)}: {search}", reason);
            return;
        }
        var failing = new SortedSet<string>(selected.Except(passing), StringComparer.Ordinal);
        var select = new JsonObject { ["kind"] = "type" };
        if (where is not null)
        {
            select["where"] = where(Side.Where);
        }
        var rule = new JsonObject { ["select"] = select, ["should"] = should(Side.Should) };
        if (selected.Count == 0)
        {
            note ??= "The upstream selection is empty: NetArchTest passes an empty selection, Rulebearing reports it vacuous (ADR-0007).";
        }
        var expect = selected.Count == 0
            ? new JsonObject { ["vacuous"] = true }
            : new JsonObject
            {
                ["pass"] = new JsonArray(passing.Select(n => (JsonNode)JsonValue.Create(n)).ToArray()),
                ["fail"] = new JsonArray(failing.Select(n => (JsonNode)JsonValue.Create(n)).ToArray()),
            };
        Entries.Add(new Entry(file, test, Chain(csharp), rule, expect, search, note, architecture, null));
    }

    /// <summary>
    /// A predicate test: <paramref name="result"/> is the upstream chain, <paramref name="selection"/>
    /// the same chain without the predicate under test, whose verdict per type the case records.
    /// <paramref name="count"/> and <paramref name="contains"/> are upstream's own assertions.
    /// </summary>
    internal void Predicate(
        string test,
        Func<IEnumerable<Type>> selection,
        Func<IEnumerable<Type>> result,
        Func<Side, JsonNode>? where,
        Func<Side, JsonNode> should,
        int count,
        Type[]? contains = null,
        string? note = null,
        string[]? architecture = null,
        string? reason = null,
        [CallerArgumentExpression(nameof(result))] string csharp = "")
    {
        var selected = result().ToList();
        Check(selected.Count == count, test, $"{selected.Count} types, not {count}");
        foreach (var type in contains ?? [])
        {
            Check(selected.Contains(type), test, $"{type} is not selected");
        }
        Split(test, selection().Select(Name), selected.Select(Name), where, should, csharp, null, note, architecture, reason);
    }

    /// <summary>
    /// A condition test: <paramref name="selection"/> is the chain up to <c>Should()</c>,
    /// <paramref name="result"/> the whole chain. <paramref name="successful"/> and
    /// <paramref name="failing"/> are upstream's own assertions.
    /// </summary>
    internal void Condition(
        string test,
        Func<IEnumerable<Type>> selection,
        Func<TestResult> result,
        Func<Side, JsonNode>? where,
        Func<Side, JsonNode> should,
        bool successful,
        Type[]? failing = null,
        string? note = null,
        string? reason = null,
        [CallerArgumentExpression(nameof(result))] string csharp = "")
    {
        var outcome = result();
        Check(outcome.IsSuccessful == successful, test, $"IsSuccessful is {outcome.IsSuccessful}");
        var failingNames = (outcome.FailingTypes ?? []).Select(Name).ToHashSet(StringComparer.Ordinal);
        if (failing is not null)
        {
            Check(failingNames.SetEquals(failing.Select(Name)), test, "the failing types differ");
        }
        var selected = selection().Select(Name).ToList();
        Split(test, selected, selected.Where(n => !failingNames.Contains(n)), where, should, csharp, null, note, null, reason);
    }

    /// <summary>
    /// One dependency search (<c>DependencySearch.FindTypesThatHaveDependencyOnAny</c>, as the
    /// upstream helper calls it): the subjects found pass. <paramref name="found"/> is upstream's
    /// assertion, that every subject is found or none is.
    /// </summary>
    internal void Search(
        string test,
        Func<IEnumerable<TypeDefinition>> subjects,
        IReadOnlyList<string> dependencies,
        bool found,
        Func<Side, JsonNode> where,
        string csharp,
        string? note = null) =>
        SearchWith(
            test,
            subjects,
            inputs => new DependencySearch().FindTypesThatHaveDependencyOnAny(inputs, dependencies),
            (inputs, hits) => found ? hits.Count == inputs.Count : hits.Count == 0,
            where,
            s => Nat.HaveDependencyOnAny(s, dependencies),
            csharp,
            $"FindTypesThatHaveDependencyOnAny(subjects, {Quoted(dependencies)})",
            note);

    /// <summary>
    /// One call of NetArchTest's dependency search over <paramref name="subjects"/>: the subjects
    /// it returns pass, the rest fail. <paramref name="upstream"/> is the upstream assertion over
    /// the subjects and the returned types.
    /// </summary>
    internal void SearchWith(
        string test,
        Func<IEnumerable<TypeDefinition>> subjects,
        Func<List<TypeDefinition>, IReadOnlyList<TypeDefinition>> search,
        Func<List<TypeDefinition>, IReadOnlyList<TypeDefinition>, bool> upstream,
        Func<Side, JsonNode> where,
        Func<Side, JsonNode> should,
        string csharp,
        string searchText,
        string? note = null,
        string? reason = null)
    {
        var inputs = subjects().ToList();
        var hits = search(inputs);
        Check(upstream(inputs, hits), test, $"{hits.Count} of {inputs.Count} subjects returned");
        Split(test, inputs.Select(Name), hits.Select(Name), where, should, csharp, searchText, note, null, reason);
    }

    /// <summary>A list of names as C# array text.</summary>
    internal static string Quoted(IEnumerable<string> names) =>
        "[" + string.Join(", ", names.Select(d => $"\"{d}\"")) + "]";

    /// <summary>An upstream test, or one search of it, with no case, and why.</summary>
    internal void Unported(string test, string? query, string reason) =>
        Entries.Add(new Entry(file, test, query, null, null, null, null, null, reason));

    /// <summary>
    /// Whether a dependency search entry is a constructed type: an array, pointer, by-reference or
    /// closed generic type, which NetArchTest's search tree tells apart from its element type.
    /// </summary>
    private static bool Constructed(Type type) =>
        type.IsArray || type.IsByRef || type.IsPointer || type.IsConstructedGenericType;

    internal const string ConstructedReason =
        "constructed-type: NetArchTest's search tree tells an array, pointer, by-reference or closed generic type " +
        "(ExampleDependency[], ExampleDependency<int>, ExampleDependency&) apart from its element type; the graph " +
        "document records a dependency on the type definition and each generic argument, so no element-rule key " +
        "distinguishes the constructed type from its element type";

    private const string ExternalReason =
        "external-namespace: the search names a namespace outside the analysed assembly; a nested selector ranges " +
        "over the analysed types only, so an element rule cannot ask for a dependency on any type of an external namespace";

    /// <summary>
    /// The two searches of upstream's <c>Utils.RunDependencyTest(inputs, dependency, class, namespace)</c>:
    /// by the dependency's full name, then by its namespace.
    /// </summary>
    private void Searches(
        string test,
        Func<IEnumerable<TypeDefinition>> subjects,
        Func<Side, JsonNode> where,
        Type dependency,
        bool findClass,
        bool findNamespace,
        string csharp,
        string? classReason,
        string? namespaceReason)
    {
        var fullName = Name(dependency);
        var ns = dependency.Namespace ?? string.Empty;
        classReason ??= Constructed(dependency) ? ConstructedReason : null;
        namespaceReason ??= ns.StartsWith("NetArchTest.", StringComparison.Ordinal) ? null : ExternalReason;
        if (classReason is not null)
        {
            Unported(test, $"{csharp}: FindTypesThatHaveDependencyOnAny(subjects, [\"{fullName}\"])", classReason);
        }
        else
        {
            Search(test, subjects, [fullName], findClass, where, csharp);
        }
        if (namespaceReason is not null)
        {
            Unported(test, $"{csharp}: FindTypesThatHaveDependencyOnAny(subjects, [\"{ns}\"])", namespaceReason);
        }
        else
        {
            Search(test, subjects, [ns], findNamespace, where, csharp);
        }
    }

    /// <summary>Upstream's <c>Utils.RunDependencyTest(input)</c>: a search for <c>ExampleDependency</c>.</summary>
    internal void RunDependencyTest(string test, Type input, bool expectToFind = true, string? reason = null, [CallerArgumentExpression(nameof(input))] string inputText = "") =>
        RunDependencyTest(test, input, typeof(ExampleDependency), expectToFind, expectToFind, reason, reason, inputText, "typeof(ExampleDependency)");

    /// <summary>Upstream's <c>Utils.RunDependencyTest(input, dependency, class, namespace)</c> over the types named as <paramref name="input"/>.</summary>
    internal void RunDependencyTest(
        string test,
        Type input,
        Type dependency,
        bool findClass,
        bool findNamespace,
        string? classReason = null,
        string? namespaceReason = null,
        [CallerArgumentExpression(nameof(input))] string inputText = "",
        [CallerArgumentExpression(nameof(dependency))] string dependencyText = "")
    {
        var assembly = Assembly.GetAssembly(input) ?? throw new InvalidOperationException($"{input} has no assembly");
        Searches(
            test,
            () => Types.InAssembly(assembly).That().HaveName(input.Name).GetTypeDefinitions(),
            s => Nat.HaveName(s, input.Name),
            dependency,
            findClass,
            findNamespace,
            $"Utils.RunDependencyTest({inputText}, {dependencyText}, {Bool(findClass)}, {Bool(findNamespace)})",
            classReason,
            namespaceReason);
    }

    /// <summary>
    /// Upstream's <c>Utils.RunDependencyTest(GetTypesThatResideInTheSameNamespaceButWithoutGivenType(excluded), dependency, class, namespace)</c>:
    /// every type whose namespace starts with the first excluded type's, except the excluded ones by name.
    /// </summary>
    internal void RunDependencyTestExcept(
        string test,
        Type[] excluded,
        Type dependency,
        bool findClass,
        bool findNamespace,
        string? namespaceReason = null,
        [CallerArgumentExpression(nameof(excluded))] string excludedText = "",
        [CallerArgumentExpression(nameof(dependency))] string dependencyText = "")
    {
        var first = excluded[0];
        var ns = first.Namespace ?? string.Empty;
        var assembly = Assembly.GetAssembly(first) ?? throw new InvalidOperationException($"{first} has no assembly");
        Searches(
            test,
            () =>
            {
                var types = Types.InAssembly(assembly).That().ResideInNamespaceStartingWith(ns);
                foreach (var item in excluded)
                {
                    types = types.And().DoNotHaveName(item.Name);
                }
                return types.GetTypeDefinitions();
            },
            s => Expr.All([Nat.ResideInNamespaceStartingWith(s, ns), .. excluded.Select(e => Nat.HaveName(s, e.Name, negated: true))]),
            dependency,
            findClass,
            findNamespace,
            $"Utils.RunDependencyTest(Utils.GetTypesThatResideInTheSameNamespaceButWithoutGivenType({excludedText}), {dependencyText}, {Bool(findClass)}, {Bool(findNamespace)})",
            null,
            namespaceReason);
    }

    /// <summary>Upstream's <c>Utils.RunDependencyTest(inputs, dependencies, expectToFind)</c>: one search for a list of names.</summary>
    internal void RunDependencyTestNames(
        string test,
        Type[] excluded,
        IReadOnlyList<string> dependencies,
        bool expectToFind,
        string csharp,
        string? note = null)
    {
        var first = excluded[0];
        var ns = first.Namespace ?? string.Empty;
        var assembly = Assembly.GetAssembly(first) ?? throw new InvalidOperationException($"{first} has no assembly");
        Search(
            test,
            () =>
            {
                var types = Types.InAssembly(assembly).That().ResideInNamespaceStartingWith(ns);
                foreach (var item in excluded)
                {
                    types = types.And().DoNotHaveName(item.Name);
                }
                return types.GetTypeDefinitions();
            },
            dependencies,
            expectToFind,
            s => Expr.All([Nat.ResideInNamespaceStartingWith(s, ns), .. excluded.Select(e => Nat.HaveName(s, e.Name, negated: true))]),
            csharp,
            note);
    }

    private static string Bool(bool value) => value ? "true" : "false";
}
