// Reading rulebearing.yaml (rb-config's element-rule parsing for the evaluated subset) and
// evaluating it (rb-rules' concepts), then the analyzer end to end.
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 21.

using Microsoft.CodeAnalysis;

namespace Rulebearing.Analyzer.Tests;

/// <summary>Reading and evaluating rulebearing.yaml, and the analyzer end to end.</summary>
public sealed class RulesTests
{
    /// <summary>Keys split as rb-config splits them.</summary>
    [Theory]
    [InlineData("areNotSealed", "where", "sealed", true, false)]
    [InlineData("doNotHaveNameMatching", "where", "haveNameMatching", true, false)]
    [InlineData("areNestedIn", "where", "nestedIn", false, false)]
    [InlineData("resideInAssembly", "where", "resideInAssembly", false, false)]
    [InlineData("are", "where", "", false, false)]
    [InlineData("notBeSealed", "should", "sealed", true, false)]
    [InlineData("notDependOnAny", "should", "dependOnAny", true, false)]
    [InlineData("beSealed", "should", "sealed", false, false)]
    [InlineData("dependOnAnyTypesThat", "should", "dependOnAny", false, true)]
    [InlineData("beTypes", "should", "", false, false)]
    public void KeysSplitAsRbConfigSplitsThem(string key, string side, string name, bool negated, bool nested) =>
        Assert.Equal((name, negated, nested), RuleFile.SplitKey(key, side == "where" ? RuleFile.Side.Where : RuleFile.Side.Should));

    private const string Source = """
        namespace App.Domain { public class Order { } public sealed class Customer { } public class Uses { public App.Web.Page? P; } }
        namespace App.Web { public class Page { } }
        """;

    /// <summary>A flag set to false negates and unevaluated rules are named.</summary>
    [Fact]
    public void AFlagSetToFalseNegatesAndUnevaluatedRulesAreNamed()
    {
        var rules = RuleFile.Parse("""
            forbidden:
              - name: domain-not-to-web
                from: { path: "^src/domain" }
                to: { path: "^src/web" }
              - name: no-cycles
                from: {}
                to: { circular: true }
            allowed:
              - from: {}
                to: {}
            rules:
              elements:
                - name: unsealed
                  select: { kind: class }
                  should: { beSealed: false }
                - name: members
                  select: { kind: method }
                  should: { bePublic: true }
                - name: exotic
                  select: { kind: type }
                  should: { haveAttributeWithArguments: x }
              slices:
                - name: s
            """);
        var unsealed = Assert.Single(rules.Elements);
        var test = Assert.IsType<Test>(unsealed.Should);
        Assert.Equal((Concept.Sealed, true), (test.Concept, test.Negated));
        Assert.Equal("domain-not-to-web", Assert.Single(rules.Forbidden).Name);
        Assert.Equal(["no-cycles", "", "members", "exotic", "s"], rules.Skipped.Select(s => s.Name));
        Assert.All(rules.Skipped, s => Assert.False(string.IsNullOrEmpty(s.Reason)));
    }

    /// <summary>A malformed file is refused with the reason.</summary>
    [Theory]
    [InlineData("rules: [")]
    [InlineData("- a list")]
    [InlineData("rules:\n  elements:\n    - name: x\n      select: { kind: class }")]
    [InlineData("rules:\n  elements:\n    - name: x\n      select: { kind: class }\n      should: { beSealed: maybe }")]
    [InlineData("rules:\n  elements:\n    - name: x\n      severity: loud\n      select: { kind: class }\n      should: { beSealed: true }")]
    [InlineData("rules:\n  elements:\n    - name: x\n      select: { kind: class }\n      should: { haveNameMatching: \"a(?=b)\" }")]
    [InlineData("rules:\n  elements:\n    - name: x\n      examples: []\n      select: { kind: class }\n      should: { beSealed: true }")]
    [InlineData("rules:\n  elements:\n    - name: x\n      select: { kind: class }\n      should: { all: [] }")]
    public void AMalformedFileIsRefusedWithTheReason(string text) =>
        Assert.ThrowsAny<Exception>(() => RuleFile.Parse(text));

    private static List<string> Failing(string rule)
    {
        var rules = RuleFile.Parse("rules:\n  elements:\n" + rule);
        var evaluator = new ElementEvaluator(Support.Universe(Support.Compile(Source)));
        return evaluator.Evaluate(Assert.Single(rules.Elements)).Where(v => !v.Passed).Select(v => v.Type.FullName).ToList();
    }

    /// <summary>Concepts evaluate as the gate evaluates them.</summary>
    [Fact]
    public void ConceptsEvaluateAsTheGateEvaluatesThem()
    {
        Assert.Equal(["App.Domain.Order", "App.Domain.Uses"], Failing("""
                - name: sealed-domain
                  select: { kind: class, where: { resideInNamespace: app.domain } }
                  should: { beSealed: true }
            """));
        Assert.Equal(["App.Domain.Uses"], Failing("""
                - name: domain-off-web
                  select: { kind: type, where: { haveFullNameStartingWith: APP.DOMAIN } }
                  should: { notDependOnAny: { kind: type, where: { resideInNamespaceMatching: "^App\\.Web$" } } }
            """));
        Assert.Empty(Failing("""
                - name: names
                  select: { kind: class, where: { any: [{ haveNameEndingWith: er }, { haveName: PAGE }] } }
                  should: { all: [{ bePublic: true }, { not: { beNested: true } }, { haveFullNameMatching: "^App\\." }] }
            """));
        Assert.Equal(["App.Domain.Order"], Failing("""
                - name: identity
                  select: { kind: class, where: { resideInAssembly: sample } }
                  should: { notBe: [App.Domain.Order] }
            """));
    }

    /// <summary>The analyzer reports an element rule at the type with its fix.</summary>
    [Fact]
    public void TheAnalyzerReportsAnElementRuleAtTheTypeWithItsFix()
    {
        var compilation = Support.Compile(("src/domain/Order.cs", "namespace App.Domain { public class Order { } public sealed class Fine { } }"));
        var diagnostics = Support.Analyze(compilation, """
            rules:
              elements:
                - name: sealed-domain
                  comment: "Domain types are final (adr:0010)"
                  fix: Seal the class.
                  select: { kind: class }
                  should: { beSealed: true }
            """);
        var diagnostic = Assert.Single(diagnostics);
        Assert.Equal(RulebearingAnalyzer.ElementId, diagnostic.Id);
        Assert.Equal("Seal the class.", diagnostic.GetMessage(System.Globalization.CultureInfo.InvariantCulture));
        Assert.Equal(DiagnosticSeverity.Error, diagnostic.Severity);
        Assert.Equal(RulebearingAnalyzer.HelpBase, diagnostic.Descriptor.HelpLinkUri);
        Assert.Equal("App.Domain.Order", diagnostic.Properties["to"]);
        Assert.Equal(RulebearingAnalyzer.ViolationId("sealed-domain", "src/domain/Order.cs", "App.Domain.Order", "beSealed"), diagnostic.Properties["violationId"]);
        Assert.Equal(0, diagnostic.Location.GetLineSpan().StartLinePosition.Line);
    }

    /// <summary>The analyzer reports a forbidden dependency at the line that names it.</summary>
    [Fact]
    public void TheAnalyzerReportsAForbiddenDependencyAtTheLineThatNamesIt()
    {
        var compilation = Support.Compile(
            ("src/domain/Order.cs", "namespace App.Domain\n{\n    public class Order\n    {\n        public App.Web.Page? Page;\n    }\n}"),
            ("src/web/Page.cs", "namespace App.Web { public class Page { public App.Domain.Order? Order; } }"));
        var diagnostics = Support.Analyze(compilation, """
            forbidden:
              - name: domain-not-to-web
                severity: warn
                comment: The domain stays independent of the web layer
                from: { path: "^src/(domain)/" }
                to: { path: "^src/web/", pathNot: "^src/$1/" }
            """);
        var diagnostic = Assert.Single(diagnostics);
        Assert.Equal(RulebearingAnalyzer.DependencyId, diagnostic.Id);
        Assert.Equal(DiagnosticSeverity.Warning, diagnostic.Severity);
        Assert.Equal("The domain stays independent of the web layer", diagnostic.GetMessage(System.Globalization.CultureInfo.InvariantCulture));
        Assert.Equal("src/web/Page.cs", diagnostic.Properties["to"]);
        Assert.Equal(4, diagnostic.Location.GetLineSpan().StartLinePosition.Line);
    }

    /// <summary>An unreadable rule file is rb 0 0 0 9 and no rule file is silence.</summary>
    [Fact]
    public void AnUnreadableRuleFileIsRb0009AndNoRuleFileIsSilence()
    {
        var compilation = Support.Compile("namespace App { public class A { } }");
        var refused = Assert.Single(Support.Analyze(compilation, "rules: ["));
        Assert.Equal(RulebearingAnalyzer.ConfigId, refused.Id);
        Assert.Empty(Support.Analyze(compilation, "rules: [", fileName: "other.yaml"));
        Assert.Empty(Support.Analyze(compilation, "rules: {}"));
        var left = Assert.Single(Support.Analyze(compilation, "forbidden:\n  - name: no-cycles\n    from: {}\n    to: { circular: true }\n"));
        Assert.Equal((RulebearingAnalyzer.LeftId, DiagnosticSeverity.Info, "no-cycles"), (left.Id, left.Severity, left.Properties["rule"]));
        Assert.Contains("a condition other than `path` and `pathNot`", left.GetMessage(System.Globalization.CultureInfo.InvariantCulture), StringComparison.Ordinal);
        var extended = Support.Analyze(compilation, "extends: [rulebearing:recommended, ./base.yaml]\n");
        Assert.Equal(["rulebearing:recommended", "./base.yaml"], extended.Select(d => d.Properties["rule"]));
        Assert.Single(RuleFile.Parse("extends: ./base.yaml").Skipped);
    }

    /// <summary>A rule file that names its assemblies judges only those.</summary>
    [Fact]
    public void ARuleFileThatNamesItsAssembliesJudgesOnlyThose()
    {
        var rules = RuleFile.Parse("""
            languages:
              dotnet:
                assemblies:
                  - "src/App/bin/**/Sample.dll"
                  - 'src\Other\bin\Other.*.dll'
            """);
        Assert.Equal(["Sample.dll", "Other.*.dll"], rules.Assemblies);
        Assert.True(rules.Judges("Sample"));
        Assert.True(rules.Judges("other.core"));
        Assert.False(rules.Judges("Sample.Tests"));
        Assert.True(RuleFile.Parse("rules: {}").Judges("Anything"));
        var compilation = Support.Compile("namespace App { public class Open { } }");
        const string Elements = "\n  elements:\n    - name: sealed\n      select: { kind: class }\n      should: { beSealed: true }\n";
        Assert.Single(Support.Analyze(compilation, "languages: { dotnet: { assemblies: [Sample.dll] } }\nrules:" + Elements));
        Assert.Empty(Support.Analyze(compilation, "languages: { dotnet: { assemblies: [Other.dll] } }\nrules:" + Elements));
    }

    /// <summary>The message is the fix else the comment else the name.</summary>
    [Theory]
    [InlineData("fix", "comment", "fix")]
    [InlineData(null, "comment", "comment")]
    [InlineData(null, null, "rule")]
    [InlineData("", "", "rule")]
    public void TheMessageIsTheFixElseTheCommentElseTheName(string? fix, string? comment, string expected) =>
        Assert.Equal(expected, RulebearingAnalyzer.Message(fix, comment, "rule"));

    /// <summary>The violation id is the gates fixed vector.</summary>
    [Fact]
    public void TheViolationIdIsTheGatesFixedVector() =>
        // crates/rb-model/src/violation_id.rs: RB- and the first four bytes of SHA-256(rule\nfrom\nto\nkey).
        Assert.Matches("^RB-[0-9a-f]{8}$", RulebearingAnalyzer.ViolationId("r", "a.cs", "A", "beSealed"));

    /// <summary>Placeholders are substituted escaped.</summary>
    [Fact]
    public void PlaceholdersAreSubstitutedEscaped() =>
        Assert.Equal(@"^src/a\.b/", RulebearingAnalyzer.Substitute("^src/$1/", ["src/a.b", "a.b"]));
}
