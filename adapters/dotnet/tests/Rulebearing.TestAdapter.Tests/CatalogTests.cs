// The catalog port on the vectors of crates/rb-report/src/catalog.rs and junit.rs's own tests, so
// the C# and the Rust are held to the same expectations.

using System.Text.Json;
using Xunit;

namespace Rulebearing.TestAdapter.Tests;

/// <summary>The port of <c>catalog.rs</c>, on its own test vectors.</summary>
public sealed class CatalogTests
{
    private const string Result = """
        {
          "modules": [{ "source": "a.ts", "dependencies": [{ "resolved": "b.ts", "line": 3, "column": 8 }] }],
          "code": { "types": [{ "fullName": "S.A", "line": 7, "column": 2 }, { "fullName": "S.B" }] },
          "summary": {
            "violations": [
              { "type": "dependency", "from": "a.ts", "to": "b.ts", "rule": { "name": "no-b", "severity": "error" }, "id": "RB-1" },
              { "type": "element", "from": "a.cs", "to": "S.A", "rule": { "name": "sealed", "severity": "ignore" }, "id": "RB-2", "fix": "Seal it." },
              { "type": "element", "from": "b.cs", "to": "S.B", "rule": { "name": "sealed", "severity": "error" } },
              { "type": "dependency", "from": "x", "to": "y", "rule": { "name": "zeta", "severity": "warn" }, "comment": "z" },
              { "type": "dependency", "from": "x", "to": "z", "rule": { "name": "alpha", "severity": "info" } }
            ],
            "ruleSetUsed": {
              "forbidden": [{ "name": "no-b", "severity": "error", "comment": "c", "fix": "f" }, { "name": "quiet" }],
              "allowed": [{ "from": {}, "to": {}, "comment": "only these", "fix": "Use one." }],
              "required": [{ "name": "needs", "severity": "info" }],
              "elements": [{ "name": "sealed", "severity": "error" }],
              "slices": [{ "name": "apart", "severity": "warn" }],
              "diagrams": [{ "name": "drawn", "severity": "error" }]
            },
            "ratchets": [{ "name": "budget", "budget": "b.json", "count": 3, "ceiling": 2, "status": "exceeded" }],
            "vacuousRules": [{ "name": "quiet", "side": "from" }, { "name": "allowed[0]", "side": "from" }]
          }
        }
        """;

    /// <summary>Every rule, in configuration order, then the unlisted ones in name order.</summary>
    [Fact]
    public void EveryRuleInConfigurationOrderThenTheUnlistedOnes()
    {
        using JsonDocument document = JsonDocument.Parse(Result);
        List<Catalog.Rule> rules = Catalog.Rules(document.RootElement);
        Assert.Equal(
            [
                ("no-b", "forbidden", "error"),
                ("quiet", "forbidden", "warn"),
                ("not-in-allowed", "allowed", "warn"),
                ("needs", "required", "info"),
                ("sealed", "elements", "error"),
                ("apart", "slices", "warn"),
                ("drawn", "diagrams", "error"),
                ("alpha", "rules", "info"),
                ("zeta", "rules", "warn"),
                ("budget", "ratchets", "error"),
                ("allowed[0]", "rules", "error"),
            ],
            rules.Select(static r => (r.Name, r.Family, r.Severity)));
        Assert.Equal("c", rules[0].Comment);
        Assert.Equal("Use one.", rules[2].Fix);
        Assert.Equal("z", rules[8].Comment);

        using JsonDocument strict = JsonDocument.Parse(Result.Replace("\"ruleSetUsed\": {", "\"ruleSetUsed\": { \"allowedSeverity\": \"error\",", StringComparison.Ordinal));
        Assert.Contains(Catalog.Rules(strict.RootElement), static r => r.Name == "not-in-allowed" && r.Severity == "error");
        using JsonDocument empty = JsonDocument.Parse("{}");
        Assert.Empty(Catalog.Rules(empty.RootElement));
    }

    /// <summary>Positions and one-line descriptions of violations.</summary>
    [Fact]
    public void ViolationsPositionsAndDescriptions()
    {
        using JsonDocument document = JsonDocument.Parse(Result);
        JsonElement result = document.RootElement;
        JsonElement[] all = [.. result.GetProperty("summary").GetProperty("violations").EnumerateArray()];
        Assert.Equal((3UL, 8UL), Catalog.Position(result, all[0]));
        Assert.Equal((7UL, 2UL), Catalog.Position(result, all[1]));
        Assert.Null(Catalog.Position(result, all[2]));
        Assert.Null(Catalog.Position(result, all[3]));
        Assert.Equal("RB-1 a.ts -> b.ts (line 3, column 8)", Catalog.Describe(result, all[0]));
        Assert.Equal("RB-2 a.cs -> S.A (line 7, column 2) [known]", Catalog.Describe(result, all[1]));
        Assert.Equal("b.cs -> S.B", Catalog.Describe(result, all[2]));
    }

    /// <summary>Positions the extractor did not record, or that the document cannot give.</summary>
    [Theory]
    [InlineData("""{ "code": {} }""", """{ "type": "element", "to": "S.A" }""")]
    [InlineData("""{ "code": { "types": [{ "fullName": "S.A", "column": 4 }] } }""", """{ "type": "element", "to": "S.A" }""")]
    [InlineData("""{ "code": { "types": [{ "fullName": "S.X", "line": 1 }] } }""", """{ "type": "element", "to": "S.A" }""")]
    [InlineData("""{ }""", """{ "from": "a", "to": "b" }""")]
    [InlineData("""{ "modules": [{ "source": "a" }] }""", """{ "from": "a", "to": "b" }""")]
    [InlineData("""{ "modules": [{ "source": "a", "dependencies": [{ "resolved": "c", "line": 1, "column": 1 }] }] }""", """{ "from": "a", "to": "b" }""")]
    [InlineData("""{ "modules": [{ "source": "a", "dependencies": [{ "resolved": "b", "line": 1 }] }] }""", """{ "from": "a", "to": "b" }""")]
    [InlineData("""{ "modules": [{ "source": "a", "dependencies": [] }, { "source": "a", "dependencies": [{ "resolved": "b", "line": 1, "column": 1 }] }] }""", """{ "from": "a", "to": "b" }""")]
    public void NoPositionWithoutARecordedOne(string result, string violation)
    {
        using JsonDocument r = JsonDocument.Parse(result);
        using JsonDocument v = JsonDocument.Parse(violation);
        Assert.Null(Catalog.Position(r.RootElement, v.RootElement));
    }

    /// <summary>An element declared without a column is at column 1.</summary>
    [Fact]
    public void AnElementWithoutAColumnIsAtColumnOne()
    {
        using JsonDocument r = JsonDocument.Parse("""{ "code": { "types": [{ "fullName": "S.A", "line": 9 }] } }""");
        using JsonDocument v = JsonDocument.Parse("""{ "type": "element", "to": "S.A" }""");
        Assert.Equal((9UL, 1UL), Catalog.Position(r.RootElement, v.RootElement));
    }

    /// <summary>Ratchets, expiry and long failures become cases, as in catalog.rs.</summary>
    [Fact]
    public void RatchetsExpiryAndLongFailuresBecomeCases()
    {
        string many = string.Join(',', Enumerable.Range(0, 7).Select(static i => $$"""{ "type": "dependency", "from": "f{{i}}", "to": "t", "rule": { "name": "wide", "severity": "error" } }"""));
        string json = $$"""
            { "summary": {
              "violations": [{{many}}],
              "ruleSetUsed": { "forbidden": [{ "name": "wide", "severity": "error" }, { "name": "old", "expires": "2020-01-01" }, { "name": "soft" }] },
              "ratchets": [
                { "name": "over", "budget": "o.json", "count": 3, "ceiling": 2, "status": "exceeded" },
                { "name": "lost", "budget": "l.json", "count": 1, "status": "no-budget" },
                { "name": "held", "budget": "h.json", "count": 1, "ceiling": 4, "status": "held" }
              ],
              "vacuousRules": [{ "name": "soft", "side": "from", "severity": "warn" }],
              "expired": [{ "name": "old", "expires": "2020-01-01", "kind": "rule" }]
            } }
            """;
        using JsonDocument document = JsonDocument.Parse(json);
        List<RuleResult> cases = Catalog.Cases(document.RootElement);
        RuleResult By(string name) => cases.Single(c => c.Name == name);

        Assert.Equal(
            "7 violation(s) of `wide`\nf0 -> t\nf1 -> t\nf2 -> t\nf3 -> t\nf4 -> t\n... and 2 more",
            By("wide").FailureMessage);
        Assert.Equal(7, By("wide").FailureDetail!.Split('\n').Length);
        Assert.Equal(
            [new RuleError("expired", "rule `old` expired on 2020-01-01; it no longer applies and the run fails")],
            By("old").Errors);
        Assert.Empty(By("soft").Errors);
        Assert.Equal(
            ["warning: rule `soft` is vacuous: its from side matched nothing, so it checks nothing (ADR-0007)"],
            By("soft").Output);
        Assert.Equal("ratchet `over`: 3 edges exceed the ceiling of 2 in o.json", By("over").FailureMessage);
        Assert.Equal("no-budget", By("lost").Errors[0].Kind);
        Assert.Null(By("lost").FailureMessage);
        Assert.Equal(["1 edges, within the ceiling of 4 in h.json"], By("held").Output);
        Assert.Equal(RuleOutcome.Passed, By("held").Outcome);
        Assert.Equal(6, cases.Count);
    }

    /// <summary>The junit.rs vector: failures, a vacuous error, warn output and an expired known violation.</summary>
    [Fact]
    public void TheJunitVector()
    {
        const string json = """
            {
              "modules": [{ "source": "a.ts", "dependencies": [{ "resolved": "b.ts", "line": 2, "column": 1 }] }],
              "summary": {
                "violations": [
                  { "type": "dependency", "from": "a.ts", "to": "b.ts", "rule": { "name": "no-b", "severity": "error" }, "id": "RB-1" },
                  { "type": "dependency", "from": "c.ts", "to": "b.ts", "rule": { "name": "no-b", "severity": "ignore" }, "id": "RB-2" },
                  { "type": "element", "from": "s.cs", "to": "S<T>", "rule": { "name": "sealed", "severity": "warn" }, "id": "RB-3" }
                ],
                "ruleSetUsed": {
                  "forbidden": [{ "name": "no-b", "severity": "error", "fix": "Use \"the\" index & go." }, { "name": "dead" }],
                  "elements": [{ "name": "sealed", "severity": "warn" }]
                },
                "vacuousRules": [{ "name": "dead", "side": "from" }],
                "expired": [{ "name": "RB-9", "expires": "2026-01-01", "kind": "knownViolation" }]
              }
            }
            """;
        IReadOnlyList<RuleResult> cases = RulebearingRun.Parse(json);
        Assert.Equal(["no-b", "dead", "sealed", "RB-9"], cases.Select(static c => c.Name));
        Assert.Equal("Use \"the\" index & go.\nRB-1 a.ts -> b.ts (line 2, column 1)", cases[0].Message);
        Assert.Equal(["ignore: RB-2 c.ts -> b.ts [known]"], cases[0].Output);
        Assert.Equal("rule `dead` is vacuous: its from side matched nothing, so it checks nothing (ADR-0007)", cases[1].Message);
        Assert.Equal(RuleOutcome.Passed, cases[2].Outcome);
        Assert.Equal(["warn: RB-3 s.cs -> S<T>"], cases[2].Output);
        Assert.Equal("knownViolations", cases[3].Family);
        Assert.Equal("knownViolation `RB-9` expired on 2026-01-01; it no longer applies and the run fails", cases[3].Message);
    }

    /// <summary>A failing rule with no fix of its own takes a violation's fix, or counts its violations.</summary>
    [Fact]
    public void TheFixComesFromTheFirstViolationOrTheRule()
    {
        const string json = """
            { "summary": {
              "violations": [
                { "from": "a", "to": "b", "rule": { "name": "r", "severity": "warn" }, "fix": "From the warning." },
                { "from": "c", "to": "d", "rule": { "name": "r", "severity": "error" } },
                { "from": "e", "to": "f", "rule": { "name": "u", "severity": "error" }, "comment": "unlisted" },
                { "from": "g", "to": "h", "rule": { "name": "odd" } },
                { "from": "i", "to": "j", "rule": "not an object" }
              ],
              "ruleSetUsed": { "forbidden": [{ "name": "r", "severity": "error", "fix": "From the rule." }] }
            } }
            """;
        IReadOnlyList<RuleResult> cases = RulebearingRun.Parse(json);
        Assert.Equal("From the warning.\nc -> d", cases[0].Message);
        Assert.Equal(["", "odd", "u"], cases.Skip(1).Select(static c => c.Name));
        Assert.Equal("undefined", cases[1].Severity);
        Assert.Equal("undefined", cases[2].Severity);
        Assert.Equal("1 violation(s) of `u`\ne -> f", cases[3].Message);
        Assert.Equal("unlisted", cases[3].Comment);
    }
}
