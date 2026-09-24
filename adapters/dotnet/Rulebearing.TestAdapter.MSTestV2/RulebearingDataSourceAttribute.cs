// MSTest v2 data source: one data row per rule of a Rulebearing run
// (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14;
// docs/artifacts/archunitnet-0.13.4-coverage.md#test-framework-adapters, ArchUnitNET.MSTestV2).

using System.Reflection;
using Microsoft.VisualStudio.TestTools.UnitTesting;

namespace Rulebearing.TestAdapter;

/// <summary>
/// Supplies one <see cref="RuleResult"/> per rule to an MSTest test method, one data row each,
/// named after the rule, so <c>rule.Assert()</c> fails the row with the rule's <c>fix</c> and its
/// first violations as an MSTest <see cref="AssertFailedException"/>.
/// </summary>
/// <example>
/// <code>
/// [TestMethod]
/// [RulebearingDataSource("rulebearing.yaml")]
/// public void Holds(RuleResult rule) => rule.Assert();
/// </code>
/// </example>
[AttributeUsage(AttributeTargets.Method, AllowMultiple = true, Inherited = false)]
public sealed class RulebearingDataSourceAttribute : Attribute, ITestDataSource
{
    /// <summary>Runs Rulebearing with the <c>rulebearing.yaml</c> found above the test assembly.</summary>
    public RulebearingDataSourceAttribute()
        : this("rulebearing.yaml")
    {
    }

    /// <summary>Runs Rulebearing with <paramref name="config"/>.</summary>
    /// <param name="config">The configuration file, relative to a directory above the test assembly or absolute.</param>
    public RulebearingDataSourceAttribute(string config)
    {
        Config = config;
    }

    /// <summary>The configuration file.</summary>
    public string Config { get; }

    /// <summary>A graph document to evaluate instead of extracting (<c>--graph</c>).</summary>
    public string? Graph { get; set; }

    /// <summary>A result saved from <c>rulebearing cruise -T json</c>, read instead of running the binary.</summary>
    public string? Result { get; set; }

    /// <summary>The <c>rulebearing</c> binary; by default <c>RULEBEARING_BINARY</c>, then <c>PATH</c>.</summary>
    public string? Binary { get; set; }

    /// <summary>The directory to run in, when it is not the configuration's.</summary>
    public string? WorkingDirectory { get; set; }

    /// <summary>More arguments for <c>cruise</c>, such as the directories to cruise.</summary>
    public string[] Arguments { get; set; } = [];

    /// <summary>The options this attribute describes.</summary>
    public RulebearingOptions Options => new()
    {
        Config = Config,
        Graph = Graph,
        Result = Result,
        Binary = Binary,
        WorkingDirectory = WorkingDirectory,
        Arguments = Arguments,
    };

    /// <summary>MSTest's assertion exception for a failure message.</summary>
    /// <param name="message">The failure message.</param>
    /// <returns>The exception <see cref="RuleResult.Assert"/> throws.</returns>
    public static Exception Failure(string message) => new AssertFailedException(message);

    /// <inheritdoc />
    public IEnumerable<object[]> GetData(MethodInfo methodInfo) =>
        RulebearingRun.Load(Options).Select(static rule => new object[] { rule.WithAssertion(Failure) });

    /// <inheritdoc />
    public string GetDisplayName(MethodInfo methodInfo, object[] data) =>
        data is [RuleResult rule] ? rule.Name : string.Join(", ", data ?? []);
}
