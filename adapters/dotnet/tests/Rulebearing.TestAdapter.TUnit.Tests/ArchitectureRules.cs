// The fixture repository as a TUnit user writes it: one test per rule. The failing rule is
// expected to fail, so the test asserts its message instead of letting it fail the suite
// (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14).

using Rulebearing.TestAdapter.Fixture;
using TUnit.Assertions.Exceptions;

namespace Rulebearing.TestAdapter.TUnit.Tests;

/// <summary>The fixture's rules through <see cref="RulebearingDataSourceAttribute"/>.</summary>
public sealed class ArchitectureRules
{
    /// <summary>One test per rule: the passing rule asserts, the failing one fails with the junit text.</summary>
    /// <param name="rule">The rule.</param>
    /// <returns>The assertion.</returns>
    [Test]
    [RulebearingDataSource(FixtureRules.Config)]
    public async Task Holds(RuleResult rule)
    {
        ArgumentNullException.ThrowIfNull(rule);
        if (rule.Name == FixtureRules.Failing)
        {
            AssertionException? failure = null;
            try
            {
                rule.Assert();
            }
            catch (AssertionException e)
            {
                failure = e;
            }

            await Assert.That(failure?.Message).IsEqualTo(FixtureRules.FailingMessage);
        }
        else
        {
            rule.Assert();
        }
    }

    /// <summary>The attribute yields one result per rule, in the junit order.</summary>
    /// <returns>The assertion.</returns>
    [Test]
    public async Task OneResultPerRule()
    {
        RulebearingDataSourceAttribute attribute = new(FixtureRules.Config);
        await Assert.That(string.Join(",", attribute.Rules().Select(static r => r.Name))).IsEqualTo(string.Join(",", FixtureRules.Names));
    }

    /// <summary>The attribute's properties become the run's options.</summary>
    /// <returns>The assertion.</returns>
    [Test]
    public async Task PropertiesBecomeOptions()
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
        await Assert.That($"{options.Config}|{options.Graph}|{options.Result}|{options.Binary}|{options.WorkingDirectory}|{options.Arguments.Single()}")
            .IsEqualTo("rulebearing.yaml|g.json|r.json|b|w|src");
    }
}
