using System.Reflection;
using NetArchTest.Rules;
using Shop.Core.Domain;

namespace Shop.ArchitectureTests.Common;

internal static class Solution
{
    private static readonly Assembly Core = typeof(Order).Assembly;

    // A property named after its type: `Types.InAssembly` below is still NetArchTest's.
    internal static Types Types => Types.InAssembly(Core);
}

internal static class Modules
{
    internal const string Domain = "Shop.Core.Domain";
    internal const string Billing = "Shop.Core.Billing";
}

internal static class RuleExtensions
{
    // A project's own assertion over GetResult(): a call of it runs the rule.
    internal static void ShouldSucceed(this ConditionList conditions)
    {
        var result = conditions.GetResult();
        Assert.True(result.IsSuccessful);
    }

    // A project's own helper whose result the importer does not evaluate.
    internal static string[] Names(this PredicateList predicates) =>
        predicates.GetTypes().Select(t => t.FullName).ToArray();
}
