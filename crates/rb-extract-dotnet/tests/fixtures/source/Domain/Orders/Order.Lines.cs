using System.Collections.Generic;

namespace Shop.Domain.Orders;

public partial class Order
{
    private readonly List<Line> _lines = new();

    public IReadOnlyList<Line> Lines => _lines;

    public sealed class Line
    {
        public Values.Amount Price { get; init; }
    }
}
