namespace Shop.ArchitectureTests;

using Common;

public class ModuleTests
{
    [Theory]
    [InlineData(Modules.Domain, "Shop.Infrastructure")]
    [InlineData(Modules.Billing, "Shop.Web")]
    public void Module_Does_Not_Depend_On(string module, string forbidden)
    {
        Solution.Types
            .That().ResideInNamespace(module)
            .Should().NotHaveDependencyOn(forbidden)
            .ShouldSucceed();
    }

    [Theory]
    [MemberData(nameof(Pairs))]
    public void Computed_Pairs(string module, string forbidden)
    {
        Solution.Types.That().ResideInNamespace(module).Should().NotHaveDependencyOn(forbidden).ShouldSucceed();
    }

    [Fact]
    public void Helper_Results_Are_Not_Read()
    {
        var names = Solution.Types.That().ResideInNamespace(Modules.Billing).Names();
        Solution.Types.That().ResideInNamespace(Modules.Domain).Should().NotHaveDependencyOnAny(names).ShouldSucceed();
    }
}
