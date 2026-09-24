using Shop.Domain;
using Shop.Infrastructure;

namespace Shop.Application;

// The one finding init baselines: the application layer uses the infrastructure's store directly
// instead of an abstraction it declares.
public sealed class PlaceOrder
{
    private readonly OrderStore store = new();

    public int Run(int id)
    {
        store.Add(new Order(id));
        return store.Count;
    }
}
