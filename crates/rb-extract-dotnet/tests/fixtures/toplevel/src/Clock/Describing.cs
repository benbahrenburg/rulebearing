namespace TopLevel.Clock;

/// <summary>Extension methods over ticks.</summary>
public static class Describing
{
    /// <summary>The ticks as text.</summary>
    public static string Describe(this long ticks) => ticks.ToString(System.Globalization.CultureInfo.InvariantCulture);
}
