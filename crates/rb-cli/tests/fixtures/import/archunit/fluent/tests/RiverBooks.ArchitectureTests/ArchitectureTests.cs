using ArchUnitNET.Fluent;
using ArchUnitNET.Fluent.Slices;
using RiverBooks.Books;
using Users = RiverBooks.Users;

namespace RiverBooks.ArchitectureTests;

public class ArchitectureTests
{
    private static readonly Architecture Architecture = new ArchLoader()
        .LoadAssemblies(typeof(Book).Assembly, typeof(Users.User).Assembly)
        .Build();

    private const string BooksNamespace = "RiverBooks.Books";

    private static readonly IObjectProvider<IType> BooksLayer = Types()
        .That()
        .ResideInNamespace(BooksNamespace, true)
        .As("Books");

    private static readonly Class EntityClass = Architecture.GetClassOfType(typeof(Entity));

    [Fact]
    public void BooksShouldNotDependOnUsers()
    {
        Classes().That().ResideInNamespace("RiverBooks.Books", true)
            .Should().NotDependOnAny(Classes().That().ResideInNamespace("RiverBooks.Users", true))
            .Check(Architecture);
    }

    [Fact]
    public void EntitiesAreSealedOrAbstract() =>
        Classes().That().AreAssignableTo(EntityClass).And().AreNot(typeof(Entity))
            .Should().BeSealed().OrShould().BeAbstract()
            .Because("an entity is either a leaf or a base")
            .WithoutRequiringPositiveResults()
            .Check(Architecture);

    [Fact]
    public void RepositoriesLiveInTheBooksLayer()
    {
        IArchRule rule = Interfaces().That().HaveNameEndingWith("Repository")
            .Should().Be(BooksLayer)
            .AndShould().BePublic();
        rule.Check(Architecture);
    }

    [Fact]
    public void NestedTypesAndAttributes()
    {
        Types().That().Are(typeof(Book.Chapter)).Or().HaveAnyAttributes(typeof(AggregateRootAttribute))
            .Should().BeAssignableToTypesThat().ResideInNamespace(nameof(RiverBooks) + ".Books")
            .Check(Architecture);
        Types(true).That().Are(typeof(Users.UserService)).Should().NotBe(typeof(Book)).Check(Architecture);
    }

    [Fact]
    public void BooksAreFreeOfCycles()
    {
        SliceRuleDefinition.Slices().Matching("RiverBooks.(*)").Should().BeFreeOfCycles().Check(Architecture);
    }

    [Fact]
    public void TheseAreNotImported()
    {
        Types().That().FollowCustomPredicate(t => t.Name.Length > 3, "long").Should().BePublic().Check(Architecture);
        Classes().That().Are(GetEntities()).Should().BeSealed().Check(Architecture);
        Classes().That().Are(typeof(Book)).Should().NotHavePropertySetterWithVisibility(Visibility.Public).Check(Architecture);
        Classes().That().Are(typeof(UnknownType)).Should().BeSealed().Check(Architecture);
        Assert.False(Classes().That().Are(typeof(Book)).Should().BeAbstract().HasNoViolations(Architecture));
    }

    private static IObjectProvider<Class> GetEntities() => Classes().That().AreAssignableTo(typeof(Entity));
}
