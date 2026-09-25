// The adapter could not produce rule results: the binary is missing, the run cannot be trusted,
// the configuration is invalid, or the result is not JSON. Every message names the file, the
// reason and the fix where there is one (CLAUDE.md, "Errors for agents";
// docs/adr/0008-exit-code-contract.md for the exit codes it reports).

namespace Rulebearing.TestAdapter;

/// <summary>Rulebearing could not be run, or its result could not be read.</summary>
public sealed class RulebearingException : Exception
{
    /// <summary>Creates the exception with no message.</summary>
    public RulebearingException()
    {
    }

    /// <summary>Creates the exception with a message naming the file, the reason and the fix.</summary>
    /// <param name="message">What went wrong and how to fix it.</param>
    public RulebearingException(string message)
        : base(message)
    {
    }

    /// <summary>Creates the exception with a message and its cause.</summary>
    /// <param name="message">What went wrong and how to fix it.</param>
    /// <param name="innerException">The cause.</param>
    public RulebearingException(string message, Exception innerException)
        : base(message, innerException)
    {
    }
}
