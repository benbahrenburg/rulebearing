using System;
using System.Reflection;
using NetArchTest.Rules;
using Shop.Core.Domain;
using Xunit;

namespace Shop.ArchitectureTests
{
    public class DomainTests
    {
        private static readonly Assembly CoreAssembly = typeof(Order).Assembly;

        [Fact]
        public void Domain_Should_Not_Depend_On_Infrastructure()
        {
            var result = Types.InAssembly(CoreAssembly)
                .That().ResideInNamespace("Shop.Core.Domain")
                .ShouldNot().HaveDependencyOnAny("Shop.Infrastructure", "System.Data")
                .GetResult();

            Assert.True(result.IsSuccessful);
        }

        [Fact]
        public void Aggregates_Are_Named_And_Abstract_Or_Sealed()
        {
            var result = Types.InAssembly(Assembly.GetAssembly(typeof(AggregateRoot)))
                .That().Inherit(typeof(AggregateRoot)).Or().HaveNameEndingWith("Root", StringComparison.Ordinal)
                .Should().BeSealed().Or().BeAbstract()
                .GetResult();

            result.IsSuccessful.Should().BeTrue();
        }

        [Fact]
        public void Repositories_Are_Interfaces()
        {
            Assert.True(Types.InCurrentDomain()
                .That().HaveNameMatching("Repository$").And().DoNotResideInNamespaceContaining("Tests")
                .Should().BeInterfaces()
                .GetResult().IsSuccessful);
        }

        [Fact]
        public void Not_Imported()
        {
            var custom = Types.InNamespace("Shop.Core").Should().MeetCustomRule(new IsDocumented()).GetResult();
            Assert.True(custom.IsSuccessful);

            var immutable = Types.InNamespace("Shop.Core").Should().BeImmutable().GetResult();
            Assert.True(immutable.IsSuccessful);

            var broken = Types.InAssembly(CoreAssembly).Should().HaveNameStartingWith("I").GetResult();
            Assert.False(broken.IsSuccessful);

            var fromFile = Types.FromFile("Shop.Core.dll").Should().BePublic().GetResult();
            Assert.True(fromFile.IsSuccessful);
        }
    }
}
