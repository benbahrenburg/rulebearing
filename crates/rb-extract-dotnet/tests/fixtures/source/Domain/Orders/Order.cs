namespace Shop.Domain.Orders;

public partial class Order
{
    private readonly Customer _customer;

    public Order(Customer customer)
    {
        _customer = customer;
    }
}
