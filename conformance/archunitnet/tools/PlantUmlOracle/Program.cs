// Writes ArchUnitNET 0.13.4's own PlantUML output for the upstream tests that assert only a
// non-empty diagram, over the committed fixture assemblies, so the ported cases compare
// Rulebearing's builder with the text ArchUnitNET generates rather than with "not empty".
//
// Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md, Step 9.
// Decision: docs/adr/0009-conformance-suites-as-specification.md.
//
// Every selection is ordered by ordinal full name (types) or description (slices, namespaces)
// before it reaches the builder, because ArchUnitNET's own order is its loader's discovery order;
// the ported cases select the same objects in the same order. Output is written with "\n" line
// ends whatever the platform, as the Rust builder writes them.
using ArchUnitNET.Domain;
using ArchUnitNET.Domain.PlantUml.Export;
using ArchUnitNET.Fluent.Slices;
using ArchUnitNET.Loader;

if (args.Length != 2)
{
    Console.Error.WriteLine("usage: PlantUmlOracle <fixtures folder> <output folder>");
    return 2;
}
var fixtures = args[0];
var output = args[1];
Directory.CreateDirectory(output);

var architecture = new ArchLoader().LoadFilteredDirectory(fixtures, "ArchUnitNETTests.dll").Build();
var types = architecture.Types.OrderBy(t => t.FullName, StringComparer.Ordinal).ToList();
IEnumerable<Slice> Ordered(IEnumerable<Slice> slices) =>
    slices.OrderBy(s => s.Description, StringComparer.Ordinal);

// A type's dependencies come in ArchUnitNET's discovery order; the ported cases draw a type's
// targets in ordinal order of their full names, so each run of arrows from one origin is sorted
// here by its target.
string SortArrowsPerOrigin(string text)
{
    var lines = text.Split('\n').ToList();
    var start = 0;
    while (start < lines.Count)
    {
        var origin = Origin(lines[start]);
        if (origin is null)
        {
            start++;
            continue;
        }
        var end = start;
        while (end < lines.Count && Origin(lines[end]) == origin)
        {
            end++;
        }
        var run = lines
            .GetRange(start, end - start)
            .OrderBy(l => l[(l.IndexOf("] --|> [", StringComparison.Ordinal) + 8)..^1], StringComparer.Ordinal)
            .ToList();
        lines.RemoveRange(start, end - start);
        lines.InsertRange(start, run);
        start = end;
    }
    return string.Join("\n", lines);
}

string? Origin(string line)
{
    var at = line.IndexOf("] --|> [", StringComparison.Ordinal);
    return line.StartsWith('[') && at > 0 ? line[..at] : null;
}

void WriteTypes(string name, PlantUmlFileBuilder builder)
{
    WriteText(name, SortArrowsPerOrigin(builder.AsString().Replace("\r\n", "\n")));
}

void Write(string name, PlantUmlFileBuilder builder)
{
    WriteText(name, builder.AsString().Replace("\r\n", "\n"));
}

void WriteText(string name, string text)
{
    File.WriteAllText(Path.Combine(output, name + ".puml"), text);
    Console.WriteLine($"{name}: {text.Length} characters");
}

WriteTypes(
    "PlantUmlFileBuilderTest.BuildUmlByTypesTest",
    new PlantUmlFileBuilder().WithDependenciesFrom(types.Take(100))
);
WriteTypes(
    "PlantUmlFileBuilderTest.BuildUmlByTypesIncludingDependenciesToOtherTest",
    new PlantUmlFileBuilder().WithDependenciesFrom(
        types.Take(100),
        new GenerationOptions { IncludeDependenciesToOther = true }
    )
);
Write(
    "PlantUmlFileBuilderTest.BuildUmlByNamespacesTest",
    new PlantUmlFileBuilder().WithDependenciesFrom(Ordered(architecture.Namespaces))
);
Write(
    "PlantUmlFileBuilderTest.BuildUmlBySlicesTest",
    new PlantUmlFileBuilder().WithDependenciesFrom(
        Ordered(
            SliceRuleDefinition.Slices().Matching("ArchUnitNETTests.(*).").GetObjects(architecture)
        )
    )
);
// Upstream draws ArchUnitNET's own assembly, which is not a committed fixture; the ported case
// draws ArchUnitNETTests with the same options and a pattern of the same shape.
Write(
    "PlantUmlFluentComponentDiagramTests.ComponentDiagramFromSlicesTest",
    new PlantUmlFileBuilder().WithDependenciesFrom(
        Ordered(
            SliceRuleDefinition
                .Slices()
                .MatchingWithPackages("ArchUnitNETTests.(*).(*).(*)")
                .GetObjects(architecture)
        ),
        new GenerationOptions { C4Style = true, LimitDependencies = true }
    )
);
var focused = types.Where(t =>
    t.FullName == "ArchUnitNETTests.Fluent.PlantUml.PlantUmlFluentComponentDiagramTests"
);
WriteTypes(
    "PlantUmlFluentComponentDiagramTests.ComponentDiagramFromTypesTest",
    new PlantUmlFileBuilder().WithDependenciesFrom(
        focused,
        new GenerationOptions { IncludeDependenciesToOther = true }
    )
);

// One diagram per generation option and slice form beyond what the upstream tests draw, for
// crates/rb-rules/tests/plantuml_export.rs.
IEnumerable<Slice> Slices(string pattern, bool packages) =>
    Ordered(
        packages
            ? SliceRuleDefinition.Slices().MatchingWithPackages(pattern).GetObjects(architecture)
            : SliceRuleDefinition.Slices().Matching(pattern).GetObjects(architecture)
    );
Write(
    "Oracle.SlicesLimitDependencies",
    new PlantUmlFileBuilder().WithDependenciesFrom(
        Slices("ArchUnitNETTests.(*)", false),
        new GenerationOptions { LimitDependencies = true }
    )
);
Write(
    "Oracle.SlicesTwoAsterisks",
    new PlantUmlFileBuilder().WithDependenciesFrom(Slices("ArchUnitNETTests.(*).(*)", false))
);
Write(
    "Oracle.SlicesDoubleAsterisk",
    new PlantUmlFileBuilder().WithDependenciesFrom(Slices("ArchUnitNETTests.(**)", false))
);
Write(
    "Oracle.SlicesWithPackages",
    new PlantUmlFileBuilder().WithDependenciesFrom(Slices("ArchUnitNETTests.(*).(*).(*)", true))
);
Write(
    "Oracle.SlicesWithPackagesLimitDependencies",
    new PlantUmlFileBuilder().WithDependenciesFrom(
        Slices("ArchUnitNETTests.(*).(*).(*)", true),
        new GenerationOptions { LimitDependencies = true }
    )
);
Write(
    "Oracle.SlicesWithPackagesC4Style",
    new PlantUmlFileBuilder().WithDependenciesFrom(
        Slices("ArchUnitNETTests.(*).(*)", true),
        new GenerationOptions { C4Style = true }
    )
);
WriteTypes(
    "Oracle.TypesIgnoreDependenciesToChildrenAndParents",
    new PlantUmlFileBuilder().WithDependenciesFrom(
        types.Take(100),
        new GenerationOptions
        {
            DependencyFilter = DependencyFilters.IgnoreDependenciesToChildrenAndParents(),
        }
    )
);
var focus = types.Where(t => t.FullName.StartsWith("ArchUnitNETTests.Dependencies.", StringComparison.Ordinal));
WriteTypes(
    "Oracle.TypesFocusOn",
    new PlantUmlFileBuilder().WithDependenciesFrom(
        types.Take(100),
        new GenerationOptions
        {
            DependencyFilter = DependencyFilters.FocusOn(focus),
        }
    )
);
return 0;
