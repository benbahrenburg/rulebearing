namespace RiverBooks.Books;

public abstract class Entity { }

public sealed class Book : Entity
{
    public sealed class Chapter { }
}

public interface IBookRepository { }

[AttributeUsage(AttributeTargets.Class)]
public sealed class AggregateRootAttribute : Attribute { }
