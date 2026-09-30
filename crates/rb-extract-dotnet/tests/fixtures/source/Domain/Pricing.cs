using Money = Shop.Domain.Values.Amount;
using static Shop.Domain.Orders.Order;

namespace Shop.Domain;

[Pricing]
public static class Pricing
{
    public static Money For(Customer customer, Line line) => line.Price;
}

public sealed class PricingAttribute : System.Attribute
{
}
