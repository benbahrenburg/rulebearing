// JSON values printed as crates/rb-report/src/lib.rs prints them (`text`, `js_number`), which is
// JavaScript's `${value}` for the values the result carries.

using System.Text.Json;
using Xunit;

namespace Rulebearing.TestAdapter.Tests;

/// <summary>The value printing the messages use.</summary>
public sealed class JsValueTests
{
    /// <summary>Each kind of value, as <c>js_number</c> prints it.</summary>
    /// <param name="json">The property's JSON.</param>
    /// <param name="expected">The printed text.</param>
    [Theory]
    [InlineData("\"text\"", "text")]
    [InlineData("3", "3")]
    [InlineData("3.0", "3")]
    [InlineData("-0.0", "-0")]
    [InlineData("12345678901234567890", "12345678901234567168")]
    [InlineData("2.5", "2.5")]
    [InlineData("1e21", "1e21")]
    [InlineData("1.5e-7", "1.5e-7")]
    [InlineData("true", "true")]
    [InlineData("false", "false")]
    [InlineData("null", "null")]
    [InlineData("{\"b\":1,\"a\":[1,\"é\"]}", "{\"b\":1,\"a\":[1,\"é\"]}")]
    [InlineData("1e400", "1e400")]
    public void PrintsAsTheReportersDo(string json, string expected)
    {
        using JsonDocument document = JsonDocument.Parse($"{{\"v\":{json}}}");
        Assert.Equal(expected, JsValue.Text(document.RootElement, "v"));
    }

    /// <summary>A missing property, or a property of a non-object, is <c>undefined</c>.</summary>
    [Fact]
    public void AMissingValueIsUndefined()
    {
        using JsonDocument document = JsonDocument.Parse("[1]");
        Assert.Equal("undefined", JsValue.Text(document.RootElement, "v"));
        Assert.Equal("undefined", JsValue.Print(null));
        Assert.Null(JsValue.UInt(document.RootElement, "v"));
        Assert.Empty(JsValue.Array(document.RootElement, "v"));
    }

    /// <summary>Only a non-negative integer is a line or column.</summary>
    /// <param name="json">The property's JSON.</param>
    /// <param name="expected">The integer, or null.</param>
    [Theory]
    [InlineData("4", 4UL)]
    [InlineData("-4", null)]
    [InlineData("4.5", null)]
    [InlineData("\"4\"", null)]
    public void UnsignedIntegersOnly(string json, ulong? expected)
    {
        using JsonDocument document = JsonDocument.Parse($"{{\"v\":{json}}}");
        Assert.Equal(expected, JsValue.UInt(document.RootElement, "v"));
    }

    /// <summary>Exponents are written the way serde_json writes them.</summary>
    /// <param name="value">The number.</param>
    /// <param name="expected">The text.</param>
    [Theory]
    [InlineData(1.5e300, "1.5e300")]
    [InlineData(2e-10, "2e-10")]
    [InlineData(0.25, "0.25")]
    public void ShortestRoundTripDigits(double value, string expected) =>
        Assert.Equal(expected, JsValue.Shortest(value));
}
