// A small YAML writer for the ported case files: block mappings, flow lists of scalars, and every
// string that is not a plain identifier written as a JSON (and so YAML) double-quoted scalar.
// Plan: docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, Step 7.
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;

namespace Rulebearing.Conformance.NetArchTest;

/// <summary>Writes JSON nodes as block YAML, deterministically.</summary>
internal static partial class Yaml
{
    private static readonly JsonSerializerOptions Quoting = new() { Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping };

    private static readonly string[] Reserved = ["true", "false", "null", "yes", "no", "on", "off", "y", "n", "~"];

    [GeneratedRegex(@"^[A-Za-z_][A-Za-z0-9_.+`#-]*$")]
    private static partial Regex PlainText();

    /// <summary>A scalar as YAML: plain when it is an identifier, double-quoted otherwise.</summary>
    internal static string Scalar(JsonNode? node)
    {
        if (node is null)
        {
            return "null";
        }
        if (node.GetValueKind() == JsonValueKind.True)
        {
            return "true";
        }
        if (node.GetValueKind() == JsonValueKind.False)
        {
            return "false";
        }
        if (node.GetValueKind() == JsonValueKind.Number)
        {
            return node.ToJsonString();
        }
        var text = node.GetValue<string>();
        return PlainText().IsMatch(text) && !Reserved.Contains(text, StringComparer.OrdinalIgnoreCase)
            ? text
            : JsonSerializer.Serialize(text, Quoting);
    }

    private static bool IsScalar(JsonNode? node) => node is null or JsonValue;

    private static string Flow(JsonArray list) => "[" + string.Join(", ", list.Select(Scalar)) + "]";

    /// <summary>Appends <paramref name="node"/> as the value of <paramref name="key"/> at <paramref name="indent"/>.</summary>
    internal static void Field(StringBuilder output, int indent, string key, JsonNode? node)
    {
        var pad = new string(' ', indent);
        switch (node)
        {
            case JsonObject map when map.Count == 0:
                output.Append(pad).Append(key).Append(": {}\n");
                break;
            case JsonObject map:
                output.Append(pad).Append(key).Append(":\n");
                Mapping(output, indent + 2, map);
                break;
            case JsonArray list when list.All(IsScalar) && Flow(list).Length + indent + key.Length <= 100:
                output.Append(pad).Append(key).Append(": ").Append(Flow(list)).Append('\n');
                break;
            case JsonArray list:
                output.Append(pad).Append(key).Append(":\n");
                foreach (var item in list)
                {
                    Item(output, indent + 2, item);
                }
                break;
            default:
                output.Append(pad).Append(key).Append(": ").Append(Scalar(node)).Append('\n');
                break;
        }
    }

    /// <summary>Appends every field of <paramref name="map"/> at <paramref name="indent"/>.</summary>
    internal static void Mapping(StringBuilder output, int indent, JsonObject map)
    {
        foreach (var (key, value) in map)
        {
            Field(output, indent, key, value);
        }
    }

    /// <summary>Appends one list item at <paramref name="indent"/>.</summary>
    internal static void Item(StringBuilder output, int indent, JsonNode? node)
    {
        var pad = new string(' ', indent);
        if (node is JsonObject map && map.Count > 0)
        {
            var inner = new StringBuilder();
            Mapping(inner, indent + 2, map);
            // The first field goes on the dash's line.
            output.Append(pad).Append("- ").Append(inner.ToString()[(indent + 2)..]);
        }
        else if (node is JsonArray list && list.All(IsScalar))
        {
            output.Append(pad).Append("- [").Append(string.Join(", ", list.Select(Scalar))).Append("]\n");
        }
        else if (IsScalar(node))
        {
            output.Append(pad).Append("- ").Append(Scalar(node)).Append('\n');
        }
        else
        {
            throw new NotSupportedException("a list nested directly in a list");
        }
    }
}
