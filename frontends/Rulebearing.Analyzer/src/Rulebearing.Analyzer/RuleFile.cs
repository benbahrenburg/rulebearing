// The rule file the analyzer reads: rulebearing.yaml, native format, the dependency and element
// families only.
//
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 21 (RuleFileReader).
// Source: crates/rb-config/src/elements.rs (the element rule's keys, split_key, the vocabulary and
// the operand kinds), which this follows for the concepts it evaluates.
//
// A rule the analyzer cannot evaluate exactly as the gate does is kept with the reason and not
// evaluated: `cruise` reports it. That is a rule with a concept outside the evaluated set, a
// member or module selector, a dependency rule with any condition beyond `path` and `pathNot`, or
// a family other than `forbidden` and `elements`.

using YamlDotNet.RepresentationModel;

namespace Rulebearing.Analyzer;

/// <summary>A rule's severity.</summary>
internal enum Severity
{
    /// <summary><c>error</c>: fails the build.</summary>
    Error,

    /// <summary><c>warn</c>.</summary>
    Warn,

    /// <summary><c>info</c>.</summary>
    Info,

    /// <summary><c>ignore</c>: not evaluated.</summary>
    Ignore,
}

/// <summary>What an element rule's test reads.</summary>
internal enum Concept
{
    /// <summary><c>are</c> / <c>be</c>: the element is one of the operand's.</summary>
    Identity,

    /// <summary><c>public</c>.</summary>
    Public,

    /// <summary><c>internal</c>.</summary>
    Internal,

    /// <summary><c>nested</c>.</summary>
    Nested,

    /// <summary><c>nestedIn</c>.</summary>
    NestedIn,

    /// <summary><c>abstract</c>.</summary>
    Abstract,

    /// <summary><c>sealed</c>.</summary>
    Sealed,

    /// <summary><c>static</c>.</summary>
    Static,

    /// <summary><c>record</c>.</summary>
    Record,

    /// <summary><c>immutable</c>.</summary>
    Immutable,

    /// <summary><c>haveName</c>.</summary>
    HaveName,

    /// <summary><c>haveNameStartingWith</c>.</summary>
    HaveNameStartingWith,

    /// <summary><c>haveNameEndingWith</c>.</summary>
    HaveNameEndingWith,

    /// <summary><c>haveNameContaining</c>.</summary>
    HaveNameContaining,

    /// <summary><c>haveNameMatching</c>.</summary>
    HaveNameMatching,

    /// <summary><c>haveFullName</c>.</summary>
    HaveFullName,

    /// <summary><c>haveFullNameStartingWith</c>.</summary>
    HaveFullNameStartingWith,

    /// <summary><c>haveFullNameEndingWith</c>.</summary>
    HaveFullNameEndingWith,

    /// <summary><c>haveFullNameContaining</c>.</summary>
    HaveFullNameContaining,

    /// <summary><c>haveFullNameMatching</c>.</summary>
    HaveFullNameMatching,

    /// <summary><c>resideInNamespace</c>.</summary>
    ResideInNamespace,

    /// <summary><c>resideInNamespaceMatching</c>.</summary>
    ResideInNamespaceMatching,

    /// <summary><c>resideInAssembly</c>.</summary>
    ResideInAssembly,

    /// <summary><c>resideInAssemblyMatching</c>.</summary>
    ResideInAssemblyMatching,

    /// <summary><c>implementInterface</c>.</summary>
    ImplementInterface,

    /// <summary><c>assignableTo</c>.</summary>
    AssignableTo,

    /// <summary><c>dependOnAny</c>.</summary>
    DependOnAny,
}

/// <summary>What a concept's operand is.</summary>
internal enum ValueKind
{
    /// <summary>A boolean; <c>false</c> flips the negation.</summary>
    Flag,

    /// <summary>A name or a list of names.</summary>
    Names,

    /// <summary>One pattern.</summary>
    Pattern,

    /// <summary>Names, or a nested selector.</summary>
    Objects,
}

/// <summary>The kinds a selector can select that the analyzer evaluates.</summary>
internal enum SelectKind
{
    /// <summary><c>type</c>: every type.</summary>
    Type,

    /// <summary><c>class</c>: classes and attributes.</summary>
    Class,

    /// <summary><c>interface</c>.</summary>
    Interface,

    /// <summary><c>attribute</c>.</summary>
    Attribute,
}

/// <summary>A boolean expression over an element.</summary>
internal abstract record Expr;

/// <summary>Every item holds.</summary>
internal sealed record All(IReadOnlyList<Expr> Items) : Expr;

/// <summary>Some item holds.</summary>
internal sealed record Any(IReadOnlyList<Expr> Items) : Expr;

/// <summary>The item does not hold.</summary>
internal sealed record Not(Expr Item) : Expr;

/// <summary>One test: a concept, whether it is negated, and its operand; <paramref name="Key"/> is the key as written.</summary>
internal sealed record Test(string Key, Concept Concept, bool Negated, Operand Operand) : Expr;

/// <summary>A test's operand.</summary>
internal abstract record Operand;

/// <summary>No operand: a flag.</summary>
internal sealed record NoOperand : Operand;

/// <summary>Names, compared as the concept compares them.</summary>
internal sealed record Names(IReadOnlyList<string> Values) : Operand;

/// <summary>A pattern.</summary>
internal sealed record PatternOperand(JsPattern Pattern) : Operand;

/// <summary>A nested selector: the elements it selects, referenced types included.</summary>
internal sealed record SelectorOperand(Selector Selector) : Operand;

/// <summary>A selector: which elements a rule judges.</summary>
internal sealed record Selector(SelectKind Kind, Expr? Where, IReadOnlyList<string> Languages, bool IncludeReferenced);

/// <summary>An element rule.</summary>
internal sealed record ElementRule(string Name, Severity Severity, string? Comment, string? Fix, bool AllowEmpty, Selector Select, Expr Should);

/// <summary>A path condition: <c>path</c> and <c>pathNot</c>, each one or more patterns.</summary>
internal sealed record PathCondition(IReadOnlyList<JsPattern> Path, IReadOnlyList<JsPattern> PathNot);

/// <summary>A forbidden dependency rule over file paths.</summary>
internal sealed record DependencyRule(string Name, Severity Severity, string? Comment, string? Fix, PathCondition From, PathCondition To);

/// <summary>A rule the analyzer leaves to <c>cruise</c>, with why.</summary>
internal sealed record Skipped(string Name, string Family, string Reason);

/// <summary>A rule file that cannot be read.</summary>
internal sealed class RuleFileException : Exception
{
    /// <summary>A failure described by <paramref name="message"/>.</summary>
    public RuleFileException(string message)
        : base(message)
    {
    }
}

/// <summary>The rules of one rulebearing.yaml.</summary>
internal sealed class RuleFile
{
    private static readonly Dictionary<string, (Concept Concept, ValueKind Kind)> Vocabulary = new(StringComparer.Ordinal)
    {
        [""] = (Concept.Identity, ValueKind.Objects),
        ["public"] = (Concept.Public, ValueKind.Flag),
        ["internal"] = (Concept.Internal, ValueKind.Flag),
        ["nested"] = (Concept.Nested, ValueKind.Flag),
        ["nestedIn"] = (Concept.NestedIn, ValueKind.Objects),
        ["abstract"] = (Concept.Abstract, ValueKind.Flag),
        ["sealed"] = (Concept.Sealed, ValueKind.Flag),
        ["static"] = (Concept.Static, ValueKind.Flag),
        ["record"] = (Concept.Record, ValueKind.Flag),
        ["immutable"] = (Concept.Immutable, ValueKind.Flag),
        ["haveName"] = (Concept.HaveName, ValueKind.Names),
        ["haveNameStartingWith"] = (Concept.HaveNameStartingWith, ValueKind.Names),
        ["haveNameEndingWith"] = (Concept.HaveNameEndingWith, ValueKind.Names),
        ["haveNameContaining"] = (Concept.HaveNameContaining, ValueKind.Names),
        ["haveNameMatching"] = (Concept.HaveNameMatching, ValueKind.Pattern),
        ["haveFullName"] = (Concept.HaveFullName, ValueKind.Names),
        ["haveFullNameStartingWith"] = (Concept.HaveFullNameStartingWith, ValueKind.Names),
        ["haveFullNameEndingWith"] = (Concept.HaveFullNameEndingWith, ValueKind.Names),
        ["haveFullNameContaining"] = (Concept.HaveFullNameContaining, ValueKind.Names),
        ["haveFullNameMatching"] = (Concept.HaveFullNameMatching, ValueKind.Pattern),
        ["resideInNamespace"] = (Concept.ResideInNamespace, ValueKind.Names),
        ["resideInNamespaceMatching"] = (Concept.ResideInNamespaceMatching, ValueKind.Pattern),
        ["resideInAssembly"] = (Concept.ResideInAssembly, ValueKind.Names),
        ["resideInAssemblyMatching"] = (Concept.ResideInAssemblyMatching, ValueKind.Pattern),
        ["implementInterface"] = (Concept.ImplementInterface, ValueKind.Objects),
        ["assignableTo"] = (Concept.AssignableTo, ValueKind.Objects),
        ["dependOnAny"] = (Concept.DependOnAny, ValueKind.Objects),
    };

    private static readonly HashSet<string> ElementKeys = new(StringComparer.Ordinal)
    {
        "name", "comment", "fix", "severity", "expires", "owner", "since", "deprecated", "replacedBy", "allowEmpty", "because", "select", "should",
    };

    private static readonly HashSet<string> DependencyKeys = new(StringComparer.Ordinal)
    {
        "name", "comment", "fix", "severity", "expires", "owner", "since", "deprecated", "replacedBy", "allowEmpty", "because", "from", "to",
    };

    private RuleFile(IReadOnlyList<ElementRule> elements, IReadOnlyList<DependencyRule> forbidden, IReadOnlyList<Skipped> skipped, IReadOnlyList<string> assemblies)
    {
        Elements = elements;
        Forbidden = forbidden;
        Skipped = skipped;
        Assemblies = assemblies;
    }

    /// <summary>The file names of <c>languages.dotnet.assemblies</c>, the assemblies the rules judge; empty when the file names none.</summary>
    public IReadOnlyList<string> Assemblies { get; }

    /// <summary>Whether the rules judge the assembly named <paramref name="assemblyName"/>: every assembly when the file names none, else one whose file name matches.</summary>
    public bool Judges(string assemblyName) =>
        Assemblies.Count == 0 || Assemblies.Any(glob => System.Text.RegularExpressions.Regex.IsMatch(
            assemblyName + ".dll",
            "^" + System.Text.RegularExpressions.Regex.Escape(glob).Replace("\\*", ".*").Replace("\\?", ".") + "$",
            System.Text.RegularExpressions.RegexOptions.IgnoreCase | System.Text.RegularExpressions.RegexOptions.CultureInvariant));

    /// <summary>The element rules the analyzer evaluates.</summary>
    public IReadOnlyList<ElementRule> Elements { get; }

    /// <summary>The forbidden dependency rules the analyzer evaluates.</summary>
    public IReadOnlyList<DependencyRule> Forbidden { get; }

    /// <summary>The rules left to <c>cruise</c>.</summary>
    public IReadOnlyList<Skipped> Skipped { get; }

    /// <summary>Parses the text of a native rule file.</summary>
    /// <exception cref="RuleFileException">The text is not YAML, or a rule is malformed.</exception>
    public static RuleFile Parse(string text)
    {
        var stream = new YamlStream();
        try
        {
            stream.Load(new System.IO.StringReader(text));
        }
        catch (YamlDotNet.Core.YamlException e)
        {
            throw new RuleFileException("not YAML: " + e.Message);
        }
        if (stream.Documents.Count == 0 || stream.Documents[0].RootNode is not YamlMappingNode root)
        {
            throw new RuleFileException("the rule file is not a mapping");
        }
        var elements = new List<ElementRule>();
        var forbidden = new List<DependencyRule>();
        var skipped = new List<Skipped>();
        var rules = Child(root, "rules") as YamlMappingNode;
        var dependencies = rules is null ? null : Child(rules, "dependencies") as YamlMappingNode;
        foreach (var source in new[] { Child(root, "forbidden"), dependencies is null ? null : Child(dependencies, "forbidden") })
        {
            foreach (var rule in Items(source))
            {
                ReadDependency(rule, forbidden, skipped);
            }
        }
        foreach (var family in new[] { "allowed", "required" })
        {
            foreach (var rule in Items(Child(root, family)).Concat(Items(dependencies is null ? null : Child(dependencies, family))))
            {
                skipped.Add(new Skipped(NameOf(rule), family, $"`{family}` rules are evaluated over the whole graph"));
            }
        }
        if (rules is not null)
        {
            foreach (var rule in Items(Child(rules, "elements")))
            {
                ReadElement(rule, elements, skipped);
            }
            foreach (var family in new[] { "slices", "diagrams" })
            {
                foreach (var rule in Items(Child(rules, family)))
                {
                    skipped.Add(new Skipped(NameOf(rule), family, $"`{family}` rules are evaluated over the whole graph"));
                }
            }
        }
        var extended = Child(root, "extends") switch
        {
            YamlScalarNode one => [one.Value ?? string.Empty],
            YamlSequenceNode many => many.Children.OfType<YamlScalarNode>().Select(s => s.Value ?? string.Empty).ToList(),
            _ => new List<string>(),
        };
        foreach (var name in extended)
        {
            skipped.Add(new Skipped(name, "extends", "the rules of an extended configuration are read by the gate's loader"));
        }
        var languages = Child(root, "languages") as YamlMappingNode;
        var dotnet = languages is null ? null : Child(languages, "dotnet") as YamlMappingNode;
        var assemblies = (dotnet is null ? null : Child(dotnet, "assemblies")) is YamlSequenceNode listed
            ? listed.Children.OfType<YamlScalarNode>().Select(s => (s.Value ?? string.Empty).Replace('\\', '/').Split('/').Last()).ToList()
            : [];
        return new RuleFile(elements, forbidden, skipped, assemblies);
    }

    private static YamlNode? Child(YamlMappingNode map, string key) =>
        map.Children.TryGetValue(new YamlScalarNode(key), out var value) ? value : null;

    private static IEnumerable<YamlMappingNode> Items(YamlNode? node) =>
        node is YamlSequenceNode sequence ? sequence.Children.OfType<YamlMappingNode>() : [];

    private static string NameOf(YamlMappingNode rule) =>
        Child(rule, "name") is YamlScalarNode name ? name.Value ?? string.Empty : string.Empty;

    private static string? Text(YamlMappingNode map, string key) =>
        Child(map, key) is YamlScalarNode scalar ? scalar.Value : null;

    private static Severity ReadSeverity(YamlMappingNode rule, Severity fallback)
    {
        var text = Text(rule, "severity");
        return text switch
        {
            null => fallback,
            "error" => Severity.Error,
            "warn" => Severity.Warn,
            "info" => Severity.Info,
            "ignore" => Severity.Ignore,
            _ => throw new RuleFileException($"rule `{NameOf(rule)}`: severity `{text}` is not error, warn, info or ignore"),
        };
    }

    private static bool Bool(YamlNode? node, string context)
    {
        if (node is null)
        {
            return false;
        }
        if (node is YamlScalarNode { Value: var value } && bool.TryParse(value, out var result))
        {
            return result;
        }
        throw new RuleFileException($"{context} is true or false");
    }

    private static void ReadDependency(YamlMappingNode rule, List<DependencyRule> forbidden, List<Skipped> skipped)
    {
        var name = NameOf(rule);
        var extra = rule.Children.Keys.OfType<YamlScalarNode>().Select(k => k.Value ?? string.Empty).FirstOrDefault(k => !DependencyKeys.Contains(k));
        if (extra is not null)
        {
            skipped.Add(new Skipped(name, "forbidden", $"`{extra}` is evaluated over the whole graph"));
            return;
        }
        var from = Child(rule, "from") as YamlMappingNode;
        var to = Child(rule, "to") as YamlMappingNode;
        var fromCondition = from is null ? new PathCondition([], []) : PathOnly(from);
        var toCondition = to is null ? new PathCondition([], []) : PathOnly(to);
        if (fromCondition is null || toCondition is null)
        {
            skipped.Add(new Skipped(name, "forbidden", "a condition other than `path` and `pathNot` is evaluated over the whole graph"));
            return;
        }
        // Severity defaults to warn for a dependency rule, as in dependency-cruiser.
        var severity = ReadSeverity(rule, Severity.Warn);
        if (severity == Severity.Ignore)
        {
            return;
        }
        forbidden.Add(new DependencyRule(name, severity, Text(rule, "comment"), Text(rule, "fix"), fromCondition, toCondition));
    }

    private static PathCondition? PathOnly(YamlMappingNode side)
    {
        var keys = side.Children.Keys.OfType<YamlScalarNode>().Select(k => k.Value ?? string.Empty).ToList();
        if (keys.Any(k => k is not "path" and not "pathNot"))
        {
            return null;
        }
        return new PathCondition(Patterns(Child(side, "path")), Patterns(Child(side, "pathNot")));
    }

    private static List<JsPattern> Patterns(YamlNode? node) => node switch
    {
        null => [],
        YamlScalarNode scalar => [JsPattern.Compile(scalar.Value ?? string.Empty)],
        YamlSequenceNode list => list.Children.OfType<YamlScalarNode>().Select(s => JsPattern.Compile(s.Value ?? string.Empty)).ToList(),
        _ => throw new RuleFileException("a path is a pattern or a list of patterns"),
    };

    private static void ReadElement(YamlMappingNode rule, List<ElementRule> elements, List<Skipped> skipped)
    {
        var name = NameOf(rule);
        if (name.Length == 0)
        {
            throw new RuleFileException("an element rule has no name");
        }
        var extra = rule.Children.Keys.OfType<YamlScalarNode>().Select(k => k.Value ?? string.Empty).FirstOrDefault(k => !ElementKeys.Contains(k));
        if (extra is not null)
        {
            throw new RuleFileException($"element rule `{name}`: `{extra}` is not an element rule's key");
        }
        var severity = ReadSeverity(rule, Severity.Error);
        if (severity == Severity.Ignore)
        {
            return;
        }
        try
        {
            if (Child(rule, "select") is not YamlMappingNode select || Child(rule, "should") is not { } should)
            {
                throw new RuleFileException($"element rule `{name}` needs `select` and `should`");
            }
            var selector = ReadSelector(select, $"element rule `{name}`");
            var condition = ReadExpr(should, Side.Should, $"element rule `{name}`") ?? throw new RuleFileException($"element rule `{name}`: `should` is empty");
            elements.Add(new ElementRule(name, severity, Text(rule, "comment"), Text(rule, "fix"), Bool(Child(rule, "allowEmpty"), $"element rule `{name}`: allowEmpty"), selector, condition));
        }
        catch (NotEvaluatedException e)
        {
            skipped.Add(new Skipped(name, "elements", e.Message));
        }
    }

    private static Selector ReadSelector(YamlMappingNode select, string context)
    {
        var kindText = Text(select, "kind") ?? throw new RuleFileException($"{context}: a selector names its `kind`");
        var kind = kindText switch
        {
            "type" => SelectKind.Type,
            "class" => SelectKind.Class,
            "interface" => SelectKind.Interface,
            "attribute" => SelectKind.Attribute,
            _ => throw new NotEvaluatedException($"`kind: {kindText}` selects members or modules, which the analyzer does not read"),
        };
        foreach (var key in select.Children.Keys.OfType<YamlScalarNode>().Select(k => k.Value ?? string.Empty))
        {
            if (key is not "kind" and not "where" and not "language" and not "includeReferenced")
            {
                throw new RuleFileException($"{context}: `{key}` is not a selector's key");
            }
        }
        var languages = Child(select, "language") switch
        {
            null => [],
            YamlScalarNode one => [one.Value ?? string.Empty],
            YamlSequenceNode many => many.Children.OfType<YamlScalarNode>().Select(s => s.Value ?? string.Empty).ToList(),
            _ => throw new RuleFileException($"{context}: `language` is a name or a list of names"),
        };
        var where = Child(select, "where") switch
        {
            null => null,
            YamlMappingNode { Children.Count: 0 } => null,
            YamlSequenceNode { Children.Count: 0 } => null,
            var node => ReadExpr(node, Side.Where, context),
        };
        return new Selector(kind, where, languages, Bool(Child(select, "includeReferenced"), $"{context}: includeReferenced"));
    }

    /// <summary>Which side of a rule an expression is on: <c>where</c> (predicates) or <c>should</c> (conditions).</summary>
    internal enum Side
    {
        Where,
        Should,
    }

    private static Expr? ReadExpr(YamlNode node, Side side, string context)
    {
        switch (node)
        {
            case YamlSequenceNode list:
                if (list.Children.Count == 0)
                {
                    throw new RuleFileException($"{context}: an empty list is not a condition");
                }
                return new All(list.Children.Select(item => ReadExpr(item, side, context) ?? throw new RuleFileException($"{context}: an empty condition")).ToList());
            case YamlMappingNode map:
                if (map.Children.Count == 0)
                {
                    throw new RuleFileException($"{context}: an empty mapping is not a condition");
                }
                var parts = map.Children.Select(entry => ReadEntry(((YamlScalarNode)entry.Key).Value ?? string.Empty, entry.Value, side, context)).ToList();
                return parts.Count == 1 ? parts[0] : new All(parts);
            default:
                throw new RuleFileException($"{context}: a condition is a mapping or a list");
        }
    }

    private static Expr ReadEntry(string key, YamlNode value, Side side, string context)
    {
        switch (key)
        {
            case "all":
            case "any":
                if (value is not YamlSequenceNode { Children.Count: > 0 } items)
                {
                    throw new RuleFileException($"{context}: `{key}` is a non-empty list");
                }
                var parts = items.Children.Select(item => ReadExpr(item, side, context) ?? throw new RuleFileException($"{context}: an empty condition")).ToList();
                return key == "all" ? new All(parts) : new Any(parts);
            case "not":
                return new Not(ReadExpr(value, side, context) ?? throw new RuleFileException($"{context}: `not` needs a condition"));
            default:
                break;
        }
        var (name, negated, nested) = SplitKey(key, side);
        if (!Vocabulary.TryGetValue(name, out var entry))
        {
            throw new NotEvaluatedException($"`{key}` is outside what the analyzer evaluates");
        }
        if (nested && value is not YamlMappingNode)
        {
            throw new RuleFileException($"{context}: `{key}` takes a selector");
        }
        Operand operand;
        switch (entry.Kind)
        {
            case ValueKind.Flag:
                if (value is not YamlScalarNode { Value: var flag } || !bool.TryParse(flag, out var on))
                {
                    throw new RuleFileException($"{context}: `{key}` is true or false");
                }
                negated ^= !on;
                operand = new NoOperand();
                break;
            case ValueKind.Pattern:
                if (value is not YamlScalarNode { Value: { } pattern })
                {
                    throw new RuleFileException($"{context}: `{key}` is a pattern");
                }
                operand = new PatternOperand(JsPattern.Compile(pattern));
                break;
            case ValueKind.Objects when value is YamlMappingNode selector && Child(selector, "kind") is not null:
                operand = new SelectorOperand(ReadSelector(selector, context));
                break;
            default:
                operand = new Names(NamesOf(value, key, context));
                break;
        }
        return new Test(key, entry.Concept, negated, operand);
    }

    private static List<string> NamesOf(YamlNode value, string key, string context) => value switch
    {
        YamlScalarNode scalar => [Scalar(scalar)],
        YamlSequenceNode list => list.Children.Select(item => item is YamlScalarNode s ? Scalar(s) : throw new RuleFileException($"{context}: `{key}` holds names")).ToList(),
        _ => throw new RuleFileException($"{context}: `{key}` is a name or a list of names"),
    };

    /// <summary>A scalar as text, booleans written as .NET writes them (`True`, `False`), as rb-config does.</summary>
    private static string Scalar(YamlScalarNode scalar)
    {
        var value = scalar.Value ?? string.Empty;
        if (scalar.Style == YamlDotNet.Core.ScalarStyle.Plain && bool.TryParse(value, out var b))
        {
            return b ? "True" : "False";
        }
        return value;
    }

    /// <summary>rb-config's split_key: the concept's base name, whether the key negates it, and whether it takes a nested selector.</summary>
    internal static (string Name, bool Negated, bool Nested) SplitKey(string key, Side side)
    {
        string Lower(string rest) => rest.Length == 0 ? rest : char.ToLowerInvariant(rest[0]) + rest.Substring(1);
        string name;
        var negated = false;
        if (side == Side.Where)
        {
            if (key.StartsWith("areNot", StringComparison.Ordinal))
            {
                (name, negated) = (Lower(key.Substring(6)), true);
            }
            else if (key.StartsWith("doNot", StringComparison.Ordinal))
            {
                (name, negated) = (Lower(key.Substring(5)), true);
            }
            else if (key.StartsWith("are", StringComparison.Ordinal))
            {
                name = Lower(key.Substring(3));
            }
            else
            {
                name = key;
            }
        }
        else if (key.StartsWith("notBe", StringComparison.Ordinal))
        {
            (name, negated) = (Lower(key.Substring(5)), true);
        }
        else if (key.StartsWith("not", StringComparison.Ordinal))
        {
            (name, negated) = (Lower(key.Substring(3)), true);
        }
        else if (key.StartsWith("be", StringComparison.Ordinal))
        {
            name = Lower(key.Substring(2));
        }
        else
        {
            name = key;
        }
        var nested = false;
        foreach (var suffix in new[] { "TypesThat", "That" })
        {
            if (name.EndsWith(suffix, StringComparison.Ordinal))
            {
                name = name.Substring(0, name.Length - suffix.Length);
                nested = true;
                break;
            }
        }
        if (name is "types" or "methodMembers")
        {
            name = string.Empty;
        }
        return (name, negated, nested);
    }

    /// <summary>A rule that is valid but outside what the analyzer evaluates.</summary>
    private sealed class NotEvaluatedException : Exception
    {
        public NotEvaluatedException(string message)
            : base(message)
        {
        }
    }
}
