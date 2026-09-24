// Every rule of a run and its junit test case: a line-for-line port of `rules`, `cases`,
// `describe` and `position` in crates/rb-report/src/catalog.rs, so each RuleResult carries the
// message the junit reporter writes for the same rule. The adapter evaluates nothing: it reads
// `summary.ruleSetUsed`, `summary.violations`, `summary.vacuousRules`, `summary.ratchets` and
// `summary.expired` from the binary's JSON result.
//
// Contract: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, section 1.5 (the test
// adapters' message is the junit text). Proof: tests/Rulebearing.TestAdapter.Tests/JunitEqualityTests.cs
// compares every message with `rulebearing cruise -T junit` over the same inputs.

using System.Globalization;
using System.Text;
using System.Text.Json;

namespace Rulebearing.TestAdapter;

/// <summary>The rules of a run and their <c>junit</c> test cases.</summary>
internal static class Catalog
{
    /// <summary>How many violations a failure message lists before it says how many more there are.</summary>
    public const int Shown = 5;

    private static readonly string[] ElementFamilies = ["required", "elements", "slices", "diagrams"];

    /// <summary>One rule of the run.</summary>
    internal sealed record Rule(string Name, string Family, string Severity, string? Comment, string? Fix);

    /// <summary>Every rule of the run, in configuration order, then those known only from violations.</summary>
    public static List<Rule> Rules(JsonElement result)
    {
        List<Rule> rules = [];
        JsonElement? summary = JsValue.Get(result, "summary");
        JsonElement? ruleSet = JsValue.Get(summary, "ruleSetUsed");
        Rule Entry(string family, JsonElement rule) => new(
            JsValue.String(rule, "name") ?? string.Empty,
            family,
            JsValue.String(rule, "severity") ?? "warn",
            JsValue.String(rule, "comment"),
            JsValue.String(rule, "fix"));

        foreach (JsonElement rule in JsValue.Array(ruleSet, "forbidden"))
        {
            rules.Add(Entry("forbidden", rule));
        }

        IReadOnlyList<JsonElement> allowed = JsValue.Array(ruleSet, "allowed");
        if (allowed.Count > 0)
        {
            rules.Add(new Rule(
                "not-in-allowed",
                "allowed",
                JsValue.String(ruleSet, "allowedSeverity") ?? "warn",
                JsValue.String(allowed[0], "comment"),
                JsValue.String(allowed[0], "fix")));
        }

        foreach (string family in ElementFamilies)
        {
            foreach (JsonElement rule in JsValue.Array(ruleSet, family))
            {
                rules.Add(Entry(family, rule));
            }
        }

        List<Rule> unlisted = [];
        foreach (JsonElement violation in Violations(result))
        {
            string name = RuleName(violation);
            if (!rules.Concat(unlisted).Any(r => r.Name == name))
            {
                unlisted.Add(new Rule(
                    name,
                    "rules",
                    JsValue.Severity(violation),
                    JsValue.String(violation, "comment"),
                    JsValue.String(violation, "fix")));
            }
        }

        rules.AddRange(unlisted.OrderBy(static r => r.Name, StringComparer.Ordinal));
        foreach (JsonElement ratchet in List(result, "ratchets"))
        {
            rules.Add(new Rule(JsValue.Text(ratchet, "name"), "ratchets", "error", null, null));
        }

        foreach (JsonElement vacuous in List(result, "vacuousRules"))
        {
            string name = JsValue.Text(vacuous, "name");
            if (!rules.Any(r => r.Name == name))
            {
                rules.Add(new Rule(name, "rules", "error", null, null));
            }
        }

        return rules;
    }

    /// <summary>One result per rule of <see cref="Rules"/>, then one per expired known violation.</summary>
    public static List<RuleResult> Cases(JsonElement result)
    {
        IReadOnlyList<JsonElement> vacuous = List(result, "vacuousRules");
        IReadOnlyList<JsonElement> expired = List(result, "expired");
        IReadOnlyList<JsonElement> ratchets = List(result, "ratchets");
        List<RuleResult> cases = [];
        foreach (Rule rule in Rules(result))
        {
            (string Message, string Detail)? failure = null;
            List<RuleError> errors = [];
            List<string> output = [];
            string name = rule.Name;
            if (rule.Family == "ratchets")
            {
                foreach (JsonElement ratchet in ratchets.Where(r => JsValue.Text(r, "name") == name).Take(1))
                {
                    failure = Ratchet(name, ratchet, errors, output);
                }
            }
            else
            {
                List<JsonElement> found = [.. Violations(result).Where(v => RuleName(v) == name)];
                List<string> failing = [.. found.Where(static v => JsValue.Severity(v) == "error").Select(v => Describe(result, v))];
                foreach (JsonElement v in found.Where(static v => JsValue.Severity(v) != "error"))
                {
                    output.Add($"{JsValue.Severity(v)}: {Describe(result, v)}");
                }

                if (failing.Count > 0)
                {
                    StringBuilder message = new(
                        (found.Count > 0 ? FixOf(found[0], rule) : null)
                        ?? $"{failing.Count.ToString(CultureInfo.InvariantCulture)} violation(s) of `{name}`");
                    foreach (string line in failing.Take(Shown))
                    {
                        message.Append('\n').Append(line);
                    }

                    if (failing.Count > Shown)
                    {
                        message.Append(CultureInfo.InvariantCulture, $"\n... and {failing.Count - Shown} more");
                    }

                    failure = (message.ToString(), string.Join('\n', failing));
                }
            }

            foreach (JsonElement entry in vacuous.Where(v => JsValue.Text(v, "name") == name))
            {
                if (JsValue.String(entry, "severity") == "warn")
                {
                    output.Add($"warning: {VacuousMessage(entry)}");
                }
                else
                {
                    errors.Add(new RuleError("vacuous", VacuousMessage(entry)));
                }
            }

            foreach (JsonElement entry in expired.Where(e => JsValue.Text(e, "kind") == "rule" && JsValue.Text(e, "name") == name))
            {
                errors.Add(new RuleError("expired", ExpiredMessage(entry)));
            }

            cases.Add(new RuleResult(
                rule.Name,
                rule.Family,
                rule.Severity,
                rule.Comment,
                rule.Fix,
                failure?.Message,
                failure?.Detail,
                errors,
                output));
        }

        foreach (JsonElement entry in expired.Where(static e => JsValue.Text(e, "kind") != "rule"))
        {
            cases.Add(new RuleResult(
                JsValue.Text(entry, "name"),
                "knownViolations",
                "error",
                null,
                null,
                null,
                null,
                [new RuleError("expired", ExpiredMessage(entry))],
                []));
        }

        return cases;
    }

    /// <summary>One line per violation: its id, <c>from -&gt; to</c> and where it sits.</summary>
    public static string Describe(JsonElement result, JsonElement violation)
    {
        string id = JsValue.String(violation, "id") is { } s ? $"{s} " : string.Empty;
        string from = JsValue.Text(violation, "from");
        string to = JsValue.Text(violation, "to");
        string at = Position(result, violation) is { } p
            ? string.Create(CultureInfo.InvariantCulture, $" (line {p.Line}, column {p.Column})")
            : string.Empty;
        string known = JsValue.Severity(violation) == "ignore" ? " [known]" : string.Empty;
        return $"{id}{from} -> {to}{at}{known}";
    }

    /// <summary>
    /// Where a violation sits: the declaration of the type for an element violation, the edge's
    /// line and column for a dependency, when the extractor recorded them.
    /// </summary>
    public static (ulong Line, ulong Column)? Position(JsonElement result, JsonElement violation)
    {
        string from = JsValue.Text(violation, "from");
        string to = JsValue.Text(violation, "to");
        if (JsValue.String(violation, "type") == "element")
        {
            JsonElement? types = JsValue.Get(JsValue.Get(result, "code"), "types");
            if (types is not { ValueKind: JsonValueKind.Array } list)
            {
                return null;
            }

            foreach (JsonElement type in list.EnumerateArray())
            {
                if (JsValue.String(type, "fullName") == to)
                {
                    return JsValue.UInt(type, "line") is { } line ? (line, JsValue.UInt(type, "column") ?? 1) : null;
                }
            }

            return null;
        }

        return EdgePosition(result, from, to);
    }

    private static (ulong Line, ulong Column)? EdgePosition(JsonElement result, string from, string to)
    {
        if (JsValue.Get(result, "modules") is not { ValueKind: JsonValueKind.Array } modules)
        {
            return null;
        }

        foreach (JsonElement module in modules.EnumerateArray())
        {
            if (JsValue.String(module, "source") != from)
            {
                continue;
            }

            if (JsValue.Get(module, "dependencies") is not { ValueKind: JsonValueKind.Array } dependencies)
            {
                return null;
            }

            foreach (JsonElement dependency in dependencies.EnumerateArray())
            {
                if (JsValue.String(dependency, "resolved") == to)
                {
                    return JsValue.UInt(dependency, "line") is { } line && JsValue.UInt(dependency, "column") is { } column
                        ? (line, column)
                        : null;
                }
            }

            return null;
        }

        return null;
    }

    private static IReadOnlyList<JsonElement> Violations(JsonElement result) =>
        JsValue.Array(JsValue.Get(result, "summary"), "violations");

    private static IReadOnlyList<JsonElement> List(JsonElement result, string key) =>
        JsValue.Array(JsValue.Get(result, "summary"), key);

    private static string RuleName(JsonElement violation) =>
        JsValue.String(JsValue.Get(violation, "rule"), "name") ?? string.Empty;

    private static string? FixOf(JsonElement violation, Rule rule) =>
        JsValue.String(violation, "fix") ?? rule.Fix;

    private static string VacuousMessage(JsonElement entry) =>
        $"rule `{JsValue.Text(entry, "name")}` is vacuous: its {JsValue.Text(entry, "side")} side matched nothing, so it checks nothing (ADR-0007)";

    private static string ExpiredMessage(JsonElement entry) =>
        $"{JsValue.Text(entry, "kind")} `{JsValue.Text(entry, "name")}` expired on {JsValue.Text(entry, "expires")}; it no longer applies and the run fails";

    private static (string Message, string Detail)? Ratchet(string name, JsonElement ratchet, List<RuleError> errors, List<string> output)
    {
        string count = JsValue.Print(JsValue.Get(ratchet, "count"));
        string budget = JsValue.Text(ratchet, "budget");
        switch (JsValue.String(ratchet, "status"))
        {
            case "exceeded":
                string message = $"ratchet `{name}`: {count} edges exceed the ceiling of {JsValue.Print(JsValue.Get(ratchet, "ceiling"))} in {budget}";
                return (message, message);
            case "no-budget":
                errors.Add(new RuleError(
                    "no-budget",
                    $"ratchet `{name}`: the budget {budget} cannot be read, so the count {count} is checked against nothing"));
                return null;
            default:
                output.Add($"{count} edges, within the ceiling of {JsValue.Print(JsValue.Get(ratchet, "ceiling"))} in {budget}");
                return null;
        }
    }
}
