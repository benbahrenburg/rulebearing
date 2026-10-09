// Element rules over types, evaluated as rb-rules evaluates them.
//
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 21
// (ElementRuleAnalyzer). Source: crates/rb-rules/src/elements/mod.rs (selection, at most one
// result per type, the AND of duplicates) and crates/rb-rules/src/elements/concepts.rs (each
// concept's test). Names compare as the engine compares them: a simple name's prefix, suffix and
// substring with case, a full name's without; equality of names, namespaces and assemblies
// without case; a pattern searched anywhere; a nested selector as the full names it selects,
// referenced types included.

using System.Runtime.CompilerServices;

namespace Rulebearing.Analyzer;

/// <summary>The types of one compilation, and the ones they depend on.</summary>
internal sealed class Universe
{
    /// <summary>The types defined here, by full name, in full-name order.</summary>
    public required IReadOnlyList<TypeFacts> Defined { get; init; }

    /// <summary>Types of other assemblies the defined ones depend on.</summary>
    public required IReadOnlyList<TypeFacts> Referenced { get; init; }
}

/// <summary>One type's verdict under one rule.</summary>
internal sealed record Verdict(TypeFacts Type, bool Passed);

/// <summary>Evaluates element rules over a <see cref="Universe"/>.</summary>
internal sealed class ElementEvaluator(Universe universe)
{
    private readonly ConditionalWeakTable<Selector, HashSet<string>> _selected = new();

    /// <summary>Each type the rule selects, with whether it meets the rule.</summary>
    public IReadOnlyList<Verdict> Evaluate(ElementRule rule) =>
        Select(rule.Select, rule.Select.IncludeReferenced).Select(t => new Verdict(t, Holds(rule.Should, t))).ToList();

    /// <summary>The types a selector selects, in full-name order.</summary>
    public IEnumerable<TypeFacts> Select(Selector selector, bool includeReferenced)
    {
        var candidates = includeReferenced ? universe.Defined.Concat(universe.Referenced).OrderBy(t => t.FullName, StringComparer.Ordinal) : universe.Defined.AsEnumerable();
        return candidates.Where(t =>
            OfKind(selector.Kind, t)
            && (selector.Languages.Count == 0 || selector.Languages.Contains("dotnet"))
            && (selector.Where is null || Holds(selector.Where, t)));
    }

    private static bool OfKind(SelectKind kind, TypeFacts type) => kind switch
    {
        SelectKind.Class => type.Kind is "class" or "attribute",
        SelectKind.Interface => type.Kind == "interface",
        SelectKind.Attribute => type.Kind == "attribute",
        _ => true,
    };

    /// <summary>Whether <paramref name="expr"/> holds for <paramref name="type"/>.</summary>
    public bool Holds(Expr expr, TypeFacts type) => expr switch
    {
        All all => all.Items.All(e => Holds(e, type)),
        Any any => any.Items.Any(e => Holds(e, type)),
        Not not => !Holds(not.Item, type),
        Test test => test.Negated != Positive(test, type),
        _ => false,
    };

    private bool Positive(Test test, TypeFacts type)
    {
        var key = type.FullName;
        var name = type.Name;
        bool AnyName(Func<string, bool> matches) => test.Operand is Names names && names.Values.Any(matches);
        bool Matches(string subject) => test.Operand is PatternOperand p && p.Pattern.IsMatch(subject);
        return test.Concept switch
        {
            Concept.Identity => Keys(test.Operand).Contains(key),
            Concept.Public => type.Visibility == "public",
            Concept.Internal => type.Visibility == "internal",
            Concept.Nested => type.Nested,
            Concept.NestedIn => Keys(test.Operand).Any(outer => key.StartsWith(outer + "+", StringComparison.Ordinal)),
            Concept.Abstract => type.Abstract,
            Concept.Sealed => type.Sealed,
            Concept.Static => type.Static,
            Concept.Record => type.Record,
            Concept.Immutable => type.Immutable,
            Concept.HaveName => AnyName(n => EqualIgnoringCase(name, n)),
            Concept.HaveNameStartingWith => AnyName(n => name.StartsWith(n, StringComparison.Ordinal)),
            Concept.HaveNameEndingWith => AnyName(n => name.EndsWith(n, StringComparison.Ordinal)),
            Concept.HaveNameContaining => AnyName(n => name.IndexOf(n, StringComparison.Ordinal) >= 0),
            Concept.HaveNameMatching => Matches(name),
            Concept.HaveFullName => AnyName(n => EqualIgnoringCase(key, n)),
            Concept.HaveFullNameStartingWith => AnyName(n => Lower(key).StartsWith(Lower(n), StringComparison.Ordinal)),
            Concept.HaveFullNameEndingWith => AnyName(n => Lower(key).EndsWith(Lower(n), StringComparison.Ordinal)),
            Concept.HaveFullNameContaining => AnyName(n => Lower(key).IndexOf(Lower(n), StringComparison.Ordinal) >= 0),
            Concept.HaveFullNameMatching => Matches(key),
            Concept.ResideInNamespace => AnyName(n => EqualIgnoringCase(type.Namespace, n)),
            Concept.ResideInNamespaceMatching => Matches(type.Namespace),
            Concept.ResideInAssembly => AnyName(n => EqualIgnoringCase(type.AssemblyFullName, n) || EqualIgnoringCase(type.Assembly, n)),
            Concept.ResideInAssemblyMatching => Matches(type.AssemblyFullName),
            Concept.ImplementInterface => Keys(test.Operand) is var wanted && type.Interfaces.Any(wanted.Contains),
            Concept.AssignableTo => Keys(test.Operand) is var targets && new[] { key }.Concat(type.BaseTypes).Concat(type.Interfaces).Any(targets.Contains),
            Concept.DependOnAny => Keys(test.Operand) is var wanted2 && type.Dependencies.Any(wanted2.Contains),
            _ => false,
        };
    }

    /// <summary>The full names an object operand stands for: its names, or what its selector selects, referenced types included.</summary>
    private HashSet<string> Keys(Operand operand) => operand switch
    {
        Names names => new HashSet<string>(names.Values, StringComparer.Ordinal),
        SelectorOperand selector => _selected.GetValue(selector.Selector, s => new HashSet<string>(Select(s, includeReferenced: true).Select(t => t.FullName), StringComparer.Ordinal)),
        _ => [],
    };

    private static string Lower(string text) => text.ToLowerInvariant();

    private static bool EqualIgnoringCase(string a, string b) =>
        string.Equals(a, b, StringComparison.Ordinal) || string.Equals(Lower(a), Lower(b), StringComparison.Ordinal);
}
