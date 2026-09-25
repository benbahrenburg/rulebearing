// The fixture repository as an MSTest V4 user writes it: one data row per rule. The failing rule
// is expected to fail, so the test asserts its message instead of letting it fail the suite
// (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14).

using System.Reflection;
using Rulebearing.TestAdapter.Fixture;

namespace Rulebearing.TestAdapter.MSTestV4.Tests;

/// <summary>The fixture's rules through <see cref="RulebearingDataSourceAttribute"/>.</summary>
[TestClass]
public sealed class ArchitectureRules
{
    /// <summary>One data row per rule: the passing rule asserts, the failing one fails with the junit text.</summary>
    /// <param name="rule">The rule.</param>
    [TestMethod]
    [RulebearingDataSource(FixtureRules.Config)]
    public void Holds(RuleResult rule)
    {
        ArgumentNullException.ThrowIfNull(rule);
        if (rule.Name == FixtureRules.Failing)
        {
            AssertFailedException failure = Assert.ThrowsExactly<AssertFailedException>(rule.Assert);
            Assert.AreEqual(FixtureRules.FailingMessage, failure.Message);
        }
        else
        {
            rule.Assert();
        }
    }

    /// <summary>The attribute yields one row per rule, in the junit order, named after the rule.</summary>
    [TestMethod]
    public void OneRowPerRule()
    {
        RulebearingDataSourceAttribute attribute = Method.GetCustomAttribute<RulebearingDataSourceAttribute>()!;
        List<object?[]> rows = [.. attribute.GetData(Method)];
        CollectionAssert.AreEqual(FixtureRules.Names, rows.Select(row => attribute.GetDisplayName(Method, row)).ToArray());
        Assert.AreEqual("a, b", attribute.GetDisplayName(Method, ["a", "b"]));
    }

    /// <summary>The attribute's properties become the run's options.</summary>
    [TestMethod]
    public void PropertiesBecomeOptions()
    {
        RulebearingDataSourceAttribute attribute = new()
        {
            Graph = "g.json",
            Result = "r.json",
            Binary = "b",
            WorkingDirectory = "w",
            Arguments = ["src"],
        };
        RulebearingOptions options = attribute.Options;
        Assert.AreEqual(("rulebearing.yaml", "g.json", "r.json", "b", "w"), (options.Config, options.Graph, options.Result, options.Binary, options.WorkingDirectory));
        Assert.AreEqual("src", options.Arguments.Single());
    }

    private static MethodInfo Method => typeof(ArchitectureRules).GetMethod(nameof(Holds))!;
}
