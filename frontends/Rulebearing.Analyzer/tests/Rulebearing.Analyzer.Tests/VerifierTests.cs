// The analyzer through Microsoft.CodeAnalysis.Testing's verifier: each diagnostic at the span the
// source marks, with its id, severity and message, and nothing else reported.
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 21 ("analyzer unit
// tests with Microsoft.CodeAnalysis.Testing").

using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp.Testing;
using Microsoft.CodeAnalysis.Testing;

namespace Rulebearing.Analyzer.Tests;

/// <summary>The analyzer through the Roslyn testing verifier.</summary>
public sealed class VerifierTests
{
    private static CSharpAnalyzerTest<RulebearingAnalyzer, DefaultVerifier> Test(string rules, params (string Path, string Source)[] sources)
    {
        // netstandard2.0's reference assemblies come from NETStandard.Library, which the analyzer's
        // own restore has already put in the package cache.
        var test = new CSharpAnalyzerTest<RulebearingAnalyzer, DefaultVerifier> { ReferenceAssemblies = ReferenceAssemblies.NetStandard.NetStandard20 };
        foreach (var (path, source) in sources)
        {
            test.TestState.Sources.Add((path, source));
        }
        test.TestState.AdditionalFiles.Add(("/0/rulebearing.yaml", rules));
        return test;
    }

    /// <summary>An element rule is reported at the type that breaks it, with its fix.</summary>
    [Fact]
    public async Task AnElementRuleIsReportedAtTheTypeThatBreaksIt()
    {
        var test = Test("""
            rules:
              elements:
                - name: sealed-domain
                  fix: Seal the class.
                  select: { kind: class, where: { resideInNamespace: App.Domain } }
                  should: { beSealed: true }
            """, ("/0/src/domain/Order.cs", "namespace App.Domain { public class {|#0:Order|} { } public sealed class Fine { } }"));
        test.ExpectedDiagnostics.Add(new DiagnosticResult(RulebearingAnalyzer.ElementId, DiagnosticSeverity.Error).WithLocation(0).WithMessage("Seal the class."));
        await test.RunAsync();
    }

    /// <summary>A forbidden dependency is reported at the member that names the type, at the rule's severity.</summary>
    [Fact]
    public async Task AForbiddenDependencyIsReportedAtTheMemberThatNamesTheType()
    {
        var test = Test("""
            forbidden:
              - name: domain-not-to-web
                comment: The domain stays independent of the web layer
                from: { path: "^src/domain/" }
                to: { path: "^src/web/" }
            """,
            ("/0/src/domain/Order.cs", "namespace App.Domain { public class Order { public App.Web.Page? {|#0:Page|}; } }"),
            ("/0/src/web/Page.cs", "namespace App.Web { public class Page { public App.Domain.Order? Order; } }"));
        test.ExpectedDiagnostics.Add(new DiagnosticResult(RulebearingAnalyzer.DependencyId, DiagnosticSeverity.Warning).WithLocation(0).WithMessage("The domain stays independent of the web layer"));
        await test.RunAsync();
    }

    /// <summary>A rule left to cruise is named, and an unreadable file is refused.</summary>
    [Fact]
    public async Task ARuleLeftToCruiseIsNamedAndAnUnreadableFileIsRefused()
    {
        const string Source = "namespace App { public class A { } }";
        var left = Test("forbidden:\n  - name: no-cycles\n    from: {}\n    to: { circular: true }\n", ("/0/src/A.cs", Source));
        left.ExpectedDiagnostics.Add(new DiagnosticResult(RulebearingAnalyzer.LeftId, DiagnosticSeverity.Info).WithArguments("no-cycles", "a condition other than `path` and `pathNot` is evaluated over the whole graph"));
        await left.RunAsync();
        var refused = Test("rules: [", ("/0/src/A.cs", Source));
        refused.ExpectedDiagnostics.Add(new DiagnosticResult(RulebearingAnalyzer.ConfigId, DiagnosticSeverity.Error));
        await refused.RunAsync();
    }
}
