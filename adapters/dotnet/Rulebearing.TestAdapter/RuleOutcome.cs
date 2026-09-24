// The three results a junit test case can have (crates/rb-report/src/junit.rs).

namespace Rulebearing.TestAdapter;

/// <summary>Whether a rule held, failed, or could not be checked.</summary>
public enum RuleOutcome
{
    /// <summary>No error-severity violation and nothing that stopped the rule being checked.</summary>
    Passed,

    /// <summary>At least one error-severity violation: a <c>junit</c> failure.</summary>
    Failed,

    /// <summary>The rule could not be checked (vacuous, expired, a ratchet without a budget): a <c>junit</c> error.</summary>
    Error,
}
