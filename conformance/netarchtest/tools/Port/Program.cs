// Ports NetArchTest 1.3.2's unit tests (test/NetArchTest.Rules.UnitTests) to conformance gate 2 and
// writes ported/<TestClass>.yaml, ported.json and unported.json under the directory it is given.
// Each case's verdict is computed by NetArchTest itself over the committed fixtures (Oracle.cs), and
// each rule is the mapping in Rules.cs. Output is deterministic: rerunning gives identical files.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7.
// Decisions: docs/adr/0009-conformance-suites-as-specification.md, docs/adr/0019-mit-licence.md.
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using Rulebearing.Conformance.NetArchTest;

const string Pin = "1.3.2";
const string SourceDirectory = "test/NetArchTest.Rules.UnitTests";

if (args.Length != 1)
{
    Console.Error.WriteLine("usage: Port <conformance/netarchtest directory>");
    return 2;
}
var gate = args[0];
var oracle = new Oracle();
PredicateCases.Register(oracle);
ConditionCases.Register(oracle);
ListCases.Register(oracle);
TypesCases.Register(oracle);
DependencySearchCases.Register(oracle);

// Block numbers: a test with one entry is its own id; a test with several numbers them from 1.
var perTest = oracle.Entries.GroupBy(e => (e.File, e.Test)).ToDictionary(g => g.Key, g => g.Count());
var seen = new Dictionary<(string, string), int>();
string Id(Entry e, out int? block)
{
    var key = (e.File, e.Test);
    seen[key] = seen.GetValueOrDefault(key) + 1;
    block = perTest[key] == 1 ? null : seen[key];
    return block is null ? e.Test : $"{e.Test}#{block}";
}

var ported = Path.Combine(gate, "ported");
Directory.CreateDirectory(ported);
foreach (var stale in Directory.GetFiles(ported, "*.yaml"))
{
    File.Delete(stale);
}
var unported = new JsonArray();
var files = new SortedDictionary<string, StringBuilder>(StringComparer.Ordinal);
var cases = 0;
foreach (var entry in oracle.Entries)
{
    var id = Id(entry, out var block);
    var source = $"{SourceDirectory}/{entry.File.Replace('.', '/')}.cs";
    if (entry.Reason is not null)
    {
        unported.Add(new JsonObject
        {
            ["source"] = source,
            ["test"] = entry.Test,
            ["block"] = block,
            ["query"] = block is null ? null : entry.CSharp,
            ["reason"] = entry.Reason,
        });
        continue;
    }
    cases++;
    if (!files.TryGetValue(entry.File, out var text))
    {
        text = new StringBuilder()
            .Append("# Ported from NetArchTest ").Append(Pin).Append(' ').Append(source)
            .Append(" by conformance/netarchtest/tools/Port. Do not edit; rerun the tool.\n")
            .Append("source: ").Append(source).Append('\n')
            .Append("architecture: [").Append(Oracle.Structure).Append("]\n")
            .Append("cases:\n");
        files[entry.File] = text;
    }
    var item = new JsonObject { ["id"] = id, ["csharp"] = entry.CSharp };
    if (entry.Search is not null)
    {
        item["search"] = entry.Search;
    }
    if (entry.Architecture is not null)
    {
        item["architecture"] = new JsonArray(entry.Architecture.Select(a => (JsonNode)JsonValue.Create(a)).ToArray());
    }
    if (entry.Note is not null)
    {
        item["note"] = entry.Note;
    }
    item["rule"] = entry.Rule;
    item["expect"] = entry.Expect;
    Yaml.Item(text, 0, item);
}
foreach (var (name, text) in files)
{
    File.WriteAllText(Path.Combine(ported, $"{name.Replace('.', '-')}.yaml"), text.ToString());
}

var json = new JsonSerializerOptions { WriteIndented = true, Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping };
var customPredicate = unported.Count(e => e?["reason"]?.GetValue<string>() == "custom-predicate");
File.WriteAllText(Path.Combine(gate, "unported.json"), new JsonObject
{
    ["$comment"] = "NetArchTest unit tests, or single searches of them, with no case in ported/, each with a reason, written by conformance/netarchtest/tools/Port (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7). `block` is the 1-based entry of `test` (its case or search), or null for a whole test. Reasons: custom-predicate (MeetCustomRule, a C# predicate); api-only: <what> (NetArchTest's own API, with no rule counterpart); constructed-type, external-namespace and vocabulary-gap: <what> (no element-rule key gives NetArchTest's verdict); dependency-definition: <what> (NetArchTest counts as a dependency something ArchUnitNET's definition, which the graph document follows, does not, or the reverse).",
    ["pin"] = Pin,
    ["entries"] = unported,
}.ToJsonString(json) + "\n");
File.WriteAllText(Path.Combine(gate, "ported.json"), new JsonObject
{
    ["$comment"] = "Conformance gate 2, NetArchTest half (docs/adr/0009-conformance-suites-as-specification.md): `ported` may only rise. Written by conformance/netarchtest/tools/Port. `total` counts every case and every unported entry; `ported` counts the cases in ported/*.yaml; `customPredicate` counts the unported.json entries whose reason is custom-predicate.",
    ["pin"] = Pin,
    ["total"] = cases + unported.Count,
    ["ported"] = cases,
    ["customPredicate"] = customPredicate,
}.ToJsonString(json) + "\n");
Console.WriteLine($"port: {cases} cases, {unported.Count} unported ({customPredicate} custom-predicate)");
return 0;
