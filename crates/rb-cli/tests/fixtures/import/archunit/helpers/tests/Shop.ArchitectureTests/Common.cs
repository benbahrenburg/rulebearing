using System.Linq;
using NetArchTest.Rules;
using Shop.Core.Domain;

namespace Shop.ArchitectureTests;

// An xUnit class fixture: a test class that implements IClassFixture<CoreFixture> is given one.
public class CoreFixture
{
    public Types Types { get; } = Types.InAssembly(typeof(Order).Assembly);
}

internal static class Solution
{
    internal static Types Types => Types.InAssembly(typeof(Order).Assembly);
}

public static class Utils
{
    // A generic helper, called through `using static` with its type argument.
    public static string namespaceof<T>() => typeof(T).Namespace!;
}

internal static class PredicatesExtensions
{
    // The full names of the types a selection returns, which a dependency search is then given.
    internal static string[] GetModuleTypes(this PredicateList predicates) =>
        predicates
            .GetTypes()
            .Select(type => type.FullName)
            .Distinct()
            .ToArray()!;
}
