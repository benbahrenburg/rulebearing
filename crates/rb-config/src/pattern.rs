//! Path patterns: JavaScript regular expressions, run on the linear-time `regex` crate.
//!
//! - Decision: [ADR-0016](../../../docs/adr/0016-linear-time-regex-and-strict-compat.md)
//!   (linear-time engine, the compatibility table, escaped captures, `--strict-compat`)
//! - Plan: [Wave 1, Step 3](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-3-extends-presets-defines-captures-regex-1a)
//! - Requirement: [FR-RULE-10](../../../docs/prd.md#fr-rule-10)
//! - Source: [design § The rule file](../../../docs/artifacts/design.md#the-rule-file) (safe-regex)
//!
//! A dependency-cruiser pattern is a string handed to `new RegExp(pattern)`: JavaScript syntax,
//! no flags, no `u` mode. [`translate`] rewrites it into the `regex` crate's dialect so that it
//! matches the same strings, following [`COMPATIBILITY`], the table the documentation publishes.
//! Lookaround and backreferences have no linear-time equivalent and are refused with
//! [`PatternError::Unsupported`], which the command line reports as exit 3 naming the rule.
//!
//! [`safety`] re-implements safe-regex's two heuristics (star height above one, more repetitions
//! than the limit) so that a pattern dependency-cruiser would refuse is accepted with a warning,
//! or refused under `--strict-compat`.

use std::fmt::Write as _;

use regex::Regex;
use regex_syntax::ast::{self, Ast};

/// One row of the JavaScript-to-Rust compatibility table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatRow {
    /// The JavaScript construct.
    pub javascript: &'static str,
    /// What Rulebearing does with it.
    pub rulebearing: &'static str,
    /// Whether a pattern using it is accepted.
    pub supported: bool,
}

/// The compatibility table ([ADR-0016](../../../docs/adr/0016-linear-time-regex-and-strict-compat.md)).
/// The tests below prove every supported row by matching, and every unsupported row by refusal.
pub const COMPATIBILITY: &[CompatRow] = &[
    CompatRow {
        javascript: r"\d, \D",
        rulebearing: "ASCII digits only, as JavaScript: [0-9]",
        supported: true,
    },
    CompatRow {
        javascript: r"\w, \W",
        rulebearing: "ASCII word characters only, as JavaScript: [0-9A-Za-z_]",
        supported: true,
    },
    CompatRow {
        javascript: r"\s, \S",
        rulebearing: "JavaScript's whitespace set, including U+FEFF",
        supported: true,
    },
    CompatRow {
        javascript: r"\b, \B",
        rulebearing: "ASCII word boundary, as JavaScript",
        supported: true,
    },
    CompatRow {
        javascript: ".",
        rulebearing: "any character except the four JavaScript line terminators",
        supported: true,
    },
    CompatRow {
        javascript: r"[\b]",
        rulebearing: "backspace, U+0008",
        supported: true,
    },
    CompatRow {
        javascript: "[^] and []",
        rulebearing: "any character; no character",
        supported: true,
    },
    CompatRow {
        javascript: r"\/, \- and other escaped punctuation",
        rulebearing: "the literal character",
        supported: true,
    },
    CompatRow {
        javascript: r"\p, \a and other identity escapes of letters (no u flag)",
        rulebearing: "the literal letter, as JavaScript without the u flag",
        supported: true,
    },
    CompatRow {
        javascript: "{ and } that do not form a quantifier",
        rulebearing: "literal braces, as JavaScript's Annex B",
        supported: true,
    },
    CompatRow {
        javascript: "[ and & and ~ inside a character class",
        rulebearing: "literal characters (Rust would read nested classes and set operations)",
        supported: true,
    },
    CompatRow {
        javascript: r"\xHH, \uHHHH, \cX, \0, \t, \n, \v, \f, \r",
        rulebearing: "the same character",
        supported: true,
    },
    CompatRow {
        javascript: "(?<name>...) named groups, (?:...) groups",
        rulebearing: "the same group",
        supported: true,
    },
    CompatRow {
        javascript: "*, +, ?, {n}, {n,}, {n,m} and their lazy forms",
        rulebearing: "the same quantifier",
        supported: true,
    },
    CompatRow {
        javascript: "(?=...), (?!...) lookahead",
        rulebearing: "refused, exit 3: no linear-time equivalent",
        supported: false,
    },
    CompatRow {
        javascript: "(?<=...), (?<!...) lookbehind",
        rulebearing: "refused, exit 3: no linear-time equivalent",
        supported: false,
    },
    CompatRow {
        javascript: r"\1 to \9, \k<name> backreferences",
        rulebearing: "matched by instantiating the referenced group with each substring it can match (ADR-0028); a reference to a missing or enclosing group is refused, exit 3",
        supported: true,
    },
];

/// Why a pattern cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    /// The pattern uses a construct the linear-time engine cannot run.
    #[error(
        "pattern `{pattern}` uses {construct}, which has no linear-time equivalent; rewrite it without lookaround or backreferences"
    )]
    Unsupported {
        /// The pattern as written.
        pattern: String,
        /// The construct, as the compatibility table names it.
        construct: &'static str,
    },
    /// The pattern is not a valid JavaScript regular expression.
    #[error("pattern `{pattern}` is not a valid regular expression: {reason}")]
    Invalid {
        /// The pattern as written.
        pattern: String,
        /// What the parser said.
        reason: String,
    },
}

/// JavaScript's `\s`, as a class body.
const JS_SPACE: &str = r"\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}";
/// JavaScript's `.`: everything but the line terminators.
const JS_DOT: &str = r"[^\n\r\u{2028}\u{2029}]";

/// Rewrites a JavaScript pattern into the `regex` crate's dialect.
///
/// # Errors
/// [`PatternError::Unsupported`] for lookaround and backreferences, and
/// [`PatternError::Invalid`] for text JavaScript itself would reject.
pub fn translate(pattern: &str) -> Result<String, PatternError> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut i = 0;
    let mut in_class = false;
    let mut class_start = false;
    let mut previous_in_class: Option<char> = None;
    let unsupported = |construct: &'static str| PatternError::Unsupported {
        pattern: pattern.to_owned(),
        construct,
    };
    let invalid = |reason: &str| PatternError::Invalid {
        pattern: pattern.to_owned(),
        reason: reason.to_owned(),
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            let Some(&next) = chars.get(i + 1) else {
                return Err(invalid("\\ at end of pattern"));
            };
            i += 2;
            let piece = translate_escape(next, &chars, &mut i, in_class)
                .map_err(|e| e.into_error(pattern))?;
            out.push_str(&piece);
            class_start = false;
            previous_in_class = Some(next);
            continue;
        }
        if in_class {
            match c {
                ']' if class_start => {
                    // `[]` matches nothing and `[^]` anything; Rust has neither spelling.
                    let negated = out.ends_with("[^");
                    out.truncate(out.len() - if negated { 2 } else { 1 });
                    out.push_str(if negated { r"(?s:.)" } else { r"[a&&b]" });
                    in_class = false;
                }
                ']' => {
                    out.push(']');
                    in_class = false;
                }
                '[' | '&' | '~' => {
                    out.push('\\');
                    out.push(c);
                }
                '-' if previous_in_class == Some('-') => out.push_str(r"\-"),
                _ => out.push(c),
            }
            class_start = false;
            previous_in_class = Some(c);
            i += 1;
            continue;
        }
        match c {
            '[' => {
                out.push('[');
                in_class = true;
                class_start = true;
                previous_in_class = None;
                if chars.get(i + 1) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
            }
            '(' if chars.get(i + 1) == Some(&'?') => {
                match (chars.get(i + 2), chars.get(i + 3)) {
                    (Some(':'), _) => out.push_str("(?:"),
                    (Some('='), _) => return Err(unsupported("(?=...) lookahead")),
                    (Some('!'), _) => return Err(unsupported("(?!...) lookahead")),
                    (Some('<'), Some('=')) => return Err(unsupported("(?<=...) lookbehind")),
                    (Some('<'), Some('!')) => return Err(unsupported("(?<!...) lookbehind")),
                    (Some('<'), _) => out.push_str("(?<"),
                    _ => return Err(invalid("invalid group")),
                }
                i += 3;
                continue;
            }
            '.' => out.push_str(JS_DOT),
            '{' => {
                if let Some(end) = quantifier_end(&chars, i) {
                    out.extend(&chars[i..=end]);
                    i = end + 1;
                    continue;
                }
                out.push_str(r"\{");
            }
            '}' | ']' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
        i += 1;
    }
    if in_class {
        return Err(invalid("unterminated character class"));
    }
    Ok(out)
}

/// An unsupported escape; the caller names the whole pattern.
struct EscapeError(&'static str);

impl EscapeError {
    fn into_error(self, pattern: &str) -> PatternError {
        PatternError::Unsupported {
            pattern: pattern.to_owned(),
            construct: self.0,
        }
    }
}

/// Translates the escape `\next`; `i` points past it and advances over any operand.
fn translate_escape(
    next: char,
    chars: &[char],
    i: &mut usize,
    in_class: bool,
) -> Result<String, EscapeError> {
    let class = |body: &str, negated: bool| {
        if negated {
            format!("[^{body}]")
        } else {
            format!("[{body}]")
        }
    };
    Ok(match next {
        'd' => class("0-9", false),
        'D' => class("0-9", true),
        'w' => class("0-9A-Za-z_", false),
        'W' => class("0-9A-Za-z_", true),
        's' => class(JS_SPACE, false),
        'S' => class(JS_SPACE, true),
        'b' if in_class => r"\x08".to_owned(),
        'b' => r"(?-u:\b)".to_owned(),
        'B' => r"(?-u:\B)".to_owned(),
        't' => r"\t".to_owned(),
        'n' => r"\n".to_owned(),
        'v' => r"\x0B".to_owned(),
        'f' => r"\x0C".to_owned(),
        'r' => r"\r".to_owned(),
        '0' if !chars.get(*i).is_some_and(char::is_ascii_digit) => r"\x00".to_owned(),
        '1'..='9' if !in_class => {
            return Err(EscapeError(r"\1 to \9 backreferences"));
        }
        'k' if !in_class && chars.get(*i) == Some(&'<') => {
            return Err(EscapeError(r"\k<name> backreferences"));
        }
        'x' => hex_escape(chars, i, 2).unwrap_or_else(|| "x".to_owned()),
        'u' => hex_escape(chars, i, 4).unwrap_or_else(|| "u".to_owned()),
        'c' => match chars.get(*i) {
            Some(letter) if letter.is_ascii_alphabetic() => {
                *i += 1;
                format!(r"\x{:02X}", (*letter as u32) % 32)
            }
            _ => r"\\c".to_owned(),
        },
        '8' | '9' => next.to_string(),
        c if c.is_ascii_digit() => {
            // A legacy octal escape inside a class, or `\0` followed by a digit.
            let mut value = c.to_digit(8).unwrap_or_default();
            while let Some(d) = chars.get(*i).and_then(|d| d.to_digit(8)) {
                if value * 8 + d > 0o377 {
                    break;
                }
                value = value * 8 + d;
                *i += 1;
            }
            format!(r"\x{{{value:X}}}")
        }
        c if c.is_ascii_alphabetic() => {
            // Without the u flag an escaped letter JavaScript does not define is the letter.
            c.to_string()
        }
        c => escape_literal(c),
    })
}

/// `\xHH` or `\uHHHH`: `digits` hexadecimal digits after `i`, or `None` when absent.
fn hex_escape(chars: &[char], i: &mut usize, digits: usize) -> Option<String> {
    let hex: String = chars.get(*i..*i + digits)?.iter().collect();
    let value = u32::from_str_radix(&hex, 16).ok()?;
    *i += digits;
    Some(format!(r"\x{{{value:X}}}"))
}

/// A literal character, escaped when it means something to the `regex` crate.
fn escape_literal(c: char) -> String {
    if regex_syntax::is_meta_character(c) {
        format!("\\{c}")
    } else if c.is_ascii_punctuation() {
        // Rust accepts an escaped punctuation character, but the literal needs no escape.
        c.to_string()
    } else {
        // A non-ASCII identity escape: the character itself.
        let mut text = String::new();
        let _ = write!(text, "{c}");
        text
    }
}

/// When `{` at `start` opens a JavaScript quantifier `{n}`, `{n,}` or `{n,m}`, the index of its
/// `}`.
fn quantifier_end(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start + 1;
    let digits = |i: &mut usize| {
        let from = *i;
        while chars.get(*i).is_some_and(char::is_ascii_digit) {
            *i += 1;
        }
        *i > from
    };
    if !digits(&mut i) {
        return None;
    }
    if chars.get(i) == Some(&',') {
        i += 1;
        digits(&mut i);
    }
    (chars.get(i) == Some(&'}')).then_some(i)
}

/// Compiles a JavaScript pattern without backreferences to one [`Regex`]; use [`matcher`] for a
/// pattern that may have them.
///
/// # Errors
/// See [`translate`]; a translation the `regex` crate still rejects is [`PatternError::Invalid`].
pub fn compile(pattern: &str) -> Result<Regex, PatternError> {
    compile_plain(pattern)
}

/// A capturing group of a JavaScript pattern, by byte offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Group {
    /// Offset of `(`.
    open: usize,
    /// Offset of the first byte of the group's content.
    content: usize,
    /// Offset of `)`.
    close: usize,
    /// The name of a `(?<name>...)` group.
    name: Option<String>,
}

/// A backreference `\k` or `\k<name>`, by byte offsets, and the 0-based group it names.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Backreference {
    start: usize,
    end: usize,
    group: usize,
}

/// The capturing groups and backreferences of a JavaScript pattern.
fn scan(pattern: &str) -> (Vec<Group>, Vec<(usize, usize, String)>) {
    let bytes = pattern.as_bytes();
    let mut groups: Vec<Group> = Vec::new();
    let mut open: Vec<Option<usize>> = Vec::new();
    let mut references = Vec::new();
    let mut in_class = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                if !in_class {
                    let rest = &pattern[i + 1..];
                    if let Some(digit) = rest.chars().next().filter(|c| ('1'..='9').contains(c)) {
                        references.push((i, i + 2, digit.to_string()));
                    } else if let Some(named) = rest.strip_prefix("k<")
                        && let Some(end) = named.find('>')
                    {
                        references.push((i, i + 4 + end, format!("<{}", &named[..end])));
                    }
                }
                i += 1;
            }
            b'[' if !in_class => in_class = true,
            b']' if in_class => in_class = false,
            b'(' if !in_class => {
                let rest = &pattern[i..];
                let named = rest
                    .strip_prefix("(?<")
                    .filter(|r| !r.starts_with('=') && !r.starts_with('!'))
                    .and_then(|r| r.find('>').map(|end| r[..end].to_owned()));
                let capturing = !rest.starts_with("(?") || named.is_some();
                if capturing {
                    let content = i + named.as_ref().map_or(1, |n| n.len() + 4);
                    groups.push(Group {
                        open: i,
                        content,
                        close: i,
                        name: named,
                    });
                    open.push(Some(groups.len() - 1));
                } else {
                    open.push(None);
                }
            }
            b')' if !in_class => {
                if let Some(Some(index)) = open.pop() {
                    groups[index].close = i;
                }
            }
            _ => {}
        }
        i += 1;
    }
    (groups, references)
}

/// Rewrites every capturing group of `pattern` as a non-capturing one.
fn uncapture(pattern: &str) -> String {
    let (groups, _) = scan(pattern);
    let mut out = pattern.to_owned();
    for group in groups.iter().rev() {
        out.replace_range(group.open..group.content, "(?:");
    }
    out
}

/// A backreference, planned: which groups are referenced and how to instantiate them.
#[derive(Debug)]
struct Backreferences {
    pattern: String,
    groups: Vec<Group>,
    references: Vec<Backreference>,
    /// The referenced groups, each with an anchored matcher for its own content.
    referenced: Vec<(usize, Regex)>,
    /// Instantiated patterns, compiled once each.
    cache: std::sync::Mutex<std::collections::HashMap<Vec<String>, Option<Regex>>>,
}

/// The most candidate assignments a backreference pattern tries on one string.
pub const BACKREFERENCE_CANDIDATE_LIMIT: usize = 4096;

impl Backreferences {
    /// The pattern with each referenced group replaced by a literal, keeping group numbers.
    fn instantiate(&self, values: &[String]) -> String {
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        for ((group, _), value) in self.referenced.iter().zip(values) {
            let g = &self.groups[*group];
            // Inner capturing groups keep their numbers and capture the empty string.
            let inner = self
                .groups
                .iter()
                .filter(|o| o.open > g.open && o.close < g.close)
                .count();
            edits.push((
                g.content,
                g.close,
                format!("{}{}", escape_javascript(value), "()".repeat(inner)),
            ));
        }
        for reference in &self.references {
            if let Some(position) = self
                .referenced
                .iter()
                .position(|(g, _)| *g == reference.group)
            {
                edits.push((
                    reference.start,
                    reference.end,
                    format!("(?:{})", escape_javascript(&values[position])),
                ));
            }
        }
        edits.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
        let mut out = self.pattern.clone();
        for (start, end, text) in edits {
            out.replace_range(start..end, &text);
        }
        out
    }

    /// Every substring of `text` the group's own pattern matches whole.
    fn candidates(matcher: &Regex, text: &str) -> Vec<String> {
        let bounds: Vec<usize> = text
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(text.len()))
            .collect();
        let mut out = Vec::new();
        for (a, &start) in bounds.iter().enumerate() {
            for &end in &bounds[a..] {
                let slice = &text[start..end];
                if matcher.is_match(slice) && !out.iter().any(|o: &String| o == slice) {
                    out.push(slice.to_owned());
                }
            }
        }
        out
    }

    /// The first instantiation that matches `text`, in candidate order.
    fn matching(&self, text: &str) -> Option<Regex> {
        let lists: Vec<Vec<String>> = self
            .referenced
            .iter()
            .map(|(_, m)| Self::candidates(m, text))
            .collect();
        let total = lists.iter().map(Vec::len).product::<usize>();
        if total == 0 || total > BACKREFERENCE_CANDIDATE_LIMIT {
            return None;
        }
        let mut index = vec![0usize; lists.len()];
        loop {
            let values: Vec<String> = index
                .iter()
                .zip(&lists)
                .map(|(i, list)| list[*i].clone())
                .collect();
            let regex = {
                let mut cache = self
                    .cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                cache
                    .entry(values.clone())
                    .or_insert_with(|| compile_plain(&self.instantiate(&values)).ok())
                    .clone()
            };
            if let Some(regex) = regex
                && regex.is_match(text)
            {
                return Some(regex);
            }
            let mut position = 0;
            loop {
                if position == index.len() {
                    return None;
                }
                index[position] += 1;
                if index[position] < lists[position].len() {
                    break;
                }
                index[position] = 0;
                position += 1;
            }
        }
    }
}

/// A compiled pattern. Almost every pattern is one linear-time [`Regex`]; a pattern with
/// backreferences is an over-approximating [`Regex`] plus a verifier that instantiates each
/// referenced group with the substrings it can match
/// ([ADR-0028](../../../docs/adr/0028-backreferences-by-instantiation-on-the-linear-engine.md)).
#[derive(Debug)]
pub struct Matcher {
    regex: Regex,
    backreferences: Option<Backreferences>,
}

impl Matcher {
    /// Whether the pattern matches somewhere in `text`, as `RegExp.prototype.test` does.
    pub fn is_match(&self, text: &str) -> bool {
        if !self.regex.is_match(text) {
            return false;
        }
        self.backreferences
            .as_ref()
            .is_none_or(|b| b.matching(text).is_some())
    }

    /// dependency-cruiser's `extractGroups` for this pattern; see [`extract_groups`].
    pub fn groups(&self, text: &str) -> Vec<String> {
        match &self.backreferences {
            None => extract_groups(&self.regex, text),
            Some(b) => b
                .matching(text)
                .map(|regex| extract_groups(&regex, text))
                .unwrap_or_default(),
        }
    }

    /// The first match, as `RegExp.prototype.exec(text)[0]`.
    pub fn find<'t>(&self, text: &'t str) -> Option<&'t str> {
        match &self.backreferences {
            None => self.regex.find(text).map(|m| m.as_str()),
            Some(b) => b.matching(text)?.find(text).map(|m| m.as_str()),
        }
    }
}

/// Compiles a pattern without backreferences.
fn compile_plain(pattern: &str) -> Result<Regex, PatternError> {
    let translated = translate(pattern)?;
    Regex::new(&translated).map_err(|e| PatternError::Invalid {
        pattern: pattern.to_owned(),
        reason: e.to_string(),
    })
}

/// Compiles a JavaScript pattern into a [`Matcher`].
///
/// # Errors
/// See [`translate`]; a backreference to a group that does not exist, or one inside the group it
/// names, is [`PatternError::Unsupported`].
pub fn matcher(pattern: &str) -> Result<Matcher, PatternError> {
    let (groups, found) = scan(pattern);
    if found.is_empty() {
        return Ok(Matcher {
            regex: compile_plain(pattern)?,
            backreferences: None,
        });
    }
    let unsupported = || PatternError::Unsupported {
        pattern: pattern.to_owned(),
        construct: r"\1 to \9, \k<name> backreferences to a missing or enclosing group",
    };
    let mut references = Vec::new();
    for (start, end, target) in found {
        let group = match target.strip_prefix('<') {
            Some(name) => groups.iter().position(|g| g.name.as_deref() == Some(name)),
            None => target.parse::<usize>().ok().and_then(|n| n.checked_sub(1)),
        }
        .filter(|g| *g < groups.len())
        .ok_or_else(unsupported)?;
        let g = &groups[group];
        if start > g.open && start < g.close {
            return Err(unsupported());
        }
        references.push(Backreference { start, end, group });
    }
    let mut targets: Vec<usize> = references.iter().map(|r| r.group).collect();
    targets.sort_unstable();
    targets.dedup();
    let mut approximate = pattern.to_owned();
    for reference in references.iter().rev() {
        let g = &groups[reference.group];
        let content = uncapture(&pattern[g.content..g.close]);
        approximate.replace_range(reference.start..reference.end, &format!("(?:{content})"));
    }
    let group_matchers = targets
        .into_iter()
        .map(|group| {
            let g = &groups[group];
            compile_plain(&format!(
                "^(?:{})$",
                uncapture(&pattern[g.content..g.close])
            ))
            .map(|m| (group, m))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Matcher {
        regex: compile_plain(&approximate)?,
        backreferences: Some(Backreferences {
            pattern: pattern.to_owned(),
            groups,
            references,
            referenced: group_matchers,
            cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }),
    })
}

/// safe-regex's repetition limit for option patterns (its default).
pub const OPTION_REPETITION_LIMIT: usize = 25;
/// dependency-cruiser raises the limit to 10,000 for rule patterns.
pub const RULE_REPETITION_LIMIT: usize = 10_000;

/// What safe-regex would say about a pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Safety {
    /// safe-regex accepts it.
    Safe,
    /// A quantifier is nested inside another (star height above one).
    NestedQuantifier,
    /// More quantifiers than the limit.
    TooManyRepetitions,
}

/// Measures a pattern the way safe-regex's heuristic analyser does. A pattern that does not
/// parse is reported by [`compile`], not here, and counts as safe.
pub fn safety(pattern: &str, limit: usize) -> Safety {
    let Ok(translated) = translate(pattern) else {
        return Safety::Safe;
    };
    let Ok(ast) = ast::parse::Parser::new().parse(&translated) else {
        return Safety::Safe;
    };
    let (height, count) = measure(&ast, 0);
    if height > 1 {
        Safety::NestedQuantifier
    } else if count > limit {
        Safety::TooManyRepetitions
    } else {
        Safety::Safe
    }
}

/// The deepest nesting of repetitions and the number of repetitions under `ast`.
fn measure(ast: &Ast, depth: usize) -> (usize, usize) {
    match ast {
        Ast::Repetition(repetition) => {
            let (height, count) = measure(&repetition.ast, depth + 1);
            (height.max(depth + 1), count + 1)
        }
        Ast::Group(group) => measure(&group.ast, depth),
        Ast::Alternation(alternation) => fold(&alternation.asts, depth),
        Ast::Concat(concat) => fold(&concat.asts, depth),
        _ => (depth, 0),
    }
}

fn fold(asts: &[Ast], depth: usize) -> (usize, usize) {
    asts.iter().fold((depth, 0), |(height, count), ast| {
        let (h, c) = measure(ast, depth);
        (height.max(h), count + c)
    })
}

/// dependency-cruiser's `extractGroups`: the match and every participating group, or nothing
/// when the pattern has no groups or does not match. Groups that did not participate are
/// dropped, which shifts the later indices, exactly as the JavaScript `filter` does.
pub fn extract_groups(regex: &Regex, text: &str) -> Vec<String> {
    if regex.captures_len() <= 1 {
        return Vec::new();
    }
    regex.captures(text).map_or_else(Vec::new, |captures| {
        captures
            .iter()
            .flatten()
            .map(|m| m.as_str().to_owned())
            .collect()
    })
}

/// Escapes text for use as a literal inside a JavaScript pattern.
pub fn escape_javascript(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(
            c,
            '\\' | '^'
                | '$'
                | '.'
                | '*'
                | '+'
                | '?'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '|'
                | '/'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// dependency-cruiser's `replaceGroupPlaceholders`, with each group escaped
/// ([ADR-0016](../../../docs/adr/0016-linear-time-regex-and-strict-compat.md)): `$0` becomes the
/// whole match, `$1` the first participating group, and so on, in index order.
pub fn replace_group_placeholders(pattern: &str, groups: &[String]) -> String {
    let mut result = pattern.to_owned();
    for (index, group) in groups.iter().enumerate() {
        let placeholder = format!("${index}");
        if result.contains(&placeholder) {
            result = result.replace(&placeholder, &escape_javascript(group));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn matches(pattern: &str, text: &str) -> bool {
        compile(pattern).is_ok_and(|r| r.is_match(text))
    }

    #[test]
    fn digit_word_and_space_are_ascii_as_in_javascript() {
        assert!(matches(r"^\d+$", "123"));
        assert!(
            !matches(r"^\d$", "٣"),
            "Arabic-Indic digit is not \\d in JavaScript"
        );
        assert!(matches(r"^\D$", "٣"));
        assert!(matches(r"^\w+$", "a_Z9"));
        assert!(!matches(r"^\w$", "é"));
        assert!(matches(r"^\W$", "é"));
        assert!(matches(r"^\s$", "\u{FEFF}"));
        assert!(matches(r"^\S$", "a"));
        assert!(matches(r"a\bb|a\b", "a b"));
        assert!(matches(r"\Bb", "ab"));
        assert!(matches(r"[\d]", "5"));
        assert!(matches(r"[^\D]", "5"));
    }

    #[test]
    fn dot_excludes_the_javascript_line_terminators() {
        assert!(matches("^a.b$", "axb"));
        assert!(!matches("^a.b$", "a\nb"));
        assert!(!matches("^a.b$", "a\u{2028}b"));
        assert!(matches("^a[.]b$", "a.b"));
    }

    #[test]
    fn escaped_punctuation_and_letters_are_literals() {
        assert!(matches(r"^src\/a\-b$", "src/a-b"));
        assert!(matches(r"^\p$", "p"), "no u flag: \\p is p");
        assert!(matches(r"^\a\e$", "ae"));
        assert!(matches(r"^\.$", "."));
        assert!(!matches(r"^\.$", "x"));
        assert!(matches(r"^\@$", "@"));
        assert!(matches("^é\\é$", "éé"));
    }

    #[test]
    fn braces_that_are_not_quantifiers_are_literal() {
        assert!(matches("^a{2}$", "aa"));
        assert!(matches("^a{1,}$", "aaa"));
        assert!(matches("^a{1,2}$", "aa"));
        assert!(matches("^{foo}$", "{foo}"));
        assert!(matches("^a{$", "a{"));
        assert!(matches("^a{,2}$", "a{,2}"));
        assert!(matches("^x]$", "x]"));
    }

    #[test]
    fn classes_keep_javascript_meaning() {
        assert!(matches("^[[]$", "["));
        assert!(matches("^[a&&b]$", "&"));
        assert!(matches("^[~~]$", "~"));
        assert!(matches(r"^[a\-]$", "-"));
        assert!(matches(r"^[\b]$", "\u{8}"));
        assert!(matches("^[^]$", "\n"));
        assert!(!matches("^[]$", ""));
        assert!(!matches("a[]", "a"));
        assert!(
            !matches("^[]]$", "]"),
            "[] matches nothing, then a literal ]"
        );
        assert!(matches(r"^[\8]$", "8"));
        assert!(matches("^[^a]$", "b"));
        assert!(!matches("^[^a]$", "a"));
    }

    #[test]
    fn character_escapes() {
        assert!(matches(r"^\x41\u0042$", "AB"));
        assert!(matches(r"^\t\n\v\f\r$", "\t\n\u{B}\u{C}\r"));
        assert!(matches(r"^\0$", "\0"));
        assert!(matches(r"^\cJ$", "\n"));
        assert!(matches(r"^\xZZ$", "xZZ"));
        assert!(matches(r"^\uZZ$", "uZZ"));
        assert!(matches(r"^[\101]$", "A"));
        assert!(matches(r"^\c1$", "\\c1"));
    }

    #[test]
    fn groups_and_quantifiers() {
        assert!(matches("^(?:ab)+?$", "abab"));
        assert!(matches("^(?<name>x)$", "x"));
        assert!(matches("^a*b+c?$", "aabbb"));
    }

    #[test]
    fn lookaround_and_backreferences_are_refused() {
        for (pattern, construct) in [
            ("a(?=b)", "(?=...) lookahead"),
            ("a(?!b)", "(?!...) lookahead"),
            ("(?<=a)b", "(?<=...) lookbehind"),
            ("(?<!a)b", "(?<!...) lookbehind"),
            (r"(a)\1", r"\1 to \9 backreferences"),
            (r"(?<n>a)\k<n>", r"\k<name> backreferences"),
        ] {
            assert_eq!(
                translate(pattern),
                Err(PatternError::Unsupported {
                    pattern: pattern.to_owned(),
                    construct
                }),
                "{pattern}"
            );
        }
        let message = compile("a(?=b)")
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(message.contains("linear-time"), "{message}");
    }

    #[test]
    fn invalid_patterns_are_named() {
        for pattern in ["a\\", "[a", "(?i)a", "(a"] {
            assert!(
                matches!(compile(pattern), Err(PatternError::Invalid { .. })),
                "{pattern}"
            );
        }
    }

    #[test]
    fn every_table_row_is_either_supported_or_refused() {
        let supported = COMPATIBILITY.iter().filter(|r| r.supported).count();
        let refused = COMPATIBILITY.iter().filter(|r| !r.supported).count();
        assert_eq!((supported, refused), (15, 2));
    }

    #[test]
    fn safety_is_safe_regex_s_heuristic() {
        assert_eq!(safety("^src/", RULE_REPETITION_LIMIT), Safety::Safe);
        assert_eq!(
            safety("(a+)+", RULE_REPETITION_LIMIT),
            Safety::NestedQuantifier
        );
        assert_eq!(
            safety("(a?)*", RULE_REPETITION_LIMIT),
            Safety::NestedQuantifier
        );
        assert_eq!(safety("(ab|c*)", RULE_REPETITION_LIMIT), Safety::Safe);
        let many = "a*".repeat(26);
        assert_eq!(
            safety(&many, OPTION_REPETITION_LIMIT),
            Safety::TooManyRepetitions
        );
        assert_eq!(safety(&many, RULE_REPETITION_LIMIT), Safety::Safe);
        assert_eq!(safety("a(?=b)", 1), Safety::Safe, "refused elsewhere");
    }

    #[test]
    fn groups_are_extracted_as_javascript_filters_them() -> Result<(), PatternError> {
        let regex = compile("^(a)?(b)/")?;
        assert_eq!(extract_groups(&regex, "b/c"), ["b/", "b"]);
        assert_eq!(extract_groups(&regex, "ab/c"), ["ab/", "a", "b"]);
        assert!(extract_groups(&regex, "zzz").is_empty());
        let no_groups = compile("^a")?;
        assert!(extract_groups(&no_groups, "a").is_empty());
        Ok(())
    }

    #[test]
    fn placeholders_are_substituted_escaped() {
        let groups = vec!["src/x/".to_owned(), "x".to_owned()];
        assert_eq!(
            replace_group_placeholders("^src/$1/|^test/$1/", &groups),
            "^src/x/|^test/x/"
        );
        let wild = vec![".*".to_owned(), ".*".to_owned()];
        let replaced = replace_group_placeholders("^apps/$1/", &wild);
        assert_eq!(replaced, r"^apps/\.\*/");
        assert!(
            !matches(&replaced, "apps/web/"),
            "a capture never becomes a wildcard"
        );
        assert!(matches(&replaced, "apps/.*/"));
        assert_eq!(replace_group_placeholders("$0", &groups), r"src\/x\/");
    }

    #[test]
    fn backreferences_match_by_instantiation() -> Result<(), PatternError> {
        // The langfuse oracle's pathNot: a component folder's own file or its index.
        let langfuse = matcher(r"^$1/|(^|/)([A-Z][A-Za-z0-9]*)/(\2|index)\.(ts|tsx)$")?;
        assert!(langfuse.is_match("src/features/Button/Button.tsx"));
        assert!(langfuse.is_match("Button/index.ts"));
        assert!(!langfuse.is_match("src/features/Button/Other.tsx"));
        assert!(!langfuse.is_match("src/features/button/button.tsx"));
        let named = matcher(r"^(?<a>[a-z]+)-\k<a>$")?;
        assert!(named.is_match("ab-ab"));
        assert!(!named.is_match("ab-cd"));
        let twice = matcher(r"^(x+)(y+)\2\1$")?;
        assert!(twice.is_match("xxyyyyxx"));
        assert!(!twice.is_match("xxyyyx"));
        assert_eq!(twice.find("xyyx"), Some("xyyx"));
        assert_eq!(twice.groups("xyyx"), ["xyyx", "x", "y"]);
        assert!(twice.groups("abc").is_empty());
        let nested = matcher(r"^((a)b)\1$")?;
        assert!(nested.is_match("abab"));
        assert!(!nested.is_match("abba"));
        for bad in [r"(a)\2", r"(a\1)", r"\k<nope>(a)"] {
            assert!(
                matches!(matcher(bad), Err(PatternError::Unsupported { .. })),
                "{bad}"
            );
        }
        let plain = matcher("^src/(.+)$")?;
        assert_eq!(plain.groups("src/a"), ["src/a", "a"]);
        assert_eq!(plain.find("x/src/a"), None);
        assert!(plain.is_match("src/a"));
        Ok(())
    }

    #[test]
    fn scanning_finds_groups_and_references() {
        let (groups, references) = scan(r"(a)(?:b)(?<n>c)[(\1]\1\k<n>");
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[1].name.as_deref(), Some("n"));
        assert_eq!(references.len(), 2);
        assert_eq!(uncapture("(a(?<n>b))(?:c)"), "(?:a(?:b))(?:c)");
    }

    proptest! {
        #[test]
        fn escaped_text_matches_itself_only(text in "\\PC{0,16}") {
            let escaped = format!("^{}$", escape_javascript(&text));
            let regex = compile(&escaped);
            prop_assert!(regex.is_ok(), "{escaped}");
            if let Ok(regex) = regex {
                prop_assert!(regex.is_match(&text));
            }
        }

        #[test]
        fn translation_never_panics(pattern in "\\PC{0,24}") {
            let _ = compile(&pattern);
            let _ = safety(&pattern, OPTION_REPETITION_LIMIT);
        }

        #[test]
        fn literal_paths_match_themselves(path in "[a-z0-9_/.-]{1,24}") {
            let escaped = escape_javascript(&path);
            prop_assert!(matches(&escaped, &path));
        }
    }
}
