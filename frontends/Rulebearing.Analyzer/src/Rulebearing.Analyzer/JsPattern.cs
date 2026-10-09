// Path and name patterns: JavaScript regular expressions, matched as Rulebearing matches them.
//
// Decision: docs/adr/0016-linear-time-regex-and-strict-compat.md (the compatibility table, and
// lookaround refused). Plan: docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md,
// Step 21 (PathMatcher, held to the table rb-config exports as regex-compatibility.json).
//
// A rule's pattern is the string dependency-cruiser hands to `new RegExp(pattern)`: no flags, no
// `u` mode, tested anywhere in the subject. rb-config translates it for Rust's `regex` crate; this
// translates it for .NET's engine with the same meaning: \d, \w and \b are ASCII, \s is
// JavaScript's whitespace set, `.` stops only at the four JavaScript line terminators, `$` is the
// end of input, an escaped letter that is no escape is the letter, braces that form no quantifier
// are literal, and inside a class `[` is a plain character. Lookaround is refused, as Rulebearing
// refuses it, so the analyzer never evaluates a pattern the gate would reject.

using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;

namespace Rulebearing.Analyzer;

/// <summary>A pattern that cannot be used, with the reason.</summary>
internal sealed class PatternException : Exception
{
    /// <summary>A refusal of <paramref name="pattern"/> for <paramref name="reason"/>.</summary>
    public PatternException(string pattern, string reason)
        : base($"the pattern `{pattern}` {reason}")
    {
    }
}

/// <summary>A JavaScript regular expression, translated for .NET.</summary>
internal sealed class JsPattern
{
    private const string Word = "0-9A-Za-z_";
    private const string NotWordRanges = "\\x00-/:-@\\[-\\^`{-\\uFFFF";
    private const string Digit = "0-9";
    private const string NotDigitRanges = "\\x00-/:-\\uFFFF";

    // JavaScript's WhiteSpace and LineTerminator code points.
    private const string Space = "\\t\\n\\v\\f\\r \\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000\\uFEFF";
    private const string NotSpaceRanges = "\\x00-\\x08\\x0E-\\x1F!-\\u009F\\u00A1-\\u167F\\u1681-\\u1FFF\\u200B-\\u2027\\u202A-\\u202E\\u2030-\\u205E\\u2060-\\u2FFF\\u3001-\\uFEFE\\uFF00-\\uFFFF";

    private readonly Regex _regex;

    private JsPattern(string source, Regex regex)
    {
        Source = source;
        _regex = regex;
    }

    /// <summary>The pattern as written.</summary>
    public string Source { get; }

    /// <summary>Translates and compiles <paramref name="pattern"/>.</summary>
    /// <exception cref="PatternException">The pattern uses lookaround or is malformed.</exception>
    public static JsPattern Compile(string pattern)
    {
        var translated = Translate(pattern);
        try
        {
            return new JsPattern(pattern, new Regex(translated, RegexOptions.CultureInvariant, TimeSpan.FromSeconds(1)));
        }
        catch (ArgumentException e)
        {
            throw new PatternException(pattern, "is not a valid regular expression: " + e.Message);
        }
    }

    /// <summary>Whether the pattern matches somewhere in <paramref name="text"/>, as <c>RegExp.prototype.test</c>.</summary>
    public bool IsMatch(string text) => _regex.IsMatch(text);

    /// <summary>The whole match and each participating group, as dependency-cruiser's extractGroups filters them; empty when nothing matches.</summary>
    public string[] Groups(string text)
    {
        var match = _regex.Match(text);
        if (!match.Success)
        {
            return [];
        }
        var groups = new System.Collections.Generic.List<string>();
        for (var i = 0; i < match.Groups.Count; i++)
        {
            if (match.Groups[i].Success)
            {
                groups.Add(match.Groups[i].Value);
            }
        }
        return [.. groups];
    }

    /// <summary>The .NET pattern with the same meaning as the JavaScript <paramref name="pattern"/>.</summary>
    /// <exception cref="PatternException">The pattern uses lookaround or is malformed.</exception>
    public static string Translate(string pattern)
    {
        var output = new StringBuilder();
        var depth = 0;
        var groups = CountGroups(pattern);
        var i = 0;
        while (i < pattern.Length)
        {
            var c = pattern[i];
            switch (c)
            {
                case '\\':
                    i = Escape(pattern, i, output, groups);
                    continue;
                case '[':
                    i = Class(pattern, i, output);
                    continue;
                case '(':
                    i = Group(pattern, i, output);
                    depth++;
                    continue;
                case ')':
                    if (depth == 0)
                    {
                        throw new PatternException(pattern, "is not a valid regular expression: unmatched )");
                    }
                    depth--;
                    output.Append(')');
                    break;
                case '.':
                    output.Append("[^\\n\\r\\u2028\\u2029]");
                    break;
                case '$':
                    output.Append("\\z");
                    break;
                case '{':
                case '}':
                    if (c == '{' && QuantifierLength(pattern, i) is int length)
                    {
                        output.Append(pattern, i, length);
                        i += length;
                        continue;
                    }
                    output.Append('\\').Append(c);
                    break;
                default:
                    output.Append(c);
                    break;
            }
            i++;
        }
        if (depth != 0)
        {
            throw new PatternException(pattern, "is not a valid regular expression: unmatched (");
        }
        return output.ToString();
    }

    private static int CountGroups(string pattern)
    {
        var count = 0;
        var inClass = false;
        for (var i = 0; i < pattern.Length; i++)
        {
            var c = pattern[i];
            if (c == '\\')
            {
                i++;
            }
            else if (c == '[')
            {
                inClass = true;
            }
            else if (c == ']')
            {
                inClass = false;
            }
            else if (!inClass && c == '(' && (i + 1 >= pattern.Length || pattern[i + 1] != '?' || (i + 2 < pattern.Length && pattern[i + 2] == '<' && i + 3 < pattern.Length && pattern[i + 3] != '=' && pattern[i + 3] != '!')))
            {
                count++;
            }
        }
        return count;
    }

    /// <summary>The length of a quantifier <c>{n}</c>, <c>{n,}</c> or <c>{n,m}</c> starting at <paramref name="at"/>, or null when the braces are literal.</summary>
    private static int? QuantifierLength(string pattern, int at)
    {
        var match = Regex.Match(pattern.Substring(at), "^\\{[0-9]+(,[0-9]*)?\\}");
        return match.Success ? match.Length : null;
    }

    private static int Group(string pattern, int at, StringBuilder output)
    {
        if (at + 1 < pattern.Length && pattern[at + 1] == '?')
        {
            var rest = pattern.Substring(at + 2);
            if (rest.StartsWith("=", StringComparison.Ordinal) || rest.StartsWith("!", StringComparison.Ordinal))
            {
                throw new PatternException(pattern, "uses lookahead, which has no linear-time equivalent");
            }
            if (rest.StartsWith("<=", StringComparison.Ordinal) || rest.StartsWith("<!", StringComparison.Ordinal))
            {
                throw new PatternException(pattern, "uses lookbehind, which has no linear-time equivalent");
            }
            if (rest.StartsWith(":", StringComparison.Ordinal))
            {
                output.Append("(?:");
                return at + 3;
            }
            var named = Regex.Match(rest, "^<([A-Za-z_$][A-Za-z0-9_$]*)>");
            if (named.Success)
            {
                output.Append("(?<").Append(named.Groups[1].Value).Append('>');
                return at + 2 + named.Length;
            }
            throw new PatternException(pattern, "is not a valid regular expression: (? is not a group");
        }
        output.Append('(');
        return at + 1;
    }

    private static int Escape(string pattern, int at, StringBuilder output, int groups)
    {
        if (at + 1 >= pattern.Length)
        {
            throw new PatternException(pattern, "is not a valid regular expression: it ends with \\");
        }
        var next = pattern[at + 1];
        switch (next)
        {
            case 'd':
                output.Append('[').Append(Digit).Append(']');
                return at + 2;
            case 'D':
                output.Append("[^").Append(Digit).Append(']');
                return at + 2;
            case 'w':
                output.Append('[').Append(Word).Append(']');
                return at + 2;
            case 'W':
                output.Append("[^").Append(Word).Append(']');
                return at + 2;
            case 's':
                output.Append('[').Append(Space).Append(']');
                return at + 2;
            case 'S':
                output.Append("[^").Append(Space).Append(']');
                return at + 2;
            case 'b':
                output.Append("(?:(?<=[").Append(Word).Append("])(?![").Append(Word).Append("])|(?<![").Append(Word).Append("])(?=[").Append(Word).Append("]))");
                return at + 2;
            case 'B':
                output.Append("(?:(?<=[").Append(Word).Append("])(?=[").Append(Word).Append("])|(?<![").Append(Word).Append("])(?![").Append(Word).Append("]))");
                return at + 2;
            case 'k':
                var named = Regex.Match(pattern.Substring(at + 2), "^<([A-Za-z_$][A-Za-z0-9_$]*)>");
                if (named.Success)
                {
                    output.Append("\\k<").Append(named.Groups[1].Value).Append('>');
                    return at + 2 + named.Length;
                }
                output.Append('k');
                return at + 2;
            default:
                break;
        }
        if (next is >= '1' and <= '9')
        {
            var digits = Regex.Match(pattern.Substring(at + 1), "^[0-9]+").Value;
            var number = int.Parse(digits, CultureInfo.InvariantCulture);
            if (number <= groups)
            {
                output.Append('\\').Append(digits);
                return at + 1 + digits.Length;
            }
            throw new PatternException(pattern, $"refers to group {number}, which does not exist");
        }
        return Character(pattern, at, output, inClass: false);
    }

    /// <summary>One escaped character, outside or inside a class: appends its .NET form and returns the index after it.</summary>
    private static int Character(string pattern, int at, StringBuilder output, bool inClass)
    {
        var next = pattern[at + 1];
        switch (next)
        {
            case 't':
                output.Append("\\t");
                return at + 2;
            case 'n':
                output.Append("\\n");
                return at + 2;
            case 'v':
                output.Append("\\v");
                return at + 2;
            case 'f':
                output.Append("\\f");
                return at + 2;
            case 'r':
                output.Append("\\r");
                return at + 2;
            case '0' when !inClass && (at + 2 >= pattern.Length || !char.IsDigit(pattern[at + 2])):
                output.Append("\\x00");
                return at + 2;
            case 'x':
                var hex = Regex.Match(pattern.Substring(at + 2), "^[0-9A-Fa-f]{2}");
                if (hex.Success)
                {
                    output.Append("\\x").Append(hex.Value);
                    return at + 4;
                }
                output.Append('x');
                return at + 2;
            case 'u':
                var unicode = Regex.Match(pattern.Substring(at + 2), "^[0-9A-Fa-f]{4}");
                if (unicode.Success)
                {
                    output.Append("\\u").Append(unicode.Value);
                    return at + 6;
                }
                output.Append('u');
                return at + 2;
            case 'c':
                if (at + 2 < pattern.Length && char.IsLetter(pattern[at + 2]) && pattern[at + 2] < 128)
                {
                    var control = (char)(char.ToUpperInvariant(pattern[at + 2]) % 32);
                    output.Append("\\x").Append(((int)control).ToString("X2", CultureInfo.InvariantCulture));
                    return at + 3;
                }
                // Annex B: `\c` before anything but a letter is a backslash and a `c`.
                output.Append("\\\\c");
                return at + 2;
            default:
                break;
        }
        if (inClass && next is >= '0' and <= '7')
        {
            // Annex B: a legacy octal escape inside a class.
            var octal = Regex.Match(pattern.Substring(at + 1), "^[0-7]{1,3}").Value;
            var value = Convert.ToInt32(octal, 8);
            output.Append("\\x").Append(value.ToString("X2", CultureInfo.InvariantCulture));
            return at + 1 + octal.Length;
        }
        // Any other escape is its character: escaped punctuation, and letters that are no escape
        // without the `u` flag.
        output.Append(Literal(next, inClass));
        return at + 2;
    }

    private static string Literal(char c, bool inClass)
    {
        if (char.IsLetterOrDigit(c) || c == '_')
        {
            return c.ToString();
        }
        if (inClass)
        {
            return "\\" + c;
        }
        return Regex.Escape(c.ToString());
    }

    private static int Class(string pattern, int at, StringBuilder output)
    {
        var i = at + 1;
        var negated = i < pattern.Length && pattern[i] == '^';
        if (negated)
        {
            i++;
        }
        if (i < pattern.Length && pattern[i] == ']')
        {
            // `[]` matches nothing and `[^]` any character.
            output.Append(negated ? "[\\s\\S]" : "(?!)");
            return i + 1;
        }
        var body = new StringBuilder();
        while (i < pattern.Length && pattern[i] != ']')
        {
            var c = pattern[i];
            if (c == '\\')
            {
                if (i + 1 >= pattern.Length)
                {
                    throw new PatternException(pattern, "is not a valid regular expression: it ends with \\");
                }
                var next = pattern[i + 1];
                switch (next)
                {
                    case 'd':
                        body.Append(Digit);
                        i += 2;
                        continue;
                    case 'D':
                        body.Append(NotDigitRanges);
                        i += 2;
                        continue;
                    case 'w':
                        body.Append(Word);
                        i += 2;
                        continue;
                    case 'W':
                        body.Append(NotWordRanges);
                        i += 2;
                        continue;
                    case 's':
                        body.Append(Space);
                        i += 2;
                        continue;
                    case 'S':
                        body.Append(NotSpaceRanges);
                        i += 2;
                        continue;
                    case 'b':
                        body.Append("\\x08");
                        i += 2;
                        continue;
                    case '-':
                        body.Append("\\-");
                        i += 2;
                        continue;
                    default:
                        i = Character(pattern, i, body, inClass: true);
                        continue;
                }
            }
            body.Append(c is '[' or '^' && body.Length > 0 || c == '[' ? "\\" + c : c.ToString());
            i++;
        }
        if (i >= pattern.Length)
        {
            throw new PatternException(pattern, "is not a valid regular expression: unmatched [");
        }
        output.Append(negated ? "[^" : "[").Append(body).Append(']');
        return i + 1;
    }
}
