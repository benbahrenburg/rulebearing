using System.Linq;
using ArchUnitNET.Domain;
using ArchUnitNET.Fluent.Syntax.Elements.Types.Classes;
using ArchUnitNET.Loader;
using Shop.Core.Domain;
using Xunit;
using static ArchUnitNET.Fluent.ArchRuleDefinition;

namespace Shop.ArchitectureTests;

public class ArchTests
{
    private static readonly Architecture Architecture = new ArchLoader().LoadAssemblies(typeof(Order).Assembly).Build();

    // An extension method whose body continues the chain over its `this` parameter.
    [Fact]
    public void Services_Are_Sealed_And_Public() =>
        Classes().That().HaveNameEndingWith("Service").Should().BeSealedAndPublic().Check(Architecture);

    // A C# 14 extension block wrapping a custom condition: it stays in ArchUnitNET.
    [Fact]
    public void Domain_Has_No_Public_Setters() =>
        Classes().That().ResideInNamespace("Shop.Core.Domain").Should().NotHavePublicSetters().Check(Architecture);

    // Runs no rule: LINQ over the loaded classes, so the test stays with ArchUnitNET.
    [Fact]
    public void Services_Are_Counted()
    {
        var services = Architecture.Classes.Where(c => c.NameEndsWith("Service")).ToList();
        Assert.Single(services);
    }
}

public static class ArchExtensions
{
    public static ClassesShouldConjunction BeSealedAndPublic(this ClassesShould should) =>
        should.BeSealed().AndShould().BePublic();

    extension(ClassesShould should)
    {
        public ClassesShouldConjunction NotHavePublicSetters() =>
            should.FollowCustomCondition(c => c.GetPropertyMembers().All(p => p.SetterVisibility != Visibility.Public), "have no public setters", "has a public setter");
    }
}
