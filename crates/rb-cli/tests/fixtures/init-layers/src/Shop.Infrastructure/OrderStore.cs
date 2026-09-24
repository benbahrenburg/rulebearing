using Shop.Domain;

namespace Shop.Infrastructure;

public sealed class OrderStore
{
    private readonly List<Order> orders = [];

    public void Add(Order order) => orders.Add(order);

    public int Count => orders.Count;
}
