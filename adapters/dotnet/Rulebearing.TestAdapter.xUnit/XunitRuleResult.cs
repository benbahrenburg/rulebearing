// A RuleResult xUnit v2 can serialize, so discovery lists one test per rule, and whose Assert
// throws xUnit's own assertion exception (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md,
// Step 14: "`rule.Assert()` throws the framework's assertion with the `junit` message text").

using Xunit.Abstractions;
using Xunit.Sdk;

namespace Rulebearing.TestAdapter;

/// <summary>A <see cref="RuleResult"/> that xUnit v2 serializes and that fails with <see cref="FailException"/>.</summary>
public sealed class XunitRuleResult : RuleResult, IXunitSerializable
{
    private const string Key = "rule";

    /// <summary>For xUnit's deserializer.</summary>
    [Obsolete("For xUnit's deserializer only.")]
    public XunitRuleResult()
    {
    }

    /// <summary>Wraps <paramref name="rule"/>.</summary>
    /// <param name="rule">The rule result to present to xUnit.</param>
    public XunitRuleResult(RuleResult rule)
    {
        ArgumentNullException.ThrowIfNull(rule);
        Restore(rule, Failure);
    }

    /// <summary>xUnit's assertion exception for a failure message.</summary>
    /// <param name="message">The failure message.</param>
    /// <returns>The exception <see cref="RuleResult.Assert"/> throws.</returns>
    public static Exception Failure(string message) => FailException.ForFailure(message);

    /// <inheritdoc />
    public void Deserialize(IXunitSerializationInfo info)
    {
        ArgumentNullException.ThrowIfNull(info);
        Restore(FromJson(info.GetValue<string>(Key)), Failure);
    }

    /// <inheritdoc />
    public void Serialize(IXunitSerializationInfo info)
    {
        ArgumentNullException.ThrowIfNull(info);
        info.AddValue(Key, ToJson());
    }
}
