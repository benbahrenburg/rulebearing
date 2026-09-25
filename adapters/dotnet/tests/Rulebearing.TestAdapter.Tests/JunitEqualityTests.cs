// The message-equality proof: for the same inputs, every RuleResult the adapter yields from
// `rulebearing cruise -T json` carries exactly the text `rulebearing cruise -T junit` writes for
// that rule: the test case names and order, each <failure> message and body, each <error> type and
// message, and <system-out>. Contract: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md,
// section 1.5 ("the test adapters' message is this text") and Step 14.

using System.Diagnostics;
using System.Xml.Linq;
using Rulebearing.TestAdapter.Fixture;
using Xunit;

namespace Rulebearing.TestAdapter.Tests;

/// <summary>The adapter's messages are the <c>junit</c> reporter's, byte for byte.</summary>
public sealed class JunitEqualityTests
{
    private static readonly string[] JunitArguments = ["cruise", "--config", "rulebearing.yaml", "--output-type", "junit", "--output-to", "-", "--no-progress"];

    /// <summary>The scenarios the proof runs over.</summary>
    public static TheoryData<string> Scenarios => new() { "fixture", "kitchen-sink", "elements", "unnamed" };

    /// <summary>Every rule's result equals the junit test case for the same rule.</summary>
    /// <param name="name">The scenario.</param>
    [Theory]
    [MemberData(nameof(Scenarios))]
    public void EveryMessageIsTheJunitText(string name)
    {
        Scenario? scenario = name switch
        {
            "kitchen-sink" => Scenario.KitchenSink(),
            "elements" => Scenario.ElementsOverGraph(),
            "unnamed" => Scenario.Unnamed(),
            _ => null,
        };
        try
        {
            string config = scenario?.Config ?? Path.GetFullPath(Path.Combine(FindAbove("fixture"), "rulebearing.yaml"));
            string directory = Path.GetDirectoryName(config)!;
            string[] extra = name switch
            {
                "kitchen-sink" => ["--liveness", "warn"],
                "elements" => ["--graph", Path.Combine(directory, "graph.json")],
                _ => [],
            };
            IReadOnlyList<RuleResult> results = RulebearingRun.Load(new RulebearingOptions
            {
                Config = config,
                Binary = TestBinary.Path,
                Arguments = name == "kitchen-sink" ? extra : [],
                Graph = name == "elements" ? Path.Combine(directory, "graph.json") : null,
            });
            XDocument junit = XDocument.Parse(RunJunit(directory, extra));
            List<XElement> cases = [.. junit.Descendants("testcase")];

            Assert.Equal(cases.Select(static c => (string?)c.Attribute("name")), results.Select(static r => r.Name));
            Assert.Equal(cases.Select(static c => (string?)c.Attribute("classname")), results.Select(static r => $"rulebearing.{r.Family}"));
            for (int i = 0; i < cases.Count; i++)
            {
                XElement testCase = cases[i];
                RuleResult result = results[i];
                XElement? failure = testCase.Element("failure");
                Assert.Equal((string?)failure?.Attribute("message"), result.FailureMessage);
                Assert.Equal(failure?.Value, result.FailureDetail);
                Assert.Equal(
                    testCase.Elements("error").Select(static e => ((string?)e.Attribute("type"), (string?)e.Attribute("message"))),
                    result.Errors.Select(static e => ((string?)e.Kind, (string?)e.Message)));
                Assert.Equal(testCase.Element("system-out")?.Value ?? string.Empty, string.Join('\n', result.Output));
                string? expected = failure is null && !testCase.Elements("error").Any()
                    ? null
                    : string.Join('\n', new[] { (string?)failure?.Attribute("message") }
                        .Concat(testCase.Elements("error").Select(static e => (string?)e.Attribute("message")))
                        .OfType<string>());
                Assert.Equal(expected, result.Message);
            }

            if (name == "unnamed")
            {
                Assert.Equal(["unnamed", "unnamed#2", "unnamed#3"], results.Select(static r => r.Name));
                Assert.Equal(RuleOutcome.Failed, results[0].Outcome);
                Assert.EndsWith("\n... and 2 more", results[0].FailureMessage, StringComparison.Ordinal);
                Assert.Equal(RuleOutcome.Passed, results[1].Outcome);
                Assert.Single(results[1].Output);
                Assert.Equal(RuleOutcome.Passed, results[2].Outcome);
                Assert.False(File.Exists(Path.Combine(directory, "must-not-be-written.txt")));
            }
            else if (name == "fixture")
            {
                Assert.Equal(FixtureRules.Names, results.Select(static r => r.Name));
                Assert.Equal(FixtureRules.FailingMessage, results[0].Message);
            }
            else
            {
                Assert.Contains(results, static r => r.Outcome == RuleOutcome.Error);
                Assert.Contains(results, static r => r.Outcome == RuleOutcome.Failed);
            }
        }
        finally
        {
            scenario?.Dispose();
        }
    }

    /// <summary>A vacuous rule fails with the liveness reason (ADR-0007).</summary>
    [Fact]
    public void AVacuousRuleFailsWithTheLivenessReason()
    {
        using Scenario scenario = Scenario.ElementsOverGraph();
        IReadOnlyList<RuleResult> results = RulebearingRun.Load(new RulebearingOptions
        {
            Config = scenario.Config,
            Binary = TestBinary.Path,
            Graph = Path.Combine(scenario.Directory, "graph.json"),
        });
        RuleResult vacuous = Assert.Single(results, static r => r.Name == "nothing-matches");
        Assert.Equal(RuleOutcome.Error, vacuous.Outcome);
        RuleFailedException failure = Assert.Throws<RuleFailedException>(vacuous.Assert);
        Assert.Equal(
            "rule `nothing-matches` is vacuous: its from side matched nothing, so it checks nothing (ADR-0007)",
            failure.Message);
    }

    private static string FindAbove(string relative)
    {
        for (DirectoryInfo? directory = new(AppContext.BaseDirectory); directory is not null; directory = directory.Parent)
        {
            string candidate = Path.Combine(directory.FullName, relative);
            if (Directory.Exists(candidate))
            {
                return candidate;
            }
        }

        throw new DirectoryNotFoundException(relative);
    }

    private static string RunJunit(string directory, string[] extra)
    {
        ProcessStartInfo start = new(TestBinary.Path)
        {
            WorkingDirectory = directory,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
        };
        foreach (string argument in JunitArguments.Concat(extra))
        {
            start.ArgumentList.Add(argument);
        }

        using Process process = Process.Start(start)!;
        Task<string> stderr = process.StandardError.ReadToEndAsync();
        string stdout = process.StandardOutput.ReadToEnd();
        process.WaitForExit();
        Assert.True(stdout.Length > 0, stderr.GetAwaiter().GetResult());
        return stdout;
    }
}
