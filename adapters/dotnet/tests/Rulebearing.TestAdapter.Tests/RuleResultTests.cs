// RuleResult: its outcome, its message, Assert, the framework hook and serialization.

using Xunit;

namespace Rulebearing.TestAdapter.Tests;

/// <summary>What a rule result says and how it fails.</summary>
public sealed class RuleResultTests
{
    private static RuleResult Make(string? failure = null, params RuleError[] errors) =>
        new("r", "forbidden", "error", "c", "f", failure, failure is null ? null : "detail", errors, ["warn: x"]);

    /// <summary>A rule with neither failure nor error passes and Assert returns.</summary>
    [Fact]
    public void APassingRuleAssertsNothing()
    {
        RuleResult rule = Make();
        Assert.Equal(RuleOutcome.Passed, rule.Outcome);
        Assert.Null(rule.Message);
        rule.Assert();
        Assert.Equal("r", rule.ToString());
        Assert.Equal(("r", "forbidden", "error", "c", "f"), (rule.Name, rule.Family, rule.Severity, rule.Comment, rule.Fix));
        Assert.Equal(["warn: x"], rule.Output);
    }

    /// <summary>A failure throws with the failure message.</summary>
    [Fact]
    public void AFailureThrowsItsMessage()
    {
        RuleResult rule = Make("fix\nRB-1 a -> b");
        Assert.Equal(RuleOutcome.Failed, rule.Outcome);
        Assert.Equal("detail", rule.FailureDetail);
        Assert.Equal("fix\nRB-1 a -> b", Assert.Throws<RuleFailedException>(rule.Assert).Message);
    }

    /// <summary>An error outranks a failure, and the message carries both.</summary>
    [Fact]
    public void AnErrorOutranksAFailure()
    {
        RuleResult rule = Make("failed", new RuleError("expired", "gone"), new RuleError("vacuous", "empty"));
        Assert.Equal(RuleOutcome.Error, rule.Outcome);
        Assert.Equal("failed\ngone\nempty", rule.Message);
    }

    /// <summary>A framework package supplies its own assertion exception.</summary>
    [Fact]
    public void WithAssertionThrowsTheFrameworksException()
    {
        RuleResult rule = Make("failed").WithAssertion(static m => new InvalidOperationException($"framework: {m}"));
        Assert.Equal("framework: failed", Assert.Throws<InvalidOperationException>(rule.Assert).Message);
        Assert.Throws<ArgumentNullException>(() => rule.WithAssertion(null!));
    }

    /// <summary>ToJson and FromJson round-trip every field.</summary>
    [Fact]
    public void SerializationRoundTrips()
    {
        RuleResult rule = Make("failed", new RuleError("vacuous", "empty"));
        RuleResult back = RuleResult.FromJson(rule.ToJson());
        Assert.Equal(rule.ToJson(), back.ToJson());
        Assert.Equal(rule.Errors, back.Errors);
        Assert.Equal("failed\nempty", Assert.Throws<RuleFailedException>(back.Assert).Message);
        RuleResult bare = RuleResult.FromJson("""{"Name":"n","Family":"f","Severity":"s"}""");
        Assert.Empty(bare.Errors);
        Assert.Empty(bare.Output);
    }

    /// <summary>FromJson refuses what ToJson did not write.</summary>
    /// <param name="json">The text.</param>
    [Theory]
    [InlineData("not json")]
    [InlineData("null")]
    [InlineData("{\"Name\":\"n\"}")]
    public void FromJsonRefusesOtherText(string json) =>
        Assert.Throws<RulebearingException>(() => RuleResult.FromJson(json));

    /// <summary>The constructor refuses null.</summary>
    [Fact]
    public void TheConstructorRefusesNull()
    {
        Assert.Throws<ArgumentNullException>(() => new RuleResult(null!, "f", "s", null, null, null, null, [], []));
        Assert.Throws<ArgumentNullException>(() => new RuleResult("n", null!, "s", null, null, null, null, [], []));
        Assert.Throws<ArgumentNullException>(() => new RuleResult("n", "f", null!, null, null, null, null, [], []));
        Assert.Throws<ArgumentNullException>(() => new RuleResult("n", "f", "s", null, null, null, null, null!, []));
        Assert.Throws<ArgumentNullException>(() => new RuleResult("n", "f", "s", null, null, null, null, [], null!));
        Assert.Throws<ArgumentNullException>(() => RuleResult.FromJson(null!));
    }

    /// <summary>A framework subclass restores a result from its serialized form.</summary>
    [Fact]
    public void ASubclassRestoresAResult()
    {
        Restored restored = new(Make("failed"));
        Assert.Equal("failed", Assert.Throws<NotSupportedException>(restored.Assert).Message);
        Assert.Throws<ArgumentNullException>(() => new Restored(null!));
    }

    /// <summary>The exception types carry their messages and causes.</summary>
    [Fact]
    public void ExceptionsCarryMessagesAndCauses()
    {
        InvalidOperationException cause = new("cause");
        Assert.Same(cause, new RuleFailedException("m", cause).InnerException);
        Assert.Same(cause, new RulebearingException("m", cause).InnerException);
        Assert.NotNull(new RuleFailedException().Message);
        Assert.NotNull(new RulebearingException().Message);
    }

    private sealed class Restored : RuleResult
    {
        public Restored(RuleResult other) => Restore(other, static m => new NotSupportedException(m));
    }
}
