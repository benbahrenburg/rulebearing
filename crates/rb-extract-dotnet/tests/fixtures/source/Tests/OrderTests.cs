using Shop.Domain.Orders;
using Xunit;

namespace Shop.Tests;

public class OrderTests
{
    [Fact]
    public void Makes() => Assert.NotNull(new Order(null!));
}
