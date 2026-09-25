// One rule of a Rulebearing run, as the host test framework sees it.
//
// Contract: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, section 1.5 (one test
// case per rule; the adapters' failure message is the junit reporter's text) and Step 14 (2H).
// Decisions: docs/adr/0007-vacuous-rules-fail-by-default.md (a vacuous rule fails with the
// liveness reason), docs/adr/0018-test-coverage-threshold.md.
// Source of the message text: crates/rb-report/src/junit.rs and crates/rb-report/src/catalog.rs.

using System.Text.Json;
using System.Text.Json.Serialization;

namespace Rulebearing.TestAdapter;

/// <summary>
/// The result of one rule of a Rulebearing run: one test case in the host test framework.
/// </summary>
/// <remarks>
/// The adapter never evaluates a rule. Every field comes from the binary's JSON result, and
/// <see cref="Message"/> is the text the <c>junit</c> reporter writes for the same rule: the
/// failure message (the rule's <c>fix</c>, then the first five error violations) and, for a rule
/// that could not be checked (vacuous, expired, a ratchet without a budget), the error message.
/// </remarks>
public class RuleResult
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    private string name = string.Empty;
    private string family = string.Empty;
    private string severity = string.Empty;
    private string? comment;
    private string? fix;
    private string? failureMessage;
    private string? failureDetail;
    private IReadOnlyList<RuleError> errors = [];
    private IReadOnlyList<string> output = [];
    private Func<string, Exception> assertion = static message => new RuleFailedException(message);

    /// <summary>Creates a rule result from the fields the <c>junit</c> reporter derives.</summary>
    /// <param name="name">
    /// The rule's identity, the test case's name: its name as its violations carry it, with
    /// <c>#n</c> for the n-th rule of a name already taken (the <c>junit</c> test case name).
    /// </param>
    /// <param name="family">
    /// <c>forbidden</c>, <c>allowed</c>, <c>required</c>, <c>elements</c>, <c>slices</c>,
    /// <c>diagrams</c>, <c>ratchets</c>, <c>knownViolations</c>, or <c>rules</c> for a rule known
    /// only from its violations.
    /// </param>
    /// <param name="severity">The configured severity.</param>
    /// <param name="comment">The rule's comment, when it has one.</param>
    /// <param name="fix">The rule's <c>fix</c>, when it has one.</param>
    /// <param name="failureMessage">The failure message, when an error violation fails the rule.</param>
    /// <param name="failureDetail">Every error violation, one per line, when the rule failed.</param>
    /// <param name="errors">Why the rule could not be checked, when it could not.</param>
    /// <param name="output">What the rule reports without failing: warn, info and known findings.</param>
    public RuleResult(
        string name,
        string family,
        string severity,
        string? comment,
        string? fix,
        string? failureMessage,
        string? failureDetail,
        IReadOnlyList<RuleError> errors,
        IReadOnlyList<string> output)
    {
        ArgumentNullException.ThrowIfNull(name);
        ArgumentNullException.ThrowIfNull(family);
        ArgumentNullException.ThrowIfNull(severity);
        ArgumentNullException.ThrowIfNull(errors);
        ArgumentNullException.ThrowIfNull(output);
        this.name = name;
        this.family = family;
        this.severity = severity;
        this.comment = comment;
        this.fix = fix;
        this.failureMessage = failureMessage;
        this.failureDetail = failureDetail;
        this.errors = [.. errors];
        this.output = [.. output];
    }

    /// <summary>
    /// For a framework package whose test framework builds data rows from a parameterless
    /// constructor and restores them with <see cref="Restore"/> (xUnit's serialization).
    /// </summary>
    protected RuleResult()
    {
    }

    /// <summary>The rule's identity: its name, or <c>name#2</c> and so on for a later rule of the same name, as <c>junit</c> names the test case.</summary>
    public string Name => name;

    /// <summary>The configuration family the rule belongs to.</summary>
    public string Family => family;

    /// <summary>The configured severity.</summary>
    public string Severity => severity;

    /// <summary>The rule's comment, when it has one.</summary>
    public string? Comment => comment;

    /// <summary>The rule's <c>fix</c>, when it has one.</summary>
    public string? Fix => fix;

    /// <summary>The <c>junit</c> failure message, when an error violation fails the rule.</summary>
    public string? FailureMessage => failureMessage;

    /// <summary>Every error violation, one per line, when the rule failed.</summary>
    public string? FailureDetail => failureDetail;

    /// <summary>Why the rule could not be checked: vacuous, expired, or a ratchet without a budget.</summary>
    public IReadOnlyList<RuleError> Errors => errors;

    /// <summary>Findings reported without failing: warn, info and known violations, a warned vacuous rule.</summary>
    public IReadOnlyList<string> Output => output;

    /// <summary>Whether the rule held, failed, or could not be checked.</summary>
    public RuleOutcome Outcome => errors.Count > 0
        ? RuleOutcome.Error
        : failureMessage is null ? RuleOutcome.Passed : RuleOutcome.Failed;

    /// <summary>
    /// The message <see cref="Assert"/> fails with: the <c>junit</c> failure message, then each
    /// error message on its own line; <see langword="null"/> when the rule held.
    /// </summary>
    public string? Message
    {
        get
        {
            List<string> lines = [];
            if (failureMessage is not null)
            {
                lines.Add(failureMessage);
            }

            lines.AddRange(errors.Select(static e => e.Message));
            return lines.Count == 0 ? null : string.Join('\n', lines);
        }
    }

    /// <summary>
    /// Passes when the rule held, and otherwise throws the host framework's assertion exception
    /// with <see cref="Message"/>. Outside a framework package the exception is a
    /// <see cref="RuleFailedException"/>.
    /// </summary>
    public void Assert()
    {
        if (Message is { } message)
        {
            throw assertion(message);
        }
    }

    /// <summary>
    /// This result, failing through <paramref name="failure"/>: how a framework package makes
    /// <see cref="Assert"/> throw its framework's assertion exception.
    /// </summary>
    /// <param name="failure">Builds the exception from the failure message.</param>
    /// <returns>A copy of this result that fails through <paramref name="failure"/>.</returns>
    public RuleResult WithAssertion(Func<string, Exception> failure)
    {
        ArgumentNullException.ThrowIfNull(failure);
        RuleResult copy = new(name, family, severity, comment, fix, failureMessage, failureDetail, errors, output)
        {
            assertion = failure,
        };
        return copy;
    }

    /// <summary>The fields of this result as JSON, for a framework that serializes test data.</summary>
    /// <returns>A JSON object that <see cref="FromJson"/> reads back.</returns>
    public string ToJson() => JsonSerializer.Serialize(
        new Snapshot(name, family, severity, comment, fix, failureMessage, failureDetail, [.. errors], [.. output]),
        JsonOptions);

    /// <summary>Reads a result written by <see cref="ToJson"/>.</summary>
    /// <param name="json">The JSON <see cref="ToJson"/> wrote.</param>
    /// <returns>The result, failing with a <see cref="RuleFailedException"/>.</returns>
    /// <exception cref="RulebearingException">The text is not a serialized result.</exception>
    public static RuleResult FromJson(string json)
    {
        ArgumentNullException.ThrowIfNull(json);
        Snapshot? snapshot;
        try
        {
            snapshot = JsonSerializer.Deserialize<Snapshot>(json, JsonOptions);
        }
        catch (JsonException e)
        {
            throw new RulebearingException($"A serialized rule result is not valid JSON: {e.Message}", e);
        }

        if (snapshot is null || snapshot.Name is null || snapshot.Family is null || snapshot.Severity is null)
        {
            throw new RulebearingException("A serialized rule result has no name, family or severity.");
        }

        return new RuleResult(
            snapshot.Name,
            snapshot.Family,
            snapshot.Severity,
            snapshot.Comment,
            snapshot.Fix,
            snapshot.FailureMessage,
            snapshot.FailureDetail,
            snapshot.Errors ?? [],
            snapshot.Output ?? []);
    }

    /// <summary>The rule's name, which test frameworks show as the test case's argument.</summary>
    /// <returns>The rule's name.</returns>
    public override string ToString() => name;

    /// <summary>
    /// Replaces every field with <paramref name="other"/>'s and fails through
    /// <paramref name="failure"/>: how a framework package restores a deserialized data row.
    /// </summary>
    /// <param name="other">The result whose fields to take.</param>
    /// <param name="failure">Builds the framework's assertion exception from the failure message.</param>
    protected void Restore(RuleResult other, Func<string, Exception> failure)
    {
        ArgumentNullException.ThrowIfNull(other);
        ArgumentNullException.ThrowIfNull(failure);
        name = other.name;
        family = other.family;
        severity = other.severity;
        comment = other.comment;
        fix = other.fix;
        failureMessage = other.failureMessage;
        failureDetail = other.failureDetail;
        errors = other.errors;
        output = other.output;
        assertion = failure;
    }

    private sealed record Snapshot(
        string? Name,
        string? Family,
        string? Severity,
        string? Comment,
        string? Fix,
        string? FailureMessage,
        string? FailureDetail,
        List<RuleError>? Errors,
        List<string>? Output);
}
