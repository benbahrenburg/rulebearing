// The fixture repository as an xUnit v2 user writes it: one test per rule. The failing rule is
// expected to fail, so the test asserts its message instead of letting it fail the suite
// (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14).

using System.Reflection;
using Rulebearing.TestAdapter.Fixture;
using Xunit;
using Xunit.Sdk;

namespace Rulebearing.TestAdapter.XUnit.Tests;

/// <summary>The fixture's rules through <see cref="RulebearingRulesAttribute"/>.</summary>
public sealed class ArchitectureRules
{
    /// <summary>One test per rule: the passing rule asserts, the failing one fails with the junit text.</summary>
    /// <param name="rule">The rule.</param>
    [Theory]
    [RulebearingRules(FixtureRules.Config)]
    public void Holds(RuleResult rule)
    {
        ArgumentNullException.ThrowIfNull(rule);
        if (rule.Name == FixtureRules.Failing)
        {
            FailException failure = Assert.Throws<FailException>(rule.Assert);
            Assert.Equal(FixtureRules.FailingMessage, failure.Message);
        }
        else
        {
            rule.Assert();
        }
    }

    /// <summary>The attribute yields one serializable row per rule, in the junit order.</summary>
    [Fact]
    public void OneRowPerRule()
    {
        RulebearingRulesAttribute attribute = Attribute();
        List<object[]> rows = [.. attribute.GetData(typeof(ArchitectureRules).GetMethod(nameof(Holds))!)];
        Assert.All(rows, static row => Assert.IsType<XunitRuleResult>(Assert.Single(row)));
        Assert.Equal(FixtureRules.Names, rows.Select(static row => ((RuleResult)row[0]).Name));
    }

    /// <summary>The attribute's properties become the run's options.</summary>
    [Fact]
    public void PropertiesBecomeOptions()
    {
        RulebearingRulesAttribute attribute = new()
        {
            Graph = "g.json",
            Result = "r.json",
            Binary = "b",
            WorkingDirectory = "w",
            Arguments = ["src"],
        };
        RulebearingOptions options = attribute.Options;
        Assert.Equal(("rulebearing.yaml", "g.json", "r.json", "b", "w"), (options.Config, options.Graph, options.Result, options.Binary, options.WorkingDirectory));
        Assert.Equal(["src"], options.Arguments);
    }

    /// <summary>A row survives xUnit's serialization and still fails with xUnit's exception.</summary>
    [Fact]
    public void ARowRoundTripsThroughXunitSerialization()
    {
        XunitRuleResult row = (XunitRuleResult)Attribute().GetData(typeof(ArchitectureRules).GetMethod(nameof(Holds))!).First()[0];
        string serialized = SerializationHelper.Serialize(row);
        XunitRuleResult back = SerializationHelper.Deserialize<XunitRuleResult>(serialized);
        Assert.Equal(FixtureRules.Failing, back.Name);
        Assert.Equal(FixtureRules.FailingMessage, Assert.Throws<FailException>(back.Assert).Message);
        Assert.Throws<ArgumentNullException>(() => new XunitRuleResult(null!));
        Assert.Throws<ArgumentNullException>(() => row.Serialize(null!));
        Assert.Throws<ArgumentNullException>(() => row.Deserialize(null!));
    }

    private static RulebearingRulesAttribute Attribute() =>
        typeof(ArchitectureRules).GetMethod(nameof(Holds))!.GetCustomAttribute<RulebearingRulesAttribute>()!;
}
