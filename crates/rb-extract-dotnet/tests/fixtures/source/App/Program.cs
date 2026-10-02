using Serilog;
using Shop.Domain.Orders;

var order = new Order(new Shop.Domain.Customers.Customer("app"));
Log.Information("created {Order}", order);
