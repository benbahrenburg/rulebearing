// Rulebearing.Analyzer: the rules of rulebearing.yaml as compiler diagnostics.
//
// Source: docs/artifacts/design.md, "Two front-ends that will matter more than the MCP server".
// Requirement: docs/prd.md#fr-dist-04. Decision: docs/adr/0021-agent-surface-cli-first.md.
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 21, and its § 1.5
// (RB0001 a dependency-rule violation, RB0002 an element-rule violation, RB0009 the config could
// not be read; the message is the fix, or the comment when there is no fix; the rule name is the
// help link's fragment; error is an error, warn a warning, info a message).
//
// The rule file is the project's AdditionalFiles item named rulebearing.yaml (or .yml). Paths in
// the rules are relative to its folder, as `cruise` run from there reads them. Each compilation is
// judged on its own: an element rule's verdict for a type is the gate's, given the same facts, and
// a forbidden dependency rule fires for a reference from one file to a type declared in another.
// A rule the analyzer does not evaluate is left to `cruise` (README.md lists which).

using System.Collections.Immutable;
using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.Diagnostics;

namespace Rulebearing.Analyzer;

/// <summary>Reports the dependency and element rules of rulebearing.yaml as RB0001 and RB0002.</summary>
[DiagnosticAnalyzer(LanguageNames.CSharp)]
public sealed class RulebearingAnalyzer : DiagnosticAnalyzer
{
    /// <summary>A dependency rule's id.</summary>
    public const string DependencyId = "RB0001";

    /// <summary>An element rule's id.</summary>
    public const string ElementId = "RB0002";

    /// <summary>A rule the analyzer leaves to <c>cruise</c>.</summary>
    public const string LeftId = "RB0003";

    /// <summary>A rule file that cannot be read.</summary>
    public const string ConfigId = "RB0009";

    /// <summary>Where every diagnostic's help link points: the rule language.</summary>
    public const string HelpBase = "https://github.com/benbahrenburg/rulebearing/blob/main/docs/rules.md";

    private const string Category = "Architecture";

    private static readonly DiagnosticDescriptor Dependency = new(
        DependencyId, "A forbidden dependency", "{0}", Category, DiagnosticSeverity.Warning, isEnabledByDefault: true,
        description: "An import that a forbidden dependency rule of rulebearing.yaml does not allow.", helpLinkUri: HelpBase, customTags: WellKnownDiagnosticTags.CompilationEnd);

    private static readonly DiagnosticDescriptor Element = new(
        ElementId, "An element rule is broken", "{0}", Category, DiagnosticSeverity.Error, isEnabledByDefault: true,
        description: "A type that an element rule of rulebearing.yaml selects and that does not meet it.", helpLinkUri: HelpBase, customTags: WellKnownDiagnosticTags.CompilationEnd);

    private static readonly DiagnosticDescriptor Left = new(
        LeftId, "A rule is left to cruise", "Rule `{0}` is not checked at compile time: {1}; `rulebearing cruise` checks it", Category, DiagnosticSeverity.Info, isEnabledByDefault: true,
        description: "A rule of rulebearing.yaml the analyzer does not evaluate, because it needs the whole graph or a fact the analyzer does not read.", helpLinkUri: HelpBase, customTags: WellKnownDiagnosticTags.CompilationEnd);

    private static readonly DiagnosticDescriptor Config = new(
        ConfigId, "The rule file cannot be read", "rulebearing.yaml cannot be read: {0}", Category, DiagnosticSeverity.Error, isEnabledByDefault: true,
        description: "The analyzer reads the native rulebearing.yaml given as an AdditionalFiles item.", helpLinkUri: HelpBase, customTags: WellKnownDiagnosticTags.CompilationEnd);

    /// <inheritdoc />
    public override ImmutableArray<DiagnosticDescriptor> SupportedDiagnostics { get; } = ImmutableArray.Create(Dependency, Element, Left, Config);

    /// <inheritdoc />
    public override void Initialize(AnalysisContext context)
    {
        if (context is null)
        {
            throw new ArgumentNullException(nameof(context));
        }
        // A type a source generator writes is a type of the assembly, and the gate judges it: its
        // finding is reported where the generator put it.
        context.ConfigureGeneratedCodeAnalysis(GeneratedCodeAnalysisFlags.Analyze | GeneratedCodeAnalysisFlags.ReportDiagnostics);
        context.EnableConcurrentExecution();
        context.RegisterCompilationAction(Analyze);
    }

    private static void Analyze(CompilationAnalysisContext context)
    {
        var file = context.Options.AdditionalFiles.FirstOrDefault(f => Path.GetFileName(f.Path) is "rulebearing.yaml" or "rulebearing.yml");
        if (file is null)
        {
            return;
        }
        RuleFile rules;
        try
        {
            rules = RuleFile.Parse(file.GetText(context.CancellationToken)?.ToString() ?? string.Empty);
        }
        catch (Exception e) when (e is RuleFileException or PatternException)
        {
            context.ReportDiagnostic(Diagnostic.Create(Config, Location.None, e.Message));
            return;
        }
        // A rule file that names the assemblies it judges judges no other: a test project beside
        // them is not part of the gate's graph.
        if (!rules.Judges(context.Compilation.AssemblyName ?? string.Empty))
        {
            return;
        }
        Report(context, rules, Path.GetDirectoryName(Path.GetFullPath(file.Path)) ?? string.Empty);
    }

    private static void Report(CompilationAnalysisContext context, RuleFile rules, string root)
    {
        foreach (var skipped in rules.Skipped)
        {
            context.ReportDiagnostic(Diagnostic.Create(Left, Location.None, ImmutableDictionary<string, string?>.Empty.Add("rule", skipped.Name), skipped.Name, skipped.Reason));
        }
        if (rules.Elements.Count == 0 && rules.Forbidden.Count == 0)
        {
            return;
        }
        var universe = Build(context.Compilation, rules.Assemblies.Count == 0 ? null : rules.Judges);
        var evaluator = new ElementEvaluator(universe);
        foreach (var rule in rules.Elements)
        {
            foreach (var verdict in evaluator.Evaluate(rule).Where(v => !v.Passed))
            {
                var file = verdict.Type.FilePath is { } path ? Relative(root, path) : null;
                var id = ViolationId(rule.Name, file ?? verdict.Type.FullName, verdict.Type.FullName, ConditionKey(rule.Should));
                context.ReportDiagnostic(Create(Element, rule.Name, rule.Severity, Message(rule.Fix, rule.Comment, rule.Name), verdict.Type.Location ?? Location.None, id, verdict.Type.FullName));
            }
        }
        foreach (var (rule, location, to) in Forbidden(universe, rules.Forbidden, root))
        {
            context.ReportDiagnostic(Create(Dependency, rule.Name, rule.Severity, Message(rule.Fix, rule.Comment, rule.Name), location, null, to));
        }
    }

    /// <summary>The facts of every type the compilation defines, and of the types they depend on. A referenced assembly whose name <paramref name="loads"/> accepts is one the gate loads too, so the base chain and the interfaces run on through its types; with none, the chain stops at the compilation's edge.</summary>
    internal static Universe Build(Compilation compilation, Func<string, bool>? loads = null)
    {
        bool Loaded(IAssemblySymbol? assembly) =>
            assembly is not null && (SymbolEqualityComparer.Default.Equals(assembly, compilation.Assembly) || (loads is not null && loads(assembly.Identity.Name)));
        var targets = new Dictionary<string, INamedTypeSymbol>(StringComparer.Ordinal);
        var defined = Types(compilation.Assembly.GlobalNamespace)
            .Select(t => Facts.Of(t, compilation, local: true, Loaded, targets))
            .OrderBy(t => t.FullName, StringComparer.Ordinal)
            .ToList();
        var known = new HashSet<string>(defined.Select(t => t.FullName), StringComparer.Ordinal);
        var scratch = new Dictionary<string, INamedTypeSymbol>(StringComparer.Ordinal);
        var referenced = targets
            .Where(t => !known.Contains(t.Key))
            .Select(t => Facts.Of(t.Value, compilation, local: false, Loaded, scratch))
            .OrderBy(t => t.FullName, StringComparer.Ordinal)
            .ToList();
        return new Universe { Defined = defined, Referenced = referenced };
    }

    /// <summary>Every type a namespace holds, nested types included, compiler-generated ones not.</summary>
    private static IEnumerable<INamedTypeSymbol> Types(INamespaceSymbol ns)
    {
        foreach (var member in ns.GetMembers())
        {
            if (member is INamespaceSymbol inner)
            {
                foreach (var t in Types(inner))
                {
                    yield return t;
                }
            }
            else if (member is INamedTypeSymbol type)
            {
                foreach (var t in WithNested(type))
                {
                    yield return t;
                }
            }
        }
    }

    private static IEnumerable<INamedTypeSymbol> WithNested(INamedTypeSymbol type)
    {
        if (type.MetadataName.StartsWith("<", StringComparison.Ordinal) || !type.Locations.Any(l => l.IsInSource))
        {
            yield break;
        }
        yield return type;
        foreach (var nested in type.GetTypeMembers())
        {
            foreach (var t in WithNested(nested))
            {
                yield return t;
            }
        }
    }

    /// <summary>Every edge from one file to a type declared in another that a forbidden rule forbids, at the first line naming the type.</summary>
    internal static IEnumerable<(DependencyRule Rule, Location Location, string To)> Forbidden(Universe universe, IReadOnlyList<DependencyRule> rules, string root)
    {
        if (rules.Count == 0)
        {
            yield break;
        }
        var files = universe.Defined
            .Where(t => t.FilePath is not null)
            .ToDictionary(t => t.FullName, t => Relative(root, t.FilePath!), StringComparer.Ordinal);
        var seen = new HashSet<(string, string, string)>();
        foreach (var type in universe.Defined)
        {
            if (type.FilePath is null)
            {
                continue;
            }
            var from = Relative(root, type.FilePath);
            foreach (var target in type.Dependencies.OrderBy(t => t, StringComparer.Ordinal))
            {
                if (!files.TryGetValue(target, out var to) || to == from)
                {
                    continue;
                }
                foreach (var rule in rules)
                {
                    if (Forbids(rule, from, to) && seen.Add((rule.Name, from, to)))
                    {
                        var location = type.DependencyLocations.TryGetValue(target, out var at) ? at : type.Location ?? Location.None;
                        yield return (rule, location, to);
                    }
                }
            }
        }
    }

    /// <summary>Whether a forbidden rule matches an edge, <c>$1</c> in the target taking the source's capture, escaped.</summary>
    internal static bool Forbids(DependencyRule rule, string from, string to)
    {
        string[] groups = [];
        if (rule.From.Path.Count > 0)
        {
            var matched = rule.From.Path.FirstOrDefault(p => p.IsMatch(from));
            if (matched is null)
            {
                return false;
            }
            groups = matched.Groups(from);
        }
        if (rule.From.PathNot.Any(p => p.IsMatch(from)))
        {
            return false;
        }
        bool Test(JsPattern pattern) => groups.Length == 0 ? pattern.IsMatch(to) : JsPattern.Compile(Substitute(pattern.Source, groups)).IsMatch(to);
        if (rule.To.Path.Count > 0 && !rule.To.Path.Any(Test))
        {
            return false;
        }
        return !rule.To.PathNot.Any(Test);
    }

    /// <summary>dependency-cruiser's replaceGroupPlaceholders, each group escaped as a literal.</summary>
    internal static string Substitute(string pattern, IReadOnlyList<string> groups)
    {
        var result = pattern;
        for (var i = 0; i < groups.Count; i++)
        {
            var placeholder = "$" + i.ToString(CultureInfo.InvariantCulture);
            if (result.Contains(placeholder))
            {
                result = result.Replace(placeholder, EscapeJavaScript(groups[i]));
            }
        }
        return result;
    }

    private static string EscapeJavaScript(string text)
    {
        var builder = new StringBuilder(text.Length);
        foreach (var c in text)
        {
            if ("\\^$.*+?()[]{}|/-".IndexOf(c) >= 0)
            {
                builder.Append('\\');
            }
            builder.Append(c);
        }
        return builder.ToString();
    }

    /// <summary>A path relative to the rule file's folder, with forward slashes; the full path when it is outside.</summary>
    internal static string Relative(string root, string path)
    {
        var full = Path.GetFullPath(path).Replace('\\', '/');
        var prefix = root.Replace('\\', '/').TrimEnd('/') + "/";
        return full.StartsWith(prefix, StringComparison.Ordinal) ? full.Substring(prefix.Length) : full;
    }

    /// <summary>The message: the fix, else the comment, else the rule's name.</summary>
    internal static string Message(string? fix, string? comment, string name) =>
        !string.IsNullOrEmpty(fix) ? fix! : !string.IsNullOrEmpty(comment) ? comment! : name;

    /// <summary>The condition key the violation id uses: the first key of <c>should</c>, `not ` before a negation.</summary>
    internal static string ConditionKey(Expr should) => should switch
    {
        Test test => test.Key,
        All all => all.Items.Count > 0 ? ConditionKey(all.Items[0]) : string.Empty,
        Any any => any.Items.Count > 0 ? ConditionKey(any.Items[0]) : string.Empty,
        Not not => "not " + ConditionKey(not.Item),
        _ => string.Empty,
    };

    /// <summary>The stable violation id the gate gives the same finding: <c>RB-</c> and the first four bytes of SHA-256 over the rule, from, to and key (ADR-0015).</summary>
    internal static string ViolationId(string rule, string from, string to, string key)
    {
        using var sha = SHA256.Create();
        var hash = sha.ComputeHash(Encoding.UTF8.GetBytes(rule + "\n" + from + "\n" + to + "\n" + key));
        var hex = new StringBuilder("RB-");
        for (var i = 0; i < 4; i++)
        {
            hex.Append(hash[i].ToString("x2", CultureInfo.InvariantCulture));
        }
        return hex.ToString();
    }

    private static Diagnostic Create(DiagnosticDescriptor template, string rule, Severity severity, string message, Location location, string? id, string to)
    {
        var level = severity switch
        {
            Severity.Error => DiagnosticSeverity.Error,
            Severity.Warn => DiagnosticSeverity.Warning,
            _ => DiagnosticSeverity.Info,
        };
        var descriptor = new DiagnosticDescriptor(
            template.Id, template.Title, template.MessageFormat, template.Category, level, isEnabledByDefault: true,
            description: template.Description, helpLinkUri: HelpBase, customTags: WellKnownDiagnosticTags.CompilationEnd);
        var properties = ImmutableDictionary<string, string?>.Empty.Add("rule", rule).Add("to", to);
        if (id is not null)
        {
            properties = properties.Add("violationId", id);
        }
        return Diagnostic.Create(descriptor, location, level, additionalLocations: null, properties, message);
    }
}
