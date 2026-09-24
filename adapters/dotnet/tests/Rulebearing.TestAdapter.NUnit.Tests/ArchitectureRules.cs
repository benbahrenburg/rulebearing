// The fixture repository as an NUnit user writes it: one test case per rule. The failing rule is
// expected to fail, so the test asserts its message instead of letting it fail the suite
// (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14).

using NUnit.Framework;
using NUnit.Framework.Interfaces;
using NUnit.Framework.Internal;
using Rulebearing.TestAdapter.Fixture;

namespace Rulebearing.TestAdapter.NUnit.Tests;

/// <summary>The fixture's rules through <see cref="RulebearingTestCaseSourceAttribute"/>.</summary>
[TestFixture]
public sealed class ArchitectureRules
{
    /// <summary>One test case per rule: the passing rule asserts, the failing one fails with the junit text.</summary>
    /// <param name="rule">The rule.</param>
    [RulebearingTestCaseSource(FixtureRules.Config)]
    public void Holds(RuleResult rule)
    {
        ArgumentNullException.ThrowIfNull(rule);
        Assert.That(TestContext.CurrentContext.Test.Name, Is.EqualTo(rule.Name));
        if (rule.Name == FixtureRules.Failing)
        {
            AssertionException? failure = Assert.Throws<AssertionException>(rule.Assert);
            Assert.That(failure?.Message, Is.EqualTo(FixtureRules.FailingMessage));
        }
        else
        {
            rule.Assert();
        }
    }

    /// <summary>The attribute builds one named test case per rule, in the junit order.</summary>
    [Test]
    public void OneCasePerRule()
    {
        List<TestMethod> cases = [.. Build(new RulebearingTestCaseSourceAttribute(FixtureRules.Config))];
        Assert.That(cases.Select(static c => c.Name), Is.EqualTo(FixtureRules.Names));
        Assert.That(cases.Select(static c => c.RunState), Is.All.EqualTo(RunState.Runnable));
    }

    /// <summary>A run that cannot be made becomes one non-runnable case carrying the reason.</summary>
    [Test]
    public void ABrokenRunIsANonRunnableCase()
    {
        TestMethod broken = Build(new RulebearingTestCaseSourceAttribute("no-such-rulebearing.yaml")).Single();
        Assert.That(broken.RunState, Is.EqualTo(RunState.NotRunnable));
        Assert.That(broken.Properties.Get(PropertyNames.SkipReason), Does.Contain("no-such-rulebearing.yaml"));
    }

    /// <summary>The attribute's properties become the run's options.</summary>
    [Test]
    public void PropertiesBecomeOptions()
    {
        RulebearingTestCaseSourceAttribute attribute = new()
        {
            Graph = "g.json",
            Result = "r.json",
            Binary = "b",
            WorkingDirectory = "w",
            Arguments = ["src"],
        };
        RulebearingOptions options = attribute.Options;
        Assert.That((options.Config, options.Graph, options.Result, options.Binary, options.WorkingDirectory), Is.EqualTo(("rulebearing.yaml", "g.json", "r.json", "b", "w")));
        Assert.That(options.Arguments, Is.EqualTo(attribute.Arguments));
        Assert.That(options.Arguments.Single(), Is.EqualTo("src"));
        Assert.Throws<ArgumentNullException>(() => attribute.BuildFrom(null!, null).ToList());
    }

    private static IEnumerable<TestMethod> Build(RulebearingTestCaseSourceAttribute attribute) =>
        attribute.BuildFrom(new MethodWrapper(typeof(ArchitectureRules), nameof(Holds)), null);
}
