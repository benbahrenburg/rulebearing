// The exception RuleResult.Assert throws when no framework package supplied its own
// (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 14).

namespace Rulebearing.TestAdapter;

/// <summary>A rule failed or could not be checked, outside any framework package.</summary>
public sealed class RuleFailedException : Exception
{
    /// <summary>Creates the exception with no message.</summary>
    public RuleFailedException()
    {
    }

    /// <summary>Creates the exception with the rule's failure message.</summary>
    /// <param name="message">The <c>junit</c> failure message.</param>
    public RuleFailedException(string message)
        : base(message)
    {
    }

    /// <summary>Creates the exception with a message and its cause.</summary>
    /// <param name="message">The failure message.</param>
    /// <param name="innerException">The cause.</param>
    public RuleFailedException(string message, Exception innerException)
        : base(message, innerException)
    {
    }
}
