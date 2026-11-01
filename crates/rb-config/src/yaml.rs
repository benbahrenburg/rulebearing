//! YAML in and out: one set of parser options for every YAML text the tool reads, and the block
//! writer `config convert` and `config expand` print with.
//!
//! - Architecture: [`docs/architecture.md#configuration-and-the-rule-language`](../../../docs/architecture.md#configuration-and-the-rule-language)
//! - Decisions: [ADR-0079](../../../docs/adr/0079-yaml-is-parsed-with-serde-saphyr.md) (the parser and
//!   these options), [ADR-0025](../../../docs/adr/0025-ci-and-supply-chain-hardening.md) (the
//!   advisory gate that retired `serde_yaml`), [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md)
//! - Plan: [Wave 1, sub-wave 1A](../../../docs/plans/implemented/0001-wave-1-typescript-parity.md#wave-1a-rb-config)
//! - Requirements: [FR-CFG-01](../../../docs/prd.md#fr-cfg-01), [NFR-SEC-01](../../../docs/prd.md#nfr-sec-01)
//!
//! The options keep what the configurations, presets and fixtures already meant under the parser
//! this replaced: YAML 1.2 booleans (`yes` and `no` stay strings), `<<` an ordinary key, the last of
//! two equal keys wins, an unknown tag is an error, and `.inf` and `.nan` reach `serde_json` as
//! `null`. Errors are one line, without a source snippet, because the callers wrap them in their
//! own named reason. The parser's default budget bounds every document, so an alias bomb or a deep
//! nest is an error, never a hang or a panic.

use serde::de::Deserialize;
use serde_json::{Map, Value};
use serde_saphyr::{DuplicateKeyPolicy, MergeKeyPolicy, NonFiniteFloatPolicy, Options};

/// The YAML parser's error.
pub use serde_saphyr::Error;

/// The parser options every YAML read uses ([ADR-0079](../../../docs/adr/0079-yaml-is-parsed-with-serde-saphyr.md)).
#[must_use]
pub fn options() -> Options {
    let mut options = Options::default();
    options.strict_booleans = true;
    options.merge_keys = MergeKeyPolicy::AsOrdinary;
    options.duplicate_keys = DuplicateKeyPolicy::LastWins;
    options.reject_unsupported_tags = true;
    options.non_finite_float_policy = NonFiniteFloatPolicy::PassThrough;
    options.with_snippet = false;
    options
}

/// Parses one YAML document from text.
///
/// # Errors
///
/// [`Error`] when the text is not one YAML document of the shape `T` asks for, or exceeds the
/// parser's budget.
pub fn from_str<'de, T: Deserialize<'de>>(text: &'de str) -> Result<T, Error> {
    serde_saphyr::from_str_with_options(text, options())
}

/// Parses one YAML document from bytes.
///
/// # Errors
///
/// [`Error`] as [`from_str`], and when the bytes are not text.
pub fn from_slice<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, Error> {
    serde_saphyr::from_slice_with_options(bytes, options())
}

/// Writes a mapping as one YAML document, in block style, keys in the map's order.
///
/// The layout is the one `config convert` and `config expand` have always written: a sequence's
/// `- ` at its key's indentation, `{}` and `[]` for empty collections, a string with a newline as
/// a literal block (`|-`, `|` or `|+` by its trailing newlines), and every other scalar plain when
/// it reads back as the same value, else single-quoted, else double-quoted with escapes. Whether a
/// scalar may be plain is the serialiser's own judgement under YAML 1.2, so the text always reads
/// back equal ([ADR-0079](../../../docs/adr/0079-yaml-is-parsed-with-serde-saphyr.md)).
///
/// # Errors
///
/// [`serde_saphyr::SerializeError`] when a scalar cannot be written as YAML.
pub fn to_string(map: &Map<String, Value>) -> Result<String, serde_saphyr::SerializeError> {
    let mut out = String::new();
    if map.is_empty() {
        out.push_str("{}\n");
    } else {
        write_map(&mut out, map, 0)?;
    }
    Ok(out)
}

type Written = Result<(), serde_saphyr::SerializeError>;

fn write_map(out: &mut String, map: &Map<String, Value>, indent: usize) -> Written {
    for (key, value) in map {
        push_indent(out, indent);
        out.push_str(&scalar(&Value::String(key.clone()))?);
        out.push(':');
        write_value(out, value, indent, indent + 2)?;
    }
    Ok(())
}

fn write_seq(out: &mut String, items: &[Value], indent: usize) -> Written {
    for item in items {
        push_indent(out, indent);
        out.push('-');
        write_value(out, item, indent + 2, indent + 2)?;
    }
    Ok(())
}

/// Writes what follows a `key:` or a `-`: a scalar on the same line, or a block below it. A
/// sequence under a key sits at the key's indentation (`seq_indent`); everything else nests
/// at `indent`. Under a `-` both are the item's content column, and the first line of a nested
/// block moves up onto the `-` line.
fn write_value(out: &mut String, value: &Value, seq_indent: usize, indent: usize) -> Written {
    let under_item = out.ends_with('-');
    match value {
        Value::Object(map) if !map.is_empty() => {
            let mut block = String::new();
            write_map(&mut block, map, indent)?;
            attach(out, &block, indent, under_item);
        }
        Value::Array(items) if !items.is_empty() => {
            let mut block = String::new();
            write_seq(&mut block, items, seq_indent)?;
            attach(out, &block, seq_indent, under_item);
        }
        Value::String(text) if text.contains('\n') && literal_allowed(text) => {
            out.push_str(" |");
            out.push_str(chomping(text));
            out.push('\n');
            let body = text.strip_suffix('\n').unwrap_or(text);
            for line in body.split('\n') {
                if !line.is_empty() {
                    push_indent(out, indent);
                    out.push_str(line);
                }
                out.push('\n');
            }
        }
        _ => {
            out.push(' ');
            out.push_str(&scalar(value)?);
            out.push('\n');
        }
    }
    Ok(())
}

/// Appends a nested block: below the key, or with its first line on the `-` line.
fn attach(out: &mut String, block: &str, indent: usize, under_item: bool) {
    if under_item {
        out.push(' ');
        out.push_str(&block[indent..]);
    } else {
        out.push('\n');
        out.push_str(block);
    }
}

fn push_indent(out: &mut String, indent: usize) {
    out.extend(std::iter::repeat_n(' ', indent));
}

/// The chomping indicator that keeps the string's trailing newlines.
fn chomping(text: &str) -> &'static str {
    if text.ends_with("\n\n") {
        "+"
    } else if text.ends_with('\n') {
        ""
    } else {
        "-"
    }
}

/// A literal block keeps the text only when no line would change on reading back: no control
/// character but the newline, no line ending in a space or a tab, at least one line with text,
/// and no leading space on the first such line (which would need an indentation indicator).
fn literal_allowed(text: &str) -> bool {
    text.split('\n')
        .find(|line| !line.is_empty())
        .is_some_and(|first| !first.starts_with(' '))
        && !text.chars().any(|c| c.is_control() && c != '\n')
        && !text.split('\n').any(|line| line.ends_with(' '))
}

/// One scalar on one line: plain when the serialiser writes it plain, else single-quoted when
/// that needs no escape, else the serialiser's double-quoted form.
fn scalar(value: &Value) -> Result<String, serde_saphyr::SerializeError> {
    let mut options = serde_saphyr::SerializerOptions::default();
    options.yaml_12 = true;
    options.no_lang_directive = true;
    options.prefer_block_scalars = false;
    let written = serde_saphyr::to_string_with_options(value, options)?;
    let written = written.strip_suffix('\n').unwrap_or(&written).to_owned();
    match value {
        Value::String(text) if written.starts_with('"') && !text.chars().any(char::is_control) => {
            Ok(format!("'{}'", text.replace('\'', "''")))
        }
        _ => Ok(written),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::json;

    fn parse(text: &str) -> Result<Value, Error> {
        from_str(text)
    }

    #[test]
    fn yaml_1_1_booleans_stay_strings() -> Result<(), Error> {
        let value = parse("a: yes\nb: no\nc: on\nd: off\ne: y\nf: true\ng: False")?;
        assert_eq!(
            value,
            json!({"a": "yes", "b": "no", "c": "on", "d": "off", "e": "y", "f": true, "g": false})
        );
        Ok(())
    }

    #[test]
    fn merge_key_is_an_ordinary_key() -> Result<(), Error> {
        let value = parse("base: &b {x: 1}\nderived:\n  <<: *b\n  y: 2")?;
        assert_eq!(
            value,
            json!({"base": {"x": 1}, "derived": {"<<": {"x": 1}, "y": 2}})
        );
        Ok(())
    }

    #[test]
    fn the_last_of_two_equal_keys_wins() -> Result<(), Error> {
        assert_eq!(parse("a: 1\na: 2")?, json!({"a": 2}));
        Ok(())
    }

    #[test]
    fn an_unknown_tag_is_an_error() {
        assert!(parse("a: !foo bar").is_err());
    }

    #[test]
    fn non_finite_floats_become_null() -> Result<(), Error> {
        assert_eq!(
            parse("a: .inf\nb: -.inf\nc: .nan")?,
            json!({"a": null, "b": null, "c": null})
        );
        Ok(())
    }

    #[test]
    fn errors_are_one_line_with_the_position() {
        let message = parse("a: [1, 2\n").map_or_else(|e| e.to_string(), |_| String::new());
        assert!(!message.contains('\n'), "{message}");
        assert!(message.contains("line 1"), "{message}");
    }

    #[test]
    fn more_than_one_document_is_an_error() {
        assert!(parse("a: 1\n---\nb: 2").is_err());
    }

    #[test]
    fn a_recursive_alias_is_an_error_not_a_hang() {
        assert!(parse("big: &a [*a]").is_err());
    }

    #[test]
    fn an_alias_bomb_is_bounded() {
        // Eleven levels of ten aliases each: 10^11 nodes if expanded.
        let mut text = String::from("a0: &a0 [x, x, x, x, x, x, x, x, x, x]\n");
        for level in 1..12 {
            let prev = level - 1;
            let refs = vec![format!("*a{prev}"); 10].join(", ");
            text.extend([format!("a{level}: &a{level} [{refs}]\n")]);
        }
        assert!(parse(&text).is_err());
    }

    #[test]
    fn bytes_parse_as_text_does() -> Result<(), Error> {
        let value: Value = from_slice(b"a: [1, two]")?;
        assert_eq!(value, json!({"a": [1, "two"]}));
        Ok(())
    }

    #[test]
    fn written_yaml_keeps_the_layout_the_fixtures_hold() -> Result<(), serde_saphyr::SerializeError>
    {
        // The layout serde_yaml 0.9 wrote, which the committed `convert` and `expand` fixtures
        // hold. The keys are already in order, so the bytes do not depend on `preserve_order`.
        let value = json!({
            "a": [true, "yes", "1.0", "null", "", 3, 1.5, null],
            "b": {},
            "c": "multi\nline",
            "d": [{"e": 1, "f": [], "g": {"h": "x"}}, ["i", "j"], []],
            "k": "it's: a \"quote\" # here",
            "l": "a\n",
            "m": "a\n\nb\n\n",
            "n": " lead",
            "o": "\ttab",
            "p": "*alias",
            "q": "trailing \nspace",
            "r": "~",
            "s": "A module on a cycle cannot be changed, tested or moved on its own. plan:0005-guard-catalogue",
        });
        let Value::Object(map) = value else {
            unreachable!("a json! object is an object")
        };
        assert_eq!(
            to_string(&map)?,
            concat!(
                "a:\n- true\n- yes\n- '1.0'\n- 'null'\n- ''\n- 3\n- 1.5\n- null\n",
                "b: {}\n",
                "c: |-\n  multi\n  line\n",
                "d:\n- e: 1\n  f: []\n  g:\n    h: x\n- - i\n  - j\n- []\n",
                "k: 'it''s: a \"quote\" # here'\n",
                "l: |\n  a\n",
                "m: |+\n  a\n\n  b\n\n",
                "n: ' lead'\n",
                "o: \"\\ttab\"\n",
                "p: '*alias'\n",
                "q: \"trailing \\nspace\"\n",
                "r: '~'\n",
                "s: A module on a cycle cannot be changed, tested or moved on its own. plan:0005-guard-catalogue\n",
            )
        );
        assert_eq!(to_string(&Map::new())?, "{}\n");
        Ok(())
    }

    fn scalar() -> impl Strategy<Value = Value> {
        prop_oneof![
            any::<bool>().prop_map(Value::Bool),
            any::<i64>().prop_map(|n| json!(n)),
            ".{0,12}".prop_map(Value::String),
            "[a-z '\"#:\n-]{0,12}".prop_map(Value::String),
            Just(Value::Null),
        ]
    }

    fn document() -> impl Strategy<Value = Value> {
        scalar().prop_recursive(3, 24, 4, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
                prop::collection::btree_map("[a-z]{1,6}", inner, 0..4)
                    .prop_map(|m| Value::Object(m.into_iter().collect())),
            ]
        })
    }

    proptest! {
        #[test]
        fn what_is_written_reads_back(
            entries in prop::collection::btree_map("[a-z]{1,6}", document(), 0..4)
        ) {
            let map: Map<String, Value> = entries.into_iter().collect();
            let text = to_string(&map).map_err(|e| TestCaseError::fail(e.to_string()))?;
            let back: Value =
                from_str(&text).map_err(|e| TestCaseError::fail(format!("{e}\n{text}")))?;
            prop_assert_eq!(back, Value::Object(map), "{}", text);
        }

        #[test]
        fn arbitrary_text_never_panics(text in "\\PC{0,64}") {
            let _ = parse(&text);
        }
    }
}
