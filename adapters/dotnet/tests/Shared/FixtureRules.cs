// The fixture repository (tests/fixture) and what every framework's fixture project expects of it:
// one rule that holds, one that fails with the junit reporter's message
// (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14). Compiled into every
// test project under adapters/dotnet/tests through Directory.Build.props.

namespace Rulebearing.TestAdapter.Fixture;

/// <summary>The fixture repository's configuration and the results it must produce.</summary>
internal static class FixtureRules
{
    /// <summary>The configuration, found upwards from any test assembly under adapters/dotnet/tests.</summary>
    public const string Config = "fixture/rulebearing.yaml";

    /// <summary>The rule the fixture code keeps.</summary>
    public const string Passing = "db-not-to-ui";

    /// <summary>The rule the fixture code breaks: src/ui/view.ts imports src/db/store.ts.</summary>
    public const string Failing = "ui-not-to-db";

    /// <summary>
    /// The failing rule's message: its <c>fix</c>, then the violation with its stable id (ADR-0015)
    /// and position, exactly as <c>rulebearing cruise -T junit</c> writes it in
    /// <c>&lt;failure message&gt;</c>.
    /// </summary>
    public const string FailingMessage =
        "Call the service in src/service instead of importing src/db.\n"
        + "RB-638a6bf7 src/ui/view.ts -> src/db/store.ts (line 1, column 1)";

    /// <summary>The rule names, in the order the junit reporter lists them.</summary>
    public static readonly string[] Names = [Failing, Passing];
}
