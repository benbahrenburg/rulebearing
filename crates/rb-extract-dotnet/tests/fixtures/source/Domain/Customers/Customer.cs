namespace Shop.Domain
{
    namespace Customers
    {
        public record Customer(string Name) : IParty;

        public interface IParty
        {
        }
    }
}
