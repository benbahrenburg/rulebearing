// The fixture repository as an xUnit v3 user writes it: one test per rule. The failing rule is
// expected to fail, so the test asserts its message instead of letting it fail the suite
// (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14).

using System.Reflection;
using Rulebearing.TestAdapter.Fixture;
using Xunit;
using Xunit.Sdk;

namespace Rulebearing.TestAdapter.XUnitV3.Tests;

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

    /// <summary>The attribute yields one labelled, serializable row per rule, in the junit order.</summary>
    [Fact]
    public async Task OneRowPerRule()
    {
        RulebearingRulesAttribute attribute = Attribute();
        Assert.True(attribute.SupportsDiscoveryEnumeration());
        IReadOnlyCollection<ITheoryDataRow> rows = await attribute.GetData(Method, new DisposalTracker());
        Assert.Equal(FixtureRules.Names, rows.Select(static row => row.Label));
        Assert.Equal(FixtureRules.Names, rows.Select(static row => Assert.IsType<XunitRuleResult>(Assert.Single(row.GetData())).Name));
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
    public async Task ARowRoundTripsThroughXunitSerialization()
    {
        IReadOnlyCollection<ITheoryDataRow> rows = await Attribute().GetData(Method, new DisposalTracker());
        XunitRuleResult row = (XunitRuleResult)rows.First().GetData()[0]!;
        string serialized = SerializationHelper.Instance.Serialize(row);
        XunitRuleResult back = Assert.IsType<XunitRuleResult>(SerializationHelper.Instance.Deserialize(serialized));
        Assert.Equal(FixtureRules.Failing, back.Name);
        Assert.Equal(FixtureRules.FailingMessage, Assert.Throws<FailException>(back.Assert).Message);
        Assert.Throws<ArgumentNullException>(() => new XunitRuleResult(null!));
        Assert.Throws<ArgumentNullException>(() => row.Serialize(null!));
        Assert.Throws<ArgumentNullException>(() => row.Deserialize(null!));
    }

    private static MethodInfo Method => typeof(ArchitectureRules).GetMethod(nameof(Holds))!;

    private static RulebearingRulesAttribute Attribute() => Method.GetCustomAttribute<RulebearingRulesAttribute>()!;
}
