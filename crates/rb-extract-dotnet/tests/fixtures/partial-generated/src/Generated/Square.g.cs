// What a singleton source generator writes under obj/: a private constructor and the instance,
// and a type of its own that no written file declares.
namespace PartialGenerated.Shapes;

internal sealed partial class Square
{
    private Square()
    {
    }

    public static readonly Square Instance = new Square();
}

internal sealed class SquareFactory
{
    public SquareFactory()
    {
    }

    public Shape Create() => Square.Instance;
}
