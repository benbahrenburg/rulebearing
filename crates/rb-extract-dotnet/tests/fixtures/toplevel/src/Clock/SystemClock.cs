namespace TopLevel.Clock;

/// <summary>The time, fixed for the fixture.</summary>
public sealed class SystemClock
{
    private readonly long _ticks = 42;

    /// <summary>The current ticks.</summary>
    public long Now() => _ticks;
}
