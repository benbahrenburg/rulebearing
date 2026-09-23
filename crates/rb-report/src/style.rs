//! Terminal styling and text layout shared by the reporters: Node's `util.styleText`,
//! dependency-cruiser's `wrapAndIndent`, and `Intl.NumberFormat` percentages.
//!
//! - Specification: dependency-cruiser 18.2.0 `src/utl/wrap-and-indent.mjs`,
//!   `src/report/utl/index.mjs`; byte-compared by conformance gate 1 layer 3
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Plan: [Wave 1, Step 12](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-12-reporters-1d)
//!
//! Colour is off unless the caller asks for it, as dependency-cruiser's is when stdout is not a
//! terminal or `NO_COLOR` is set; the escape codes are the ones `styleText` writes.

/// A text style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Red.
    Red,
    /// Green.
    Green,
    /// Yellow.
    Yellow,
    /// Cyan.
    Cyan,
    /// Gray.
    Gray,
    /// Bold.
    Bold,
    /// Dim.
    Dim,
    /// Underline.
    Underline,
}

impl Style {
    const fn codes(self) -> (u8, u8) {
        match self {
            Self::Red => (31, 39),
            Self::Green => (32, 39),
            Self::Yellow => (33, 39),
            Self::Cyan => (36, 39),
            Self::Gray => (90, 39),
            Self::Bold => (1, 22),
            Self::Dim => (2, 22),
            Self::Underline => (4, 24),
        }
    }

    /// The colour for a severity, as the `err` reporter maps it.
    pub fn for_severity(severity: &str) -> Option<Self> {
        match severity {
            "error" => Some(Self::Red),
            "warn" => Some(Self::Yellow),
            "info" => Some(Self::Cyan),
            "ignore" => Some(Self::Gray),
            _ => None,
        }
    }
}

/// `styleText(style, text)`, or the text alone when colour is off.
pub fn styled(style: Option<Style>, text: &str, color: bool) -> String {
    match style {
        Some(style) if color => {
            let (open, close) = style.codes();
            format!("\u{1b}[{open}m{text}\u{1b}[{close}m")
        }
        _ => text.to_owned(),
    }
}

/// JavaScript's `\s`.
fn js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// The length JavaScript reports: UTF-16 code units.
fn js_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn split_line(line: &str, max_width: usize) -> String {
    let mut wrapped: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut width = 0;
    for word in line.split(' ') {
        if width + js_len(word) > max_width {
            wrapped.push(current.trim_end().to_owned());
            current.clear();
            width = 0;
        }
        if !current.is_empty() {
            current.push(' ');
            width += 1;
        }
        current.push_str(word);
        width += js_len(word);
    }
    wrapped.push(current.trim_end().to_owned());
    wrapped.join("\n")
}

/// dependency-cruiser's `wrapAndIndent(text, indent)`: wrapped to 78 columns less the indent,
/// then every line with anything but whitespace indented.
pub fn wrap_and_indent(text: &str, indent: usize) -> String {
    let max_width = 78usize.saturating_sub(indent);
    let wrapped: Vec<String> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .map(|line| split_line(line, max_width))
        .collect();
    let joined = wrapped.join("\n");
    let pad = " ".repeat(indent);
    joined
        .split('\n')
        .map(|line| {
            if line.chars().all(js_space) {
                line.to_owned()
            } else {
                format!("{pad}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `new Intl.NumberFormat(undefined, { style: "percent" }).format(value)` in the `en-US` locale:
/// the exact decimal value of the double, times 100, rounded half away from zero.
pub fn percentage(value: f64) -> String {
    if !value.is_finite() {
        return if value.is_nan() {
            "NaN%".into()
        } else if value > 0.0 {
            "∞%".into()
        } else {
            "-∞%".into()
        };
    }
    let negative = value < 0.0;
    // Enough digits to hold the double's exact decimal expansion for the fraction we round on.
    let text = format!("{:.40}", value.abs());
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let fraction = format!("{fraction:0<3}");
    let mut percent: u128 = format!("{whole}{}", &fraction[..2]).parse().unwrap_or(0);
    let rest = &fraction[2..];
    if rest.as_bytes().first().is_some_and(|d| *d >= b'5') {
        percent += 1;
    }
    let digits = percent.to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let sign = if negative && percent != 0 { "-" } else { "" };
    format!("{sign}{grouped}%")
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn styles_are_node_s_escape_codes() {
        assert_eq!(styled(Some(Style::Red), "x", true), "\u{1b}[31mx\u{1b}[39m");
        assert_eq!(styled(Some(Style::Bold), "x", true), "\u{1b}[1mx\u{1b}[22m");
        assert_eq!(
            styled(Some(Style::Underline), "x", true),
            "\u{1b}[4mx\u{1b}[24m"
        );
        assert_eq!(styled(Some(Style::Red), "x", false), "x");
        assert_eq!(styled(None, "x", true), "x");
        assert_eq!(Style::for_severity("warn"), Some(Style::Yellow));
        assert_eq!(Style::for_severity("info"), Some(Style::Cyan));
        assert_eq!(Style::for_severity("ignore"), Some(Style::Gray));
        assert_eq!(Style::for_severity("loud"), None);
        for (s, open) in [
            (Style::Green, 32),
            (Style::Yellow, 33),
            (Style::Cyan, 36),
            (Style::Gray, 90),
            (Style::Dim, 2),
        ] {
            assert!(styled(Some(s), "", true).starts_with(&format!("\u{1b}[{open}m")));
        }
    }

    #[test]
    fn wrapping_matches_upstream() {
        assert_eq!(wrap_and_indent("a b", 4), "    a b");
        assert_eq!(wrap_and_indent("a\n\nb", 2), "  a\n\n  b");
        let long = "word ".repeat(20);
        let wrapped = wrap_and_indent(long.trim_end(), 4);
        assert!(wrapped.lines().all(|l| l.len() <= 79), "{wrapped}");
        assert_eq!(wrapped.lines().count(), 2);
        assert_eq!(wrap_and_indent("a\r\nb", 1), " a\n b");
        assert_eq!(
            wrap_and_indent("   ", 2),
            "",
            "upstream trims each wrapped line's end"
        );
        assert_eq!(js_len("é😀"), 3);
    }

    #[test]
    fn percentages_round_like_intl() {
        assert_eq!(percentage(0.923_076_923_076_923_1), "92%");
        assert_eq!(percentage(0.5), "50%");
        assert_eq!(percentage(0.005), "1%");
        assert_eq!(
            percentage(0.285),
            "28%",
            "0.285 is just below one half in binary"
        );
        assert_eq!(percentage(1.0), "100%");
        assert_eq!(percentage(0.0), "0%");
        assert_eq!(percentage(12.345), "1,235%");
        assert_eq!(percentage(-0.5), "-50%");
        assert_eq!(percentage(-0.001), "0%");
        assert_eq!(percentage(f64::NAN), "NaN%");
        assert_eq!(percentage(f64::INFINITY), "∞%");
        assert_eq!(percentage(f64::NEG_INFINITY), "-∞%");
    }

    proptest! {
        #[test]
        fn percentages_are_whole_numbers(value in 0.0f64..1.0) {
            let p = percentage(value);
            prop_assert!(p.ends_with('%'));
            let n: u32 = p.trim_end_matches('%').parse().unwrap_or(1000);
            prop_assert!(n <= 100);
        }

        #[test]
        fn wrapped_lines_fit(words in proptest::collection::vec("[a-z]{1,10}", 0..40), indent in 0usize..10) {
            let text = words.join(" ");
            // Upstream checks the width before it adds the joining space, so a line can run one
            // column past 78.
            for line in wrap_and_indent(&text, indent).lines() {
                prop_assert!(line.len() <= 79);
            }
        }
    }
}
