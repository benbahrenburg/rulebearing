// Mini-repositories written to a temporary directory for the equality proof and the runner tests:
// every kind of junit test case (a failure with and without a fix, more than five violations, warn
// and known findings, an expired rule and known violation, a warned and a strict vacuous rule, the
// allowed list, a required rule, a ratchet over its ceiling and one without a budget, element
// violations with and without a declaration line).

using System.Text.Json;

namespace Rulebearing.TestAdapter.Tests;

/// <summary>A mini-repository in a temporary directory, deleted on dispose.</summary>
internal sealed class Scenario : IDisposable
{
    private Scenario(string directory)
    {
        Directory = directory;
    }

    /// <summary>The repository's root.</summary>
    public string Directory { get; }

    /// <summary>The configuration file.</summary>
    public string Config => Path.Combine(Directory, "rulebearing.yaml");

    /// <summary>
    /// Every kind of dependency-rule test case. Run with <c>--liveness warn</c>, it exits 2 for the
    /// expired entries and the ratchets, with every cause in the result.
    /// </summary>
    public static Scenario KitchenSink()
    {
        Scenario scenario = Create("kitchen-sink");
        for (int i = 1; i <= 7; i++)
        {
            scenario.Write($"src/a/a{i}.ts", $"import {{ b }} from '../b/b.js';\n\nexport const a{i} = b;\n");
        }

        scenario.Write("src/b/b.ts", "export const b = 1;\n");
        scenario.Write("src/main.ts", "import { a1 } from './a/a1.js';\n\nexport const main = a1;\n");
        scenario.Write("budgets/a-b.json", "{\"ceiling\":1}\n");
        scenario.Write("rulebearing.yaml", """
            forbidden:
              - name: a-not-to-b
                severity: error
                from: { path: "^src/a/" }
                to: { path: "^src/b/" }
              - name: main-not-to-a
                severity: warn
                fix: "Import through the index."
                from: { path: "^src/main" }
                to: { path: "^src/a/" }
              - name: old-rule
                severity: error
                owner: "@me"
                expires: "2020-01-01"
                from: { path: "^src/b/" }
                to: { path: "^src/a/" }
              - name: dead-warned
                severity: info
                from: { path: "^nowhere/" }
                to: { path: "^src/" }
            required:
              - name: main-reaches-b
                severity: error
                module: { path: "^src/main\\.ts$" }
                to: { path: "^src/b/", reachable: true }
            allowed:
              - from: { path: "^src/" }
                to: { path: "^src/b/" }
                fix: "Only b may be imported."
            allowedSeverity: error
            rules:
              ratchets:
                - name: a-b-edges
                  from: { path: "^src/a/" }
                  to: { path: "^src/b/" }
                  budget: budgets/a-b.json
                - name: lost-budget
                  from: { path: "^src/a/" }
                  to: { path: "^src/b/" }
                  budget: budgets/missing.json
            options:
              knownViolations:
                - type: dependency
                  from: src/a/a6.ts
                  to: src/b/b.ts
                  rule: { name: a-not-to-b, severity: error }
                - type: dependency
                  from: src/main.ts
                  to: src/a/a1.ts
                  rule: { name: main-not-to-a, severity: warn }
                  id: RB-bf86204f
                  expires: "2020-01-01"
                  owner: "@me"
            """);
        return scenario;
    }

    /// <summary>
    /// An element rule over a graph document (two unsealed classes, one with a declaration line)
    /// and a rule that matches nothing, which strict liveness fails with exit 2.
    /// </summary>
    public static Scenario ElementsOverGraph()
    {
        Scenario scenario = Create("elements");
        scenario.Write("rulebearing.yaml", """
            rules:
              elements:
                - name: classes-are-sealed
                  fix: Seal the class.
                  severity: error
                  select: { kind: class }
                  should: { beSealed: true }
              dependencies:
                forbidden:
                  - name: nothing-matches
                    severity: error
                    from: { path: "^nowhere/" }
                    to: { path: "^src/" }
            """);
        object Module(string source) => new { source, dependencies = Array.Empty<object>(), dependents = Array.Empty<object>(), orphan = true, valid = true, language = "dotnet" };
        var graph = new
        {
            modules = new[] { Module("src/A.cs"), Module("src/B.cs") },
            summary = new { violations = Array.Empty<object>(), error = 0, warn = 0, info = 0, ignore = 0, totalCruised = 2, totalDependenciesCruised = 0, optionsUsed = new { } },
            code = new
            {
                types = new object[]
                {
                    new { fullName = "S.A", name = "A", @namespace = "S", kind = "class", language = "dotnet", file = "src/A.cs", @sealed = false, line = 3, column = 14 },
                    new { fullName = "S.B", name = "B", @namespace = "S", kind = "class", language = "dotnet", file = "src/B.cs", @sealed = false },
                },
            },
        };
        scenario.Write("graph.json", JsonSerializer.Serialize(graph));
        return scenario;
    }

    /// <summary>An empty directory with a configuration.</summary>
    /// <param name="config">The configuration's text.</param>
    public static Scenario WithConfig(string config)
    {
        Scenario scenario = Create("config");
        scenario.Write("rulebearing.yaml", config);
        return scenario;
    }

    /// <summary>Writes <paramref name="text"/> to <paramref name="relative"/> under the root.</summary>
    /// <param name="relative">The file, relative to the root.</param>
    /// <param name="text">Its content.</param>
    /// <returns>The file's full path.</returns>
    public string Write(string relative, string text)
    {
        string path = Path.Combine(Directory, relative);
        System.IO.Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, text.Replace("\r\n", "\n", StringComparison.Ordinal));
        return path;
    }

    /// <inheritdoc />
    public void Dispose()
    {
        try
        {
            System.IO.Directory.Delete(Directory, recursive: true);
        }
        catch (IOException)
        {
            // A temporary directory left behind is not a test failure.
        }
    }

    private static Scenario Create(string name)
    {
        string directory = Path.Combine(Path.GetTempPath(), $"rb-testadapter-{name}-{Guid.NewGuid():N}");
        System.IO.Directory.CreateDirectory(directory);
        return new Scenario(directory);
    }
}
