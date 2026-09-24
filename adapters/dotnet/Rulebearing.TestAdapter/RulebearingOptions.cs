// What to run: the configuration, a graph to evaluate instead of extracting, or a saved result
// (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14: "runs `rulebearing
// cruise --output-type json` (or reads a `--graph` path)").

namespace Rulebearing.TestAdapter;

/// <summary>How to obtain the rule results: which configuration, graph or saved result, and which binary.</summary>
/// <remarks>
/// A relative path is looked up from the test assembly's directory upwards, then from the current
/// directory upwards; the first directory that holds it wins. That finds a
/// <c>rulebearing.yaml</c> at the repository root from any test project below it.
/// </remarks>
public sealed record RulebearingOptions
{
    /// <summary>The environment variable naming the binary, checked before <c>PATH</c>.</summary>
    public const string BinaryVariable = "RULEBEARING_BINARY";

    /// <summary>
    /// The configuration file (<c>--config</c>). Its directory is the working directory of the run
    /// unless <see cref="WorkingDirectory"/> says otherwise. <see langword="null"/> lets the binary
    /// find one in the working directory.
    /// </summary>
    public string? Config { get; init; } = "rulebearing.yaml";

    /// <summary>A graph document to evaluate the rules over instead of extracting (<c>--graph</c>).</summary>
    public string? Graph { get; init; }

    /// <summary>
    /// A result saved from <c>rulebearing cruise -T json</c>. When set, the binary is not run and
    /// <see cref="Config"/>, <see cref="Graph"/> and <see cref="Arguments"/> are ignored.
    /// </summary>
    public string? Result { get; init; }

    /// <summary>
    /// The <c>rulebearing</c> binary. When unset, <see cref="BinaryVariable"/> names it, else the
    /// first <c>rulebearing</c> on <c>PATH</c> (the <c>dotnet tool install -g Rulebearing</c> shim).
    /// </summary>
    public string? Binary { get; init; }

    /// <summary>The directory to run in, when it is not the configuration's.</summary>
    public string? WorkingDirectory { get; init; }

    /// <summary>More arguments for <c>cruise</c>: the files and directories to cruise, or options such as <c>--liveness warn</c>.</summary>
    public IReadOnlyList<string> Arguments { get; init; } = [];

    /// <summary>How long the run may take before it is stopped and reported as an error.</summary>
    public TimeSpan Timeout { get; init; } = TimeSpan.FromMinutes(10);
}
