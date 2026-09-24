using Sample.Core;
using Sample.Customers;

namespace Sample.Orders;

[Audit("orders", Level = 2)]
public partial class Order : IEntity
{
    private readonly Customer _customer;

    public Order(Customer customer)
    {
        _customer = customer;
    }

    public int Id { get; init; }

    public Customer Owner => _customer;

    public Clock Clock { get; } = new Clock();
}
