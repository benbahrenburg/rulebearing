// NetArchTest 1.3.2's predicates and conditions, each written as the element-rule expression that
// gives the same verdict. The keys are rb-config's (crates/rb-config/src/elements.rs, VOCABULARY);
// the semantics each mapping reproduces are NetArchTest's FunctionDelegates.cs, cited per method.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7.
using System.Text;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;

namespace Rulebearing.Conformance.NetArchTest;

/// <summary>Which side of a rule an expression is on, which decides how its keys are spelled.</summary>
internal enum Side
{
    /// <summary><c>select.where</c>: <c>arePublic</c>, <c>doNotHaveName</c>.</summary>
    Where,

    /// <summary><c>should</c>: <c>bePublic</c>, <c>notHaveName</c>.</summary>
    Should,
}

/// <summary>Element-rule expressions: one test, and the <c>all</c>, <c>any</c> and <c>not</c> combinators.</summary>
internal static class Expr
{
    private static readonly string[] Verbs = ["have", "reside", "depend", "only", "call", "implement", "exist", "adhere"];

    /// <summary>A concept's key on a side, spelled as rb-config spells it.</summary>
    internal static string Key(string concept, Side side, bool negated)
    {
        var verb = Verbs.Any(v => concept.StartsWith(v, StringComparison.Ordinal));
        var capital = concept.Length == 0 ? string.Empty : char.ToUpperInvariant(concept[0]) + concept[1..];
        return (side, verb, negated) switch
        {
            (Side.Where, true, false) => concept,
            (Side.Where, true, true) => "doNot" + capital,
            (Side.Where, false, false) => "are" + capital,
            (Side.Where, false, true) => "areNot" + capital,
            (Side.Should, true, false) => concept,
            (Side.Should, true, true) => "not" + capital,
            (Side.Should, false, false) => "be" + capital,
            _ => "notBe" + capital,
        };
    }

    /// <summary>One test: <c>{ key: value }</c>, a flag's value being <c>true</c>.</summary>
    internal static JsonObject Test(string concept, Side side, bool negated, JsonNode? value = null) =>
        new() { [Key(concept, side, negated)] = value ?? JsonValue.Create(true) };

    /// <summary>Every item holds.</summary>
    internal static JsonObject All(params JsonNode[] items) => new() { ["all"] = new JsonArray(items) };

    /// <summary>Some item holds.</summary>
    internal static JsonObject Any(params JsonNode[] items) => new() { ["any"] = new JsonArray(items) };

    /// <summary>The item does not hold.</summary>
    internal static JsonObject Not(JsonNode item) => new() { ["not"] = item };

    /// <summary><paramref name="expression"/>, or its negation.</summary>
    internal static JsonNode Negated(JsonNode expression, bool negated) => negated ? Not(expression) : expression;

    /// <summary>A nested selector: every type of <paramref name="kind"/> that <paramref name="where"/> keeps.</summary>
    internal static JsonObject Selector(string kind, JsonNode? where = null)
    {
        var selector = new JsonObject { ["kind"] = kind };
        if (where is not null)
        {
            selector["where"] = where;
        }
        return selector;
    }

    /// <summary>Types named by their full names, as the graph spells them (<c>Outer+Nested</c>).</summary>
    internal static JsonArray Names(params Type[] types) =>
        new(types.Select(t => (JsonNode)JsonValue.Create(Oracle.Name(t))).ToArray());
}

/// <summary>Regular-expression text for the element rules' JavaScript dialect.</summary>
internal static class Patterns
{
    /// <summary>Escapes literal text.</summary>
    internal static string Escape(string text)
    {
        var escaped = new StringBuilder();
        foreach (var c in text)
        {
            if (@"\^$.|?*+()[]{}".Contains(c, StringComparison.Ordinal))
            {
                escaped.Append('\\');
            }
            escaped.Append(c);
        }
        return escaped.ToString();
    }

    /// <summary>
    /// The pattern with every letter outside an escape matching either case: what .NET's
    /// <c>RegexOptions.IgnoreCase</c> and <c>StringComparison.InvariantCultureIgnoreCase</c> do for
    /// ASCII, which the JavaScript dialect has no flag for. A letter inside a character class is
    /// refused rather than translated, so the tool never writes a pattern it has not proven.
    /// </summary>
    internal static string IgnoreCase(string pattern)
    {
        var result = new StringBuilder();
        var inClass = false;
        for (var i = 0; i < pattern.Length; i++)
        {
            var c = pattern[i];
            if (c == '\\' && i + 1 < pattern.Length)
            {
                result.Append(c).Append(pattern[++i]);
            }
            else if (inClass)
            {
                if (char.IsLetter(c))
                {
                    throw new NotSupportedException($"a letter inside a character class in {pattern}");
                }
                inClass = c != ']';
                result.Append(c);
            }
            else if (c == '[')
            {
                inClass = true;
                result.Append(c);
            }
            else if (char.IsAsciiLetter(c))
            {
                result.Append('[').Append(char.ToUpperInvariant(c)).Append(char.ToLowerInvariant(c)).Append(']');
            }
            else
            {
                result.Append(c);
            }
        }
        return result.ToString();
    }
}

/// <summary>
/// NetArchTest's functions (FunctionDelegates.cs), each as the expression with the same verdict.
/// A predicate and its condition share one function upstream, so one method serves both sides;
/// <c>negated</c> is NetArchTest's own negative (<c>DoNotHaveName</c>, <c>NotHaveName</c>).
/// </summary>
internal static class Nat
{
    /// <summary><c>ResideInNamespace(name)</c>: <c>FullName.StartsWith(name, InvariantCultureIgnoreCase)</c>, a string prefix of the full name, which <c>haveFullNameStartingWith</c> is (it ignores case).</summary>
    internal static JsonNode ResideInNamespace(Side side, string name, bool negated = false) =>
        Expr.Test("haveFullNameStartingWith", side, negated, name);

    /// <summary><c>HaveName(name)</c>: <c>Name.Equals(name, InvariantCultureIgnoreCase)</c>, as <c>haveName</c>.</summary>
    internal static JsonNode HaveName(Side side, string name, bool negated = false) =>
        Expr.Test("haveName", side, negated, name);

    /// <summary>
    /// <c>HaveNameStartingWith(start)</c>: <c>Name.StartsWith(start, InvariantCultureIgnoreCase)</c>, a
    /// case-insensitive prefix, so an anchored case-insensitive pattern; with
    /// <c>StringComparison.Ordinal</c>, <c>haveNameStartingWith</c>, which is ordinal.
    /// </summary>
    internal static JsonNode HaveNameStartingWith(Side side, string start, bool negated = false, bool ordinal = false) =>
        ordinal
            ? Expr.Test("haveNameStartingWith", side, negated, start)
            : Expr.Test("haveNameMatching", side, negated, "^" + Patterns.IgnoreCase(Patterns.Escape(start)));

    /// <summary><c>HaveNameEndingWith(end)</c>, as <see cref="HaveNameStartingWith"/> at the other end.</summary>
    internal static JsonNode HaveNameEndingWith(Side side, string end, bool negated = false, bool ordinal = false) =>
        ordinal
            ? Expr.Test("haveNameEndingWith", side, negated, end)
            : Expr.Test("haveNameMatching", side, negated, Patterns.IgnoreCase(Patterns.Escape(end)) + "$");

    /// <summary><c>HaveNameMatching(pattern)</c>: <c>new Regex(pattern, IgnoreCase).Match(Name)</c>.</summary>
    internal static JsonNode HaveNameMatching(Side side, string pattern, bool negated = false) =>
        Expr.Test("haveNameMatching", side, negated, Patterns.IgnoreCase(pattern));

    /// <summary><c>HaveCustomAttribute(attribute)</c>: an attribute of exactly that type, as <c>haveAnyAttributes</c>.</summary>
    internal static JsonNode HaveCustomAttribute(Side side, Type attribute, bool negated = false) =>
        Expr.Test("haveAnyAttributes", side, negated, Expr.Names(attribute));

    /// <summary><c>HaveCustomAttributeOrInherit(attribute)</c>: an attribute of that type or a subclass of it.</summary>
    internal static JsonNode HaveCustomAttributeOrInherit(Side side, Type attribute, bool negated = false) =>
        Expr.Test("haveAnyAttributes", side, negated,
            Expr.Selector("type", Expr.Test("assignableTo", Side.Where, false, Expr.Names(attribute))));

    /// <summary>
    /// <c>Inherit(type)</c>: <c>IsSubclassOf</c>, the base-class chain without the type itself.
    /// <c>assignableTo</c> adds the type itself (and interfaces, which a class never is), so the type
    /// itself is taken out.
    /// </summary>
    internal static JsonNode Inherit(Side side, Type type, bool negated = false) =>
        Expr.Negated(
            Expr.All(
                Expr.Test("assignableTo", side, false, Expr.Names(type)),
                Expr.Test(string.Empty, side, true, Expr.Names(type))),
            negated);

    /// <summary><c>ImplementInterface(type)</c>: an interface the type lists, as <c>implementInterface</c>.</summary>
    internal static JsonNode ImplementInterface(Side side, Type type, bool negated = false) =>
        Expr.Test("implementInterface", side, negated, Expr.Names(type));

    /// <summary><c>BeAbstract</c>: Cecil's <c>IsAbstract</c>.</summary>
    internal static JsonNode BeAbstract(Side side, bool negated = false) => Expr.Test("abstract", side, negated);

    /// <summary><c>BeClasses</c>: Cecil's <c>IsClass</c>, which is every type that is not an interface.</summary>
    internal static JsonNode BeClass(Side side, bool negated = false) =>
        Expr.Test(string.Empty, side, !negated, Expr.Selector("interface"));

    /// <summary><c>BeInterfaces</c>: Cecil's <c>IsInterface</c>.</summary>
    internal static JsonNode BeInterface(Side side, bool negated = false) =>
        Expr.Test(string.Empty, side, negated, Expr.Selector("interface"));

    /// <summary><c>BeGeneric</c>: Cecil's <c>HasGenericParameters</c>.</summary>
    internal static JsonNode BeGeneric(Side side, bool negated = false) => Expr.Test("generic", side, negated);

    /// <summary><c>BeStatic</c>: abstract, sealed, not an interface, no public constructor.</summary>
    internal static JsonNode BeStatic(Side side, bool negated = false) => Expr.Test("static", side, negated);

    /// <summary><c>BeNested</c>: Cecil's <c>IsNested</c>.</summary>
    internal static JsonNode BeNested(Side side, bool negated = false) => Expr.Test("nested", side, negated);

    /// <summary><c>BeNestedPublic</c>: Cecil's <c>IsNestedPublic</c>, a nested type declared public.</summary>
    internal static JsonNode BeNestedPublic(Side side, bool negated = false) =>
        Expr.Negated(Expr.All(Expr.Test("nested", side, false), Expr.Test("public", side, false)), negated);

    /// <summary><c>BeNestedPrivate</c>: Cecil's <c>IsNestedPrivate</c>, a nested type declared private.</summary>
    internal static JsonNode BeNestedPrivate(Side side, bool negated = false) =>
        Expr.Negated(Expr.All(Expr.Test("nested", side, false), Expr.Test("private", side, false)), negated);

    /// <summary><c>BePublic</c>: <c>IsNested ? IsNestedPublic : IsPublic</c>, the declared visibility.</summary>
    internal static JsonNode BePublic(Side side, bool negated = false) => Expr.Test("public", side, negated);

    /// <summary><c>BeSealed</c>: Cecil's <c>IsSealed</c>.</summary>
    internal static JsonNode BeSealed(Side side, bool negated = false) => Expr.Test("sealed", side, negated);

    /// <summary>
    /// <c>ResideInNamespaceMatching(pattern)</c>: the case-insensitive pattern against
    /// <c>GetNamespace()</c>, which is the namespace of a type that is not nested, the declaring
    /// type's full name for a nested public or private type, and Cecil's empty namespace for any
    /// other nested type. <c>resideInNamespaceMatching</c> reads a nested type's enclosing namespace,
    /// so the nested cases are written with <c>areNestedIn</c>.
    /// </summary>
    internal static JsonNode ResideInNamespaceMatching(Side side, string pattern, bool negated = false)
    {
        var insensitive = Patterns.IgnoreCase(pattern);
        var branches = new List<JsonNode>
        {
            Expr.All(
                Expr.Test("nested", side, true),
                Expr.Test("resideInNamespaceMatching", side, false, insensitive)),
            Expr.All(
                Expr.Test("nested", side, false),
                Expr.Any(Expr.Test("public", side, false), Expr.Test("private", side, false)),
                Expr.Test("nestedIn", side, false,
                    Expr.Selector("type", Expr.Test("haveFullNameMatching", Side.Where, false, insensitive)))),
        };
        if (Regex.IsMatch(string.Empty, pattern, RegexOptions.IgnoreCase))
        {
            branches.Add(Expr.All(
                Expr.Test("nested", side, false),
                Expr.Test("public", side, true),
                Expr.Test("private", side, true)));
        }
        return Expr.Negated(Expr.Any([.. branches]), negated);
    }

    /// <summary><c>ResideInNamespaceStartingWith(name)</c>: <c>ResideInNamespaceMatching("^" + name)</c>, the name read as a pattern.</summary>
    internal static JsonNode ResideInNamespaceStartingWith(Side side, string name, bool negated = false) =>
        ResideInNamespaceMatching(side, "^" + name, negated);

    /// <summary><c>ResideInNamespaceEndingWith(name)</c>: <c>ResideInNamespaceMatching(name + "$")</c>.</summary>
    internal static JsonNode ResideInNamespaceEndingWith(Side side, string name, bool negated = false) =>
        ResideInNamespaceMatching(side, name + "$", negated);

    /// <summary><c>ResideInNamespaceContaining(name)</c>: <c>ResideInNamespaceMatching("^.*" + name + ".*$")</c>.</summary>
    internal static JsonNode ResideInNamespaceContaining(Side side, string name, bool negated = false) =>
        ResideInNamespaceMatching(side, "^.*" + name + ".*$", negated);

    /// <summary>
    /// The types a dependency search list names. NetArchTest's search tree matches a referenced
    /// type when the list entry is a prefix of its name in whole segments (separated by <c>.</c>,
    /// <c>/</c>, <c>+</c>, <c>:</c>), case-sensitively: an entry names a type, its nested types and a
    /// whole namespace alike. The selector ranges over the analysed types, which is where every
    /// entry this suite ports points; an entry naming an external namespace (<c>System</c>) selects
    /// nothing, which <c>onlyDependOn</c> does not need because it ignores types outside the
    /// analysed code.
    /// </summary>
    internal static JsonObject Dependencies(IEnumerable<string> entries) =>
        Expr.Selector("type", Expr.Test("haveFullNameMatching", Side.Where, false,
            "^(?:" + string.Join('|', entries.Distinct(StringComparer.Ordinal).Select(e => Patterns.Escape(e.Replace('/', '+')))) + ")(?:$|[.+])"));

    /// <summary><c>HaveDependencyOnAny(entries)</c> (and <c>HaveDependencyOn</c>, one entry).</summary>
    internal static JsonNode HaveDependencyOnAny(Side side, IEnumerable<string> entries, bool negated = false) =>
        Expr.Test("dependOnAny", side, negated, Dependencies(entries));

    /// <summary><c>HaveDependencyOnAll(entries)</c>: a dependency matching each distinct entry.</summary>
    internal static JsonNode HaveDependencyOnAll(Side side, IEnumerable<string> entries, bool negated = false) =>
        Expr.Negated(
            Expr.All([.. entries.Distinct(StringComparer.Ordinal).Select(e => Expr.Test("dependOnAny", side, false, Dependencies([e])))]),
            negated);

    /// <summary><c>OnlyHaveDependenciesOn(entries)</c>; negated, <c>HaveDependenciesOtherThan(entries)</c>.</summary>
    internal static JsonNode OnlyHaveDependenciesOn(Side side, IEnumerable<string> entries, bool negated = false) =>
        Expr.Test("onlyDependOn", side, negated, Dependencies(entries));
}
