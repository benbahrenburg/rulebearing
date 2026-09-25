using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using Sample.Customers;

namespace Sample.Orders;

public partial class Order
{
    private readonly List<Line> _lines = new List<Line>();

    public void Add(Line line)
    {
        _lines.Add(line);
    }

    public decimal Total()
    {
        return _lines.Where(l => l.Quantity > 0).Sum(l => l.Price * l.Quantity);
    }

    public async Task<Customer> LoadOwnerAsync()
    {
        await Task.Yield();
        return new Customer("async");
    }

    public IEnumerable<Line> Positive()
    {
        foreach (var line in _lines)
        {
            if (line.Quantity > 0)
            {
                yield return line;
            }
        }
    }

    public static object Dynamic()
    {
        return Type.GetType("Sample.Customers.Customer");
    }

    public sealed class Line
    {
        public decimal Price { get; set; }

        public int Quantity { get; private set; }
    }
}
