// How the reporters print a JSON value: the `text`, `js_number` and `severity` helpers of
// crates/rb-report/src/lib.rs, which follow JavaScript's `${value}`. Ported so the adapter's
// failure message is the junit reporter's, byte for byte
// (docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md, section 1.5).

using System.Globalization;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace Rulebearing.TestAdapter;

/// <summary>JSON values printed as the Rust reporters print them.</summary>
internal static class JsValue
{
    private static readonly JsonSerializerOptions Compact = new()
    {
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        WriteIndented = false,
    };

    /// <summary><c>value.get(key)</c>: the property of an object, or nothing.</summary>
    public static JsonElement? Get(JsonElement? value, string key) =>
        value is { ValueKind: JsonValueKind.Object } v && v.TryGetProperty(key, out JsonElement found)
            ? found
            : null;

    /// <summary>A property that is a string, else nothing.</summary>
    public static string? String(JsonElement? value, string key) =>
        Get(value, key) is { ValueKind: JsonValueKind.String } s ? s.GetString() : null;

    /// <summary>A property that is an array, else an empty list.</summary>
    public static IReadOnlyList<JsonElement> Array(JsonElement? value, string key) =>
        Get(value, key) is { ValueKind: JsonValueKind.Array } a ? [.. a.EnumerateArray()] : [];

    /// <summary>A property that is a non-negative integer, as serde_json's <c>as_u64</c> reads it.</summary>
    public static ulong? UInt(JsonElement? value, string key) =>
        Get(value, key) is { ValueKind: JsonValueKind.Number } n && n.TryGetUInt64(out ulong u) ? u : null;

    /// <summary><c>text</c>: a string as it is, any other value as <c>${value}</c>, a missing one as <c>undefined</c>.</summary>
    public static string Text(JsonElement? value, string key) => Get(value, key) switch
    {
        { ValueKind: JsonValueKind.String } s => s.GetString() ?? string.Empty,
        { } other => Print(other),
        null => "undefined",
    };

    /// <summary><c>js_number</c>: a value as <c>${value}</c> prints it.</summary>
    public static string Print(JsonElement? value) => value switch
    {
        null => "undefined",
        { ValueKind: JsonValueKind.Null } => "null",
        { ValueKind: JsonValueKind.String } s => s.GetString() ?? string.Empty,
        { ValueKind: JsonValueKind.Number } n => Number(n),
        { ValueKind: JsonValueKind.True } => "true",
        { ValueKind: JsonValueKind.False } => "false",
        { } other => JsonSerializer.Serialize(other, Compact),
    };

    /// <summary>A violation's severity: its rule's, <c>undefined</c> when the rule has none.</summary>
    public static string Severity(JsonElement violation) =>
        Get(violation, "rule") is { } rule ? Text(rule, "severity") : string.Empty;

    private static string Number(JsonElement number)
    {
        if (!number.TryGetDouble(out double f) || !double.IsFinite(f))
        {
            return number.GetRawText();
        }

        return Math.Abs(f % 1) == 0 && Math.Abs(f) < 1e21
            ? f.ToString("F0", CultureInfo.InvariantCulture)
            : Shortest(f);
    }

    /// <summary>
    /// serde_json's float printing (the shortest round-trip digits; an exponent written
    /// <c>e</c>, <c>e-7</c>, never <c>E+07</c>).
    /// </summary>
    internal static string Shortest(double f)
    {
        string text = f.ToString("R", CultureInfo.InvariantCulture);
        int e = text.IndexOf('E', StringComparison.Ordinal);
        if (e < 0)
        {
            return text;
        }

        string mantissa = text[..e];
        string exponent = text[(e + 1)..];
        bool negative = exponent.StartsWith('-');
        string digits = exponent.TrimStart('+', '-').TrimStart('0');
        return $"{mantissa}e{(negative ? "-" : string.Empty)}{(digits.Length == 0 ? "0" : digits)}";
    }
}
