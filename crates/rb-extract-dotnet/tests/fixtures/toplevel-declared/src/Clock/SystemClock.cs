namespace TopLevelDeclared.Clock;

/// <summary>The clock the program reads.</summary>
public sealed class SystemClock
{
    private readonly System.DateTime _started = System.DateTime.UtcNow;

    /// <summary>The time the clock was made, as text.</summary>
    /// <returns>The time in the round-trip format.</returns>
    public string Now() => _started.ToString("O", System.Globalization.CultureInfo.InvariantCulture);
}
