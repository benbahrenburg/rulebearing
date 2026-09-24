namespace Sample.Customers;

public record Customer(string Name);

public struct Money
{
    public decimal Amount;
}

public enum Tier
{
    Basic,
    Gold,
}

public abstract class Repository<T>
    where T : class
{
    protected abstract T Find(int id);
}

public static class Constants
{
    public const string Name = "sample";
}
