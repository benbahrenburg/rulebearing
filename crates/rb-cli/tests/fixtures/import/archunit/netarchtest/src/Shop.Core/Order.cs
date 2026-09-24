namespace Shop.Core.Domain
{
    public abstract class AggregateRoot { }

    public class Order : AggregateRoot { }

    public interface IRepository { }
}
