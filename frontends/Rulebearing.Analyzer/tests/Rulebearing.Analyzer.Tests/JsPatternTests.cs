// The analyzer's patterns against the compatibility cases rb-config proves for the gate
// (crates/rb-config/src/pattern.rs, exported as regex-compatibility.json): the same pattern
// matches the same subject on both sides, and the same patterns are refused.
// Decision: docs/adr/0016-linear-time-regex-and-strict-compat.md.

using System.Text.Json;

namespace Rulebearing.Analyzer.Tests;

/// <summary>The analyzer's patterns against the gate's compatibility cases.</summary>
public sealed class JsPatternTests
{
    private static JsonDocument Table() =>
        JsonDocument.Parse(File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "regex-compatibility.json")));

    /// <summary>Every compatibility case matches as the gate does.</summary>
    [Fact]
    public void EveryCompatibilityCaseMatchesAsTheGateDoes()
    {
        using var table = Table();
        var cases = table.RootElement.GetProperty("cases").EnumerateArray().ToList();
        Assert.Equal(53, cases.Count);
        foreach (var c in cases)
        {
            var pattern = c.GetProperty("pattern").GetString()!;
            var subject = c.GetProperty("subject").GetString()!;
            var expected = c.GetProperty("matches").GetBoolean();
            Assert.True(JsPattern.Compile(pattern).IsMatch(subject) == expected, $"{c.GetProperty("row").GetString()}: {pattern} on {subject}");
        }
    }

    /// <summary>Every refused pattern is refused.</summary>
    [Fact]
    public void EveryRefusedPatternIsRefused()
    {
        using var table = Table();
        foreach (var r in table.RootElement.GetProperty("refused").EnumerateArray())
        {
            var pattern = r.GetProperty("pattern").GetString()!;
            Assert.Throws<PatternException>(() => JsPattern.Compile(pattern));
        }
    }

    /// <summary>Groups are the match and each participating group.</summary>
    [Theory]
    [InlineData("^src/(a|b)/", "src/b/x.ts", new[] { "src/b/", "b" })]
    [InlineData("^(?<app>[^/]+)/", "web/x", new[] { "web/", "web" })]
    [InlineData("^lib/", "src/x", new string[0])]
    public void GroupsAreTheMatchAndEachParticipatingGroup(string pattern, string subject, string[] expected) =>
        Assert.Equal(expected, JsPattern.Compile(pattern).Groups(subject));

    /// <summary>A reference to a missing group or a stray parenthesis is refused.</summary>
    [Theory]
    [InlineData(@"(a)\2")]
    [InlineData("a)")]
    public void AReferenceToAMissingGroupOrAStrayParenthesisIsRefused(string pattern) =>
        Assert.Throws<PatternException>(() => JsPattern.Compile(pattern));

    /// <summary>Backreferences end of input and class escapes.</summary>
    [Theory]
    [InlineData(@"^(a)\1$", "aa", true)]
    [InlineData(@"^(?<x>b)\k<x>$", "bb", true)]
    [InlineData(@"^a$", "a\n", false)]
    [InlineData(@"\ka", "ka", true)]
    [InlineData(@"^[\s]$", "　", true)]
    [InlineData(@"^[\S]$", " ", false)]
    [InlineData(@"^[\w-]+$", "a-b", true)]
    [InlineData(@"^[\W]$", "a", false)]
    public void BackreferencesEndOfInputAndClassEscapes(string pattern, string subject, bool expected) =>
        Assert.Equal(expected, JsPattern.Compile(pattern).IsMatch(subject));
}
