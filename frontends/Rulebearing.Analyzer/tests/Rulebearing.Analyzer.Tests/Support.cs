// Helpers the tests share: a compilation from source, and the analyzer run over it with a rule file.
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 21.

using System.Collections.Immutable;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.Diagnostics;
using Microsoft.CodeAnalysis.Text;

namespace Rulebearing.Analyzer.Tests;

/// <summary>An additional file held in memory.</summary>
internal sealed class InMemoryText(string path, string text) : AdditionalText
{
    public override string Path { get; } = path;

    public override SourceText GetText(CancellationToken cancellationToken = default) => SourceText.From(text);
}

internal static class Support
{
    /// <summary>The folder the in-memory sources and the rule file sit in.</summary>
    public static readonly string Root = System.IO.Path.Combine(System.IO.Path.GetTempPath(), "rulebearing-analyzer-tests");

    /// <summary>A compilation of <paramref name="files"/>, each a (path relative to <see cref="Root"/>, source) pair.</summary>
    public static CSharpCompilation Compile(params (string Path, string Source)[] files)
    {
        var trees = files.Select(f => CSharpSyntaxTree.ParseText(f.Source, new CSharpParseOptions(LanguageVersion.Latest), System.IO.Path.Combine(Root, f.Path))).ToList();
        var references = ((string?)AppContext.GetData("TRUSTED_PLATFORM_ASSEMBLIES") ?? string.Empty)
            .Split(System.IO.Path.PathSeparator)
            .Where(p => p.Length > 0)
            .Select(p => MetadataReference.CreateFromFile(p));
        var compilation = CSharpCompilation.Create("Sample", trees, references, new CSharpCompilationOptions(OutputKind.DynamicallyLinkedLibrary));
        var errors = compilation.GetDiagnostics().Where(d => d.Severity == DiagnosticSeverity.Error).ToList();
        Assert.True(errors.Count == 0, string.Join("\n", errors));
        return compilation;
    }

    /// <summary>The single-file compilation of <paramref name="source"/> at <c>src/A.cs</c>.</summary>
    public static CSharpCompilation Compile(string source) => Compile(("src/A.cs", source));

    /// <summary>The diagnostics the analyzer reports for <paramref name="compilation"/> with <paramref name="rules"/> as rulebearing.yaml.</summary>
    public static ImmutableArray<Diagnostic> Analyze(Compilation compilation, string rules, string fileName = "rulebearing.yaml")
    {
        var options = new AnalyzerOptions([new InMemoryText(System.IO.Path.Combine(Root, fileName), rules)]);
        var analyzers = ImmutableArray.Create<DiagnosticAnalyzer>(new RulebearingAnalyzer());
        return compilation.WithAnalyzers(analyzers, options).GetAnalyzerDiagnosticsAsync().GetAwaiter().GetResult();
    }

    /// <summary>The facts of every type the compilation defines and depends on.</summary>
    public static Universe Universe(Compilation compilation) => RulebearingAnalyzer.Build(compilation);

    /// <summary>The facts of one defined type.</summary>
    public static TypeFacts Type(Universe universe, string fullName) =>
        universe.Defined.Single(t => t.FullName == fullName);
}
