// Every rule of a run and its junit test case: a line-for-line port of `rules`, `cases`,
// `describe` and `position` in crates/rb-report/src/catalog.rs, so each RuleResult carries the
// message the junit reporter writes for the same rule. The adapter evaluates nothing: it reads
// `summary.ruleSetUsed`, `summary.violations`, `summary.vacuousRules`, `summary.ratchets` and
// `summary.expired` from the binary's JSON result. Rule names repeat (every anonymous rule is
// `unnamed`), so each rule has an Id, the test's name (`unnamed`, `unnamed#2`, ...), and each
// violation goes to one rule of its name, exactly as catalog.rs's `identify` and `rule_index` do.
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
    internal sealed record Rule(string Name, string Family, string Severity, string? Comment, string? Fix)
    {
        /// <summary>The rule's identity within the run: the name, with <c>#n</c> for the n-th rule of a name already taken.</summary>
        public string Id { get; init; } = Name;
    }

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
            string? kind = JsValue.String(violation, "type");
            if (!rules.Concat(unlisted).Any(r => r.Name == name && Produces(r.Family, kind)))
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

        return Identify(rules);
    }

    /// <summary>
    /// The index of the rule a violation belongs to, as catalog.rs's <c>rule_index</c> picks it:
    /// among the non-ratchet rules of its name, the first whose family can produce its <c>type</c>
    /// and whose severity is its own, else the first whose family can produce it, else the first.
    /// </summary>
    public static int? RuleIndex(IReadOnlyList<Rule> rules, JsonElement violation)
    {
        ArgumentNullException.ThrowIfNull(rules);
        string name = RuleName(violation);
        List<int> named = [.. Enumerable.Range(0, rules.Count).Where(i => rules[i].Name == name && rules[i].Family != "ratchets")];
        string? kind = JsValue.String(violation, "type");
        List<int> fitting = [.. named.Where(i => Produces(rules[i].Family, kind))];
        if (fitting.Count == 0)
        {
            fitting = named;
        }

        string severity = JsValue.Severity(violation);
        foreach (int i in fitting)
        {
            if (rules[i].Severity == severity)
            {
                return i;
            }
        }

        return fitting.Count > 0 ? fitting[0] : null;
    }

    /// <summary>catalog.rs's <c>identify</c>: the name at its first occurrence, then <c>name#n</c>, past any id taken.</summary>
    private static List<Rule> Identify(List<Rule> rules)
    {
        HashSet<string> taken = [.. rules.Select(static r => r.Name)];
        Dictionary<string, int> seen = new(StringComparer.Ordinal);
        List<Rule> identified = [];
        foreach (Rule rule in rules)
        {
            int count = seen.GetValueOrDefault(rule.Name) + 1;
            seen[rule.Name] = count;
            if (count == 1)
            {
                identified.Add(rule with { Id = rule.Name });
                continue;
            }

            int n = count;
            while (taken.Contains(string.Create(CultureInfo.InvariantCulture, $"{rule.Name}#{n}")))
            {
                n++;
            }

            string id = string.Create(CultureInfo.InvariantCulture, $"{rule.Name}#{n}");
            taken.Add(id);
            identified.Add(rule with { Id = id });
        }

        return identified;
    }

    /// <summary>catalog.rs's <c>produces</c>: whether a rule of <paramref name="family"/> can produce a violation of <paramref name="kind"/>.</summary>
    private static bool Produces(string family, string? kind) => family == "rules" || kind switch
    {
        "element" => family is "elements" or "diagrams",
        "slice" => family == "slices",
        _ => family is "forbidden" or "allowed" or "required",
    };

    /// <summary>One result per rule of <see cref="Rules"/>, then one per expired known violation.</summary>
    public static List<RuleResult> Cases(JsonElement result)
    {
        IReadOnlyList<JsonElement> vacuous = List(result, "vacuousRules");
        IReadOnlyList<JsonElement> expired = List(result, "expired");
        IReadOnlyList<JsonElement> ratchets = List(result, "ratchets");
        List<RuleResult> cases = [];
        List<Rule> rules = Rules(result);
        int ratchetAt = 0;
        for (int index = 0; index < rules.Count; index++)
        {
            Rule rule = rules[index];
            (string Message, string Detail)? failure = null;
            List<RuleError> errors = [];
            List<string> output = [];
            string name = rule.Name;

            // Vacuous and expired entries go to the first rule of their name.
            bool first = rules.FindIndex(r => r.Name == name) == index;
            if (rule.Family == "ratchets")
            {
                // The ratchet rules are summary.ratchets, in order.
                if (ratchetAt < ratchets.Count)
                {
                    failure = Ratchet(name, ratchets[ratchetAt], errors, output);
                }

                ratchetAt++;
            }
            else
            {
                int current = index;
                List<JsonElement> found = [.. Violations(result).Where(v => RuleIndex(rules, v) == current)];
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

            foreach (JsonElement entry in vacuous.Where(v => first && JsValue.Text(v, "name") == name))
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

            foreach (JsonElement entry in expired.Where(e => first && JsValue.Text(e, "kind") == "rule" && JsValue.Text(e, "name") == name))
            {
                errors.Add(new RuleError("expired", ExpiredMessage(entry)));
            }

            cases.Add(new RuleResult(
                rule.Id,
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
