// A type's facts as the compiled-mode extractor would record them (crates/rb-extract-dotnet/src/codelayer.rs):
// metadata names, the kind table, raw flags, the base chain, interfaces and dependencies.
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 21.

namespace Rulebearing.Analyzer.Tests;

/// <summary>A type's facts as the compiled-mode extractor would record them.</summary>
public sealed class FactsTests
{
    private const string Shapes = """
        using System;
        using System.Collections.Generic;
        namespace App.Domain
        {
            public interface IShape { }
            public interface ISolid : IShape { }
            public abstract class Base : ISolid { }
            public class Square : Base, IEquatable<Square>
            {
                public bool Equals(Square? other) => other is not null;
                public class Inner { }
                internal sealed class Box<T> { }
            }
            public static class Helpers { }
            public struct Point { }
            public enum Color { Red }
            public delegate void Handler(int x);
            public sealed record Money(decimal Amount);
            public record struct Pair(int A, int B);
            public sealed class MarkAttribute : Attribute { }
        }
        public class Global { }
        """;

    /// <summary>Names follow metadata.</summary>
    [Fact]
    public void NamesFollowMetadata()
    {
        var universe = Support.Universe(Support.Compile(Shapes));
        var names = universe.Defined.Select(t => t.FullName).ToList();
        Assert.Contains("App.Domain.Square+Inner", names);
        Assert.Contains("App.Domain.Square+Box`1", names);
        var box = Support.Type(universe, "App.Domain.Square+Box`1");
        Assert.Equal("Box`1", box.Name);
        Assert.Equal("App.Domain", box.Namespace);
        Assert.True(box.Nested);
        Assert.Equal("internal", box.Visibility);
        var global = Support.Type(universe, "Global");
        Assert.Equal(string.Empty, global.Namespace);
        Assert.Equal("Sample", global.Assembly);
        Assert.StartsWith("Sample, Version=", global.AssemblyFullName, StringComparison.Ordinal);
        Assert.Equal(names.OrderBy(n => n, StringComparer.Ordinal), names);
    }

    /// <summary>Kinds and raw flags.</summary>
    [Theory]
    [InlineData("App.Domain.IShape", "interface", false, false, false, false)]
    [InlineData("App.Domain.Base", "class", true, false, false, false)]
    [InlineData("App.Domain.Helpers", "class", true, true, true, false)]
    [InlineData("App.Domain.Point", "struct", false, true, false, false)]
    [InlineData("App.Domain.Color", "enum", false, true, false, false)]
    [InlineData("App.Domain.Handler", "class", false, true, false, false)]
    [InlineData("App.Domain.Money", "class", false, true, false, true)]
    [InlineData("App.Domain.Pair", "struct", false, true, false, false)]
    [InlineData("App.Domain.MarkAttribute", "attribute", false, true, false, false)]
    public void KindsAndRawFlags(string fullName, string kind, bool isAbstract, bool isSealed, bool isStatic, bool isRecord)
    {
        var type = Support.Type(Support.Universe(Support.Compile(Shapes)), fullName);
        Assert.Equal(kind, type.Kind);
        Assert.Equal(isAbstract, type.Abstract);
        Assert.Equal(isSealed, type.Sealed);
        Assert.Equal(isStatic, type.Static);
        Assert.Equal(isRecord, type.Record);
    }

    /// <summary>Bases and interfaces are the extractors.</summary>
    [Fact]
    public void BasesAndInterfacesAreTheExtractors()
    {
        var universe = Support.Universe(Support.Compile(Shapes));
        var square = Support.Type(universe, "App.Domain.Square");
        Assert.Equal(["App.Domain.Base", "System.Object"], square.BaseTypes);
        Assert.Equal(["System.IEquatable`1", "App.Domain.ISolid", "App.Domain.IShape"], square.Interfaces);
        Assert.Empty(Support.Type(universe, "App.Domain.IShape").BaseTypes);
        Assert.Equal(["System.ValueType"], Support.Type(universe, "App.Domain.Point").BaseTypes);
    }

    /// <summary>Dependencies come from signatures attributes and bodies.</summary>
    [Fact]
    public void DependenciesComeFromSignaturesAttributesAndBodies()
    {
        var source = """
            using System;
            using System.Collections.Generic;
            using System.Linq;
            namespace App
            {
                public class Repo { public static Repo Open() => new Repo(); public int Count; }
                public class Item { }
                public class Tag : Attribute { public Tag(Type t) { } }
                public class Logger { public void Log(string s) { } }
                public class Error : Exception { }
                [Tag(typeof(Item))]
                public class Service
                {
                    private readonly Dictionary<string, List<Item>> _byName = new();
                    public Logger Logger { get; } = new Logger();
                    public int Run(object o)
                    {
                        var repo = Repo.Open();
                        Func<int, int> twice = x => x * repo.Count;
                        if (o is Error e) { return 0; }
                        var items = new Item[2];
                        Logger.Log(nameof(items));
                        return twice(items.Length);
                    }
                    public class Nested { public Uri? Address; }
                }
            }
            """;
        var universe = Support.Universe(Support.Compile(source));
        var service = Support.Type(universe, "App.Service");
        foreach (var expected in new[] { "App.Tag", "App.Item", "System.Collections.Generic.Dictionary`2", "System.Collections.Generic.List`1", "System.String", "App.Logger", "App.Repo", "System.Func`2", "App.Error", "System.Object", "System.Int32" })
        {
            Assert.Contains(expected, service.Dependencies);
        }
        Assert.DoesNotContain("System.Uri", service.Dependencies);
        Assert.Contains("System.Uri", Support.Type(universe, "App.Service+Nested").Dependencies);
        Assert.True(service.DependencyLocations.ContainsKey("App.Repo"));
        Assert.Contains(universe.Referenced, t => t.FullName == "System.Uri" && t.Kind == "class" && t.Referenced);
    }

    /// <summary>Bases and interfaces run on through an assembly the gate loads.</summary>
    [Fact]
    public void BasesAndInterfacesRunOnThroughAnAssemblyTheGateLoads()
    {
        var library = Support.CompileAssembly("Lib", [], ("lib/Base.cs", """
            namespace Lib
            {
                public interface IRequest<T> { }
                public abstract record CommandBase(System.Guid Id) : IRequest<int>;
            }
            """));
        var compilation = Support.CompileAssembly("Sample", [library.ToMetadataReference()], ("src/Command.cs", """
            namespace App { public sealed record Command(System.Guid Id) : Lib.CommandBase(Id); }
            """));
        var alone = Support.Type(RulebearingAnalyzer.Build(compilation), "App.Command");
        Assert.Equal(["Lib.CommandBase"], alone.BaseTypes);
        Assert.DoesNotContain("Lib.IRequest`1", alone.Interfaces);
        var universe = RulebearingAnalyzer.Build(compilation, name => name == "Lib");
        var command = Support.Type(universe, "App.Command");
        Assert.Equal(["Lib.CommandBase", "System.Object"], command.BaseTypes);
        Assert.Contains("Lib.IRequest`1", command.Interfaces);
        Assert.Contains("Lib.IRequest`1", command.Dependencies);
        Assert.Contains("Lib.IRequest`1", universe.Referenced.Single(t => t.FullName == "Lib.CommandBase").Interfaces);
        var rules = "languages: { dotnet: { assemblies: [Sample.dll, Lib.dll] } }\nrules:\n  elements:\n    - name: requests-are-classes\n      select: { kind: class, where: { implementInterface: [Lib.IRequest`1] } }\n      should: { not: { beRecord: true } }\n";
        Assert.Equal("App.Command", Assert.Single(Support.Analyze(compilation, rules)).Properties["to"]);
        Assert.Empty(Support.Analyze(compilation, rules.Replace(", Lib.dll", string.Empty, StringComparison.Ordinal)));
    }

    /// <summary>Immutability reads instance fields and setters.</summary>
    [Fact]
    public void ImmutabilityReadsInstanceFieldsAndSetters()
    {
        var source = """
            namespace App
            {
                public class Frozen { public readonly int A; public int B { get; } public int C { get; init; } public static int S; }
                public class Thawed { public int A; }
                public class Settable { public int B { get; set; } }
                public class Evented { public event System.Action? Changed; }
                public enum Level { Low }
            }
            """;
        var universe = Support.Universe(Support.Compile(source));
        Assert.True(Support.Type(universe, "App.Frozen").Immutable);
        Assert.False(Support.Type(universe, "App.Thawed").Immutable);
        Assert.False(Support.Type(universe, "App.Settable").Immutable);
        Assert.False(Support.Type(universe, "App.Evented").Immutable);
        Assert.False(Support.Type(universe, "App.Level").Immutable);
    }

    /// <summary>A type is attributed to a file its developer wrote.</summary>
    [Fact]
    public void ATypeIsAttributedToAFileItsDeveloperWrote()
    {
        var compilation = Support.Compile(
            ("obj/Generator/Square.g.cs", "namespace App { public partial class Square { public Square() { } } }"),
            ("src/Square.cs", "namespace App { public partial class Square { public int Corners() => 4; } }"));
        var square = Support.Type(Support.Universe(compilation), "App.Square");
        Assert.EndsWith("src/Square.cs", square.FilePath!.Replace('\\', '/'), StringComparison.Ordinal);
        Assert.True(Facts.IsBuildOutput("a/obj/b.cs"));
        Assert.False(Facts.IsBuildOutput("a/objects/b.cs"));
    }
}
