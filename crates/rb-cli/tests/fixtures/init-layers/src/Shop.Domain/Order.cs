namespace Shop.Domain;

public sealed class Order
{
    public Order(int id) => Id = id;

    public int Id { get; }
}
