using System.Linq;
using NetArchTest.Rules;
using Shop.Core.Domain;
using Xunit;
using Xunit.Abstractions;
using static Shop.ArchitectureTests.Utils;

namespace Shop.ArchitectureTests;

public class FixtureTests(CoreFixture fixture) : IClassFixture<CoreFixture>
{
    // A helper whose one `return` is a fluent chain, over the class fixture.
    private PredicateList GetTypesThat()
    {
        return fixture.Types.That().ResideInNamespace(namespaceof<Order>()).And();
    }

    [Fact]
    public void Services_Are_Sealed()
    {
        var result = GetTypesThat().HaveNameEndingWith("Service").Should().BeSealed().GetResult();
        Assert.True(result.IsSuccessful);
    }

    // Runs no rule: the selection is counted in C#, so the test stays with NetArchTest.
    [Fact]
    public void Sealed_Types_Are_Counted()
    {
        var types = GetTypesThat().AreSealed().GetTypes();
        Assert.Equal(1, types.Count());
    }

    // An extension method returning the names of the types a selection holds.
    [Theory]
    [InlineData("Shop.Core.Billing")]
    public void Domain_Does_Not_Use_Module(string module)
    {
        var forbidden = Solution.Types.That().ResideInNamespace(module).GetModuleTypes();
        var result = Solution.Types.That().ResideInNamespace("Shop.Core.Domain").ShouldNot().HaveDependencyOnAny(forbidden).GetResult();
        Assert.True(result.IsSuccessful);
    }

    // An expression-bodied helper, its argument bound to its parameter and its default read.
    [Fact]
    public void Domain_Does_Not_Use_Web() => Assert.True(DomainMustNotUse("Shop.Web").GetResult().IsSuccessful);

    private static ConditionList DomainMustNotUse(string forbidden, string within = "Shop.Core.Domain") =>
        Solution.Types.That().ResideInNamespace(within).ShouldNot().HaveDependencyOn(forbidden);

    // Not imported: a helper with more than one statement, and an overload chosen by type.
    [Fact]
    public void Helper_With_Statements()
    {
        Assert.True(Built().GetResult().IsSuccessful);
    }

    private static ConditionList Built()
    {
        var types = Solution.Types.That();
        return types.Should().BeSealed();
    }

    [Fact]
    public void Overloaded_Helper() => Assert.True(Pick("a").GetResult().IsSuccessful);

    private static ConditionList Pick(string a) => Solution.Types.Should().HaveName(a);

    private static ConditionList Pick(string a, string b = "x") => Solution.Types.Should().HaveName(b);
}

// Not a class fixture: xUnit fills this constructor parameter with something the sources do not build.
public class OutputTests(ITestOutputHelper output)
{
    [Fact]
    public void Constructor_Parameter()
    {
        Assert.True(output.Types.That().AreSealed().Should().BePublic().GetResult().IsSuccessful);
    }
}
