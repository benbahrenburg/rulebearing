using Shop.Application;

namespace Shop.Web;

public static class OrdersEndpoint
{
    public static int Post(int id) => new PlaceOrder().Run(id);
}
