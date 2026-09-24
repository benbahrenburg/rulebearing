// The runner: how it finds its inputs and the binary, and that every run it cannot trust is an
// exception naming the cause and the fix, never a pass (docs/adr/0008-exit-code-contract.md,
// docs/adr/0007-vacuous-rules-fail-by-default.md).

using System.Runtime.Versioning;
using Rulebearing.TestAdapter.Fixture;
using Xunit;

namespace Rulebearing.TestAdapter.Tests;

/// <summary>Runs, saved results, and the failure modes of the runner.</summary>
public sealed class RulebearingRunTests
{
    /// <summary>A relative configuration is found above the test assembly, and the run happens once.</summary>
    [Fact]
    public void ARelativeConfigIsFoundAboveAndRunOnce()
    {
        RulebearingOptions options = new() { Config = FixtureRules.Config, Binary = TestBinary.Path };
        IReadOnlyList<RuleResult> first = RulebearingRun.Load(options);
        Assert.Equal(FixtureRules.Names, first.Select(static r => r.Name));
        Assert.Same(first, RulebearingRun.Load(options with { }));
        Assert.Throws<ArgumentNullException>(() => RulebearingRun.Load(null!));
    }

    /// <summary>A saved result is read without running the binary.</summary>
    [Fact]
    public void ASavedResultIsReadWithoutTheBinary()
    {
        using Scenario scenario = Scenario.WithConfig("unused: true\n");
        string saved = scenario.Write("result.json", """
            { "summary": { "violations": [], "ruleSetUsed": { "forbidden": [{ "name": "ok" }] } } }
            """);
        IReadOnlyList<RuleResult> results = RulebearingRun.Load(new RulebearingOptions { Result = saved, Binary = "/nowhere/rulebearing" });
        Assert.Equal("ok", Assert.Single(results).Name);
    }

    /// <summary>Without a configuration the binary finds one in the working directory.</summary>
    [Fact]
    public void WithoutAConfigTheBinaryLooksInTheWorkingDirectory()
    {
        string fixture = Path.GetDirectoryName(PathSearch.File(FixtureRules.Config, "configuration"))!;
        IReadOnlyList<RuleResult> results = RulebearingRun.Load(new RulebearingOptions
        {
            Config = null,
            WorkingDirectory = fixture,
            Binary = TestBinary.Path,
            Arguments = ["src"],
        });
        Assert.Equal(FixtureRules.FailingMessage, results[0].Message);
    }

    /// <summary>Parse refuses what is not a cruise result.</summary>
    /// <param name="json">The text.</param>
    [Theory]
    [InlineData("not json")]
    [InlineData("[]")]
    [InlineData("{\"modules\":[]}")]
    public void ParseRefusesWhatIsNotAResult(string json)
    {
        Assert.Throws<RulebearingException>(() => RulebearingRun.Parse(json));
        Assert.Throws<ArgumentNullException>(() => RulebearingRun.Parse(null!));
    }

    /// <summary>A missing input names itself and the fix.</summary>
    [Fact]
    public void AMissingInputNamesItself()
    {
        Assert.Contains("no-such.yaml", Assert.Throws<RulebearingException>(() => RulebearingRun.Load(new RulebearingOptions { Config = "no-such.yaml" })).Message, StringComparison.Ordinal);
        Assert.Contains("/no/such/graph.json", Assert.Throws<RulebearingException>(() => RulebearingRun.Load(new RulebearingOptions { Config = FixtureRules.Config, Graph = "/no/such/graph.json", Binary = TestBinary.Path })).Message, StringComparison.Ordinal);
        Assert.Contains("no-such-dir", Assert.Throws<RulebearingException>(() => RulebearingRun.Load(new RulebearingOptions { WorkingDirectory = "no-such-dir" })).Message, StringComparison.Ordinal);
        Assert.Contains("/nowhere/rulebearing", Assert.Throws<RulebearingException>(() => RulebearingRun.Load(new RulebearingOptions { Config = FixtureRules.Config, Binary = "/nowhere/rulebearing" })).Message, StringComparison.Ordinal);
        Assert.Contains("Binary option", Assert.Throws<RulebearingException>(() => RulebearingRun.Load(new RulebearingOptions { Config = FixtureRules.Config, Binary = "rulebearing-not-on-path" })).Message, StringComparison.Ordinal);
    }

    /// <summary>An invalid configuration (exit 3) is an exception with the binary's message.</summary>
    [Fact]
    public void AnInvalidConfigurationIsAnException()
    {
        using Scenario scenario = Scenario.WithConfig("forbidden: 12\n");
        RulebearingException e = Assert.Throws<RulebearingException>(() => RulebearingRun.Load(new RulebearingOptions { Config = scenario.Config, Binary = TestBinary.Path }));
        Assert.Contains("invalid (exit 3)", e.Message, StringComparison.Ordinal);
    }

    /// <summary>An untrusted run (exit 2) whose every rule passed is an exception, never a pass.</summary>
    [Fact]
    public void AnUntrustedRunWithNothingFailingIsAnException()
    {
        using Scenario scenario = Scenario.WithConfig("forbidden: []\n");
        RulebearingException e = Assert.Throws<RulebearingException>(() => RulebearingRun.Load(new RulebearingOptions { Config = scenario.Config, Binary = TestBinary.Path }));
        Assert.Contains("cannot be trusted (exit 2)", e.Message, StringComparison.Ordinal);
    }

    /// <summary>A binary that prints nothing, exits with another code, cannot start, or hangs.</summary>
    /// <remarks>The fake binaries are shell scripts, so this test runs on Linux and macOS; the CI job runs on Linux.</remarks>
    [Fact]
    [UnsupportedOSPlatform("windows")]
    public void ABinaryThatMisbehavesIsAnException()
    {
        using Scenario scenario = Scenario.WithConfig("forbidden: []\n");
        string Script(string name, string body, bool executable = true)
        {
            string path = scenario.Write(name, $"#!/bin/sh\n{body}\n");
            if (executable)
            {
                File.SetUnixFileMode(path, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
            }

            return path;
        }

        RulebearingException Run(string binary, TimeSpan? timeout = null) => Assert.Throws<RulebearingException>(
            () => RulebearingRun.Load(new RulebearingOptions { Config = scenario.Config, Binary = binary, Timeout = timeout ?? TimeSpan.FromMinutes(1) }));

        Assert.Contains("exited 0 without a result", Run(Script("silent", "exit 0")).Message, StringComparison.Ordinal);
        Assert.Contains("exited 5 without a result", Run(Script("five", "echo '{}'; echo broken >&2; exit 5")).Message, StringComparison.Ordinal);
        Assert.Contains("cannot be trusted (exit 2)", Run(Script("passing", "echo '{\"summary\":{}}'; exit 2")).Message, StringComparison.Ordinal);
        Assert.Contains("Could not start", Run(Script("plain", "exit 0", executable: false)).Message, StringComparison.Ordinal);
        Assert.Contains("did not finish", Run(Script("slow", "sleep 30"), TimeSpan.FromMilliseconds(300)).Message, StringComparison.Ordinal);
    }
}

/// <summary>Tests that change the process environment, run alone.</summary>
[CollectionDefinition(Name, DisableParallelization = true)]
public sealed class SerialEnvironment
{
    /// <summary>The collection's name.</summary>
    public const string Name = "environment";
}

/// <summary>The binary from RULEBEARING_BINARY, then PATH.</summary>
[Collection(SerialEnvironment.Name)]
public sealed class BinaryLookupTests
{
    /// <summary>RULEBEARING_BINARY names the binary; a wrong one is named in the error.</summary>
    [Fact]
    public void TheVariableNamesTheBinary()
    {
        string? saved = Environment.GetEnvironmentVariable(RulebearingOptions.BinaryVariable);
        try
        {
            Environment.SetEnvironmentVariable(RulebearingOptions.BinaryVariable, TestBinary.Path);
            Assert.Equal(TestBinary.Path, PathSearch.Binary(null));
            Environment.SetEnvironmentVariable(RulebearingOptions.BinaryVariable, "/nowhere/rulebearing");
            Assert.Contains(RulebearingOptions.BinaryVariable, Assert.Throws<RulebearingException>(() => PathSearch.Binary(null)).Message, StringComparison.Ordinal);
        }
        finally
        {
            Environment.SetEnvironmentVariable(RulebearingOptions.BinaryVariable, saved);
        }
    }

    /// <summary>Without the variable, the first rulebearing on PATH; with none, an error naming the fix.</summary>
    [Fact]
    public void PathIsSearchedLast()
    {
        string? savedBinary = Environment.GetEnvironmentVariable(RulebearingOptions.BinaryVariable);
        string? savedPath = Environment.GetEnvironmentVariable("PATH");
        string directory = Path.GetDirectoryName(TestBinary.Path)!;
        try
        {
            Environment.SetEnvironmentVariable(RulebearingOptions.BinaryVariable, null);
            Environment.SetEnvironmentVariable("PATH", string.Join(Path.PathSeparator, "/nowhere", directory));
            Assert.Equal(TestBinary.Path, PathSearch.Binary(null));
            Assert.Equal(TestBinary.Path, PathSearch.Binary(Path.GetFileName(TestBinary.Path)));
            Environment.SetEnvironmentVariable("PATH", "/nowhere");
            Assert.Contains("dotnet tool install -g Rulebearing", Assert.Throws<RulebearingException>(() => PathSearch.Binary(null)).Message, StringComparison.Ordinal);
            Assert.Null(PathSearch.OnPath("rulebearing"));
        }
        finally
        {
            Environment.SetEnvironmentVariable(RulebearingOptions.BinaryVariable, savedBinary);
            Environment.SetEnvironmentVariable("PATH", savedPath);
        }
    }
}
