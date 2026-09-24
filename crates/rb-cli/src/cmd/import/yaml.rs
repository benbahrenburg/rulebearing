//! The importers' output: an ordered YAML tree that carries comments, written deterministically.
//!
//! - Plan: [Wave 2, Step 11](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f)
//!   (every emitted rule is preceded by the rule it came from as a `#` comment)
//! - Requirement: [FR-CLI-04](../../../../../docs/prd.md#fr-cli-04)
//! - Decision: [ADR-0005](../../../../../docs/adr/0005-native-config-superset-and-compat.md) (the native format)
//!
//! `serde_yaml` writes no comments and `serde_json`'s map order depends on a crate feature, so the
//! importers build a [`Node`] tree, whose maps keep insertion order, and [`render`] writes it. A
//! list item can carry comment lines above it, and can be written commented out, so a rule the
//! importer will not enable still shows what it would have been. [`Node::to_json`] gives the same
//! tree to the configuration loader, which is how every importer proves its output loads.

use serde_json::{Map, Value};

/// One YAML value.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// A string.
    Str(String),
    /// A boolean.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A sequence of items, each with its own comments.
    List(Vec<Item>),
    /// A mapping in insertion order.
    Map(Vec<(String, Node)>),
}

/// One sequence item: comment lines above it, the value, and whether it is written commented out.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// Lines written as `# line` above the item.
    pub comments: Vec<String>,
    /// The value.
    pub node: Node,
    /// Written with every line commented out, so the loader never sees it.
    pub disabled: bool,
}

impl Item {
    /// An item with no comments.
    pub fn plain(node: Node) -> Self {
        Self {
            comments: Vec::new(),
            node,
            disabled: false,
        }
    }
}

impl Node {
    /// A string node.
    pub fn str(text: impl Into<String>) -> Self {
        Self::Str(text.into())
    }

    /// A mapping from pairs.
    pub fn map(pairs: Vec<(&str, Self)>) -> Self {
        Self::Map(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }

    /// A list of plain items.
    pub fn list(items: Vec<Self>) -> Self {
        Self::List(items.into_iter().map(Item::plain).collect())
    }

    /// A list of strings.
    pub fn strs<S: AsRef<str>>(items: &[S]) -> Self {
        Self::list(items.iter().map(|s| Self::str(s.as_ref())).collect())
    }

    /// The node as JSON, leaving out disabled items.
    pub fn to_json(&self) -> Value {
        match self {
            Self::Str(s) => Value::String(s.clone()),
            Self::Bool(b) => Value::Bool(*b),
            Self::Int(n) => Value::from(*n),
            Self::List(items) => Value::Array(
                items
                    .iter()
                    .filter(|i| !i.disabled)
                    .map(|i| i.node.to_json())
                    .collect(),
            ),
            Self::Map(pairs) => Value::Object(
                pairs
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_json()))
                    .collect::<Map<String, Value>>(),
            ),
        }
    }

    /// A node from JSON; object keys keep `serde_json`'s order.
    pub fn from_json(value: &Value) -> Self {
        match value {
            Value::String(s) => Self::Str(s.clone()),
            Value::Bool(b) => Self::Bool(*b),
            Value::Number(n) => n
                .as_i64()
                .map_or_else(|| Self::Str(n.to_string()), Self::Int),
            Value::Null => Self::Str(String::new()),
            Value::Array(items) => Self::list(items.iter().map(Self::from_json).collect()),
            Value::Object(map) => Self::Map(
                map.iter()
                    .map(|(k, v)| (k.clone(), Self::from_json(v)))
                    .collect(),
            ),
        }
    }

    fn is_scalar(&self) -> bool {
        matches!(self, Self::Str(_) | Self::Bool(_) | Self::Int(_))
    }
}

/// Words YAML 1.1 or 1.2 reads as something other than a string.
const RESERVED: &[&str] = &[
    "true", "false", "yes", "no", "on", "off", "null", "y", "n", "~",
];

/// A scalar as YAML: plain when that reads back as the same string, JSON-quoted otherwise.
pub fn scalar(text: &str) -> String {
    let plain = !text.is_empty()
        && text.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-' | '+' | '@' | '$' | ':')
        })
        && !text.contains(": ")
        && !text.ends_with(':')
        && !text.starts_with(['-', '.', '@', '+'])
        && !text.starts_with(|c: char| c.is_ascii_digit())
        && !RESERVED.contains(&text.to_ascii_lowercase().as_str());
    if plain {
        text.to_owned()
    } else {
        serde_json::to_string(text).unwrap_or_default()
    }
}

fn key(text: &str) -> String {
    let plain = text
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '$' || c == '_')
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '-'));
    if plain {
        text.to_owned()
    } else {
        serde_json::to_string(text).unwrap_or_default()
    }
}

fn scalar_text(node: &Node) -> String {
    match node {
        Node::Str(s) => scalar(s),
        Node::Bool(b) => b.to_string(),
        Node::Int(n) => n.to_string(),
        Node::List(items) if items.is_empty() => "[]".to_owned(),
        Node::Map(pairs) if pairs.is_empty() => "{}".to_owned(),
        Node::List(_) | Node::Map(_) => String::new(),
    }
}

/// A list short enough, and plain enough, to write on one line.
fn flow(items: &[Item]) -> Option<String> {
    if items
        .iter()
        .any(|i| !i.node.is_scalar() || i.disabled || !i.comments.is_empty())
    {
        return None;
    }
    let text = format!(
        "[{}]",
        items
            .iter()
            .map(|i| scalar_text(&i.node))
            .collect::<Vec<_>>()
            .join(", ")
    );
    (text.len() <= 72).then_some(text)
}

/// Writes `node` as the value of a mapping key at `indent`, `out` holding `key:` already.
fn value_after_key(node: &Node, indent: usize, out: &mut Vec<String>) {
    let last = out.len() - 1;
    match node {
        Node::List(items) if !items.is_empty() => {
            if let Some(text) = flow(items) {
                out[last].push(' ');
                out[last].push_str(&text);
            } else {
                // A list whose every item is commented out is still a list to the loader.
                if items.iter().all(|i| i.disabled) {
                    out[last].push_str(" []");
                }
                list(items, indent + 2, out);
            }
        }
        Node::Map(pairs) if !pairs.is_empty() => map(pairs, indent + 2, out),
        scalar_node => {
            out[last].push(' ');
            out[last].push_str(&scalar_text(scalar_node));
        }
    }
}

fn map(pairs: &[(String, Node)], indent: usize, out: &mut Vec<String>) {
    for (k, v) in pairs {
        out.push(format!("{}{}:", " ".repeat(indent), key(k)));
        value_after_key(v, indent, out);
    }
}

fn list(items: &[Item], indent: usize, out: &mut Vec<String>) {
    let pad = " ".repeat(indent);
    for item in items {
        for comment in &item.comments {
            out.push(comment_line(&pad, comment));
        }
        let mut lines = Vec::new();
        match &item.node {
            Node::Map(pairs) if !pairs.is_empty() => {
                map(pairs, indent + 2, &mut lines);
                if let Some(first) = lines.first_mut() {
                    first.replace_range(indent..indent + 2, "- ");
                }
            }
            Node::List(inner) if !inner.is_empty() => {
                lines.push(format!("{pad}-"));
                list(inner, indent + 2, &mut lines);
            }
            other => lines.push(format!("{pad}- {}", scalar_text(other))),
        }
        if item.disabled {
            for line in &mut lines {
                let body = line.split_off(indent.min(line.len()));
                line.push_str("# ");
                line.push_str(&body);
            }
        }
        out.extend(lines);
    }
}

fn comment_line(pad: &str, text: &str) -> String {
    if text.is_empty() {
        format!("{pad}#")
    } else {
        format!("{pad}# {text}")
    }
}

/// A document: comment lines at the top, then a mapping.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    /// Lines written as `# line` before the mapping.
    pub header: Vec<String>,
    /// The top-level mapping.
    pub body: Vec<(String, Node)>,
    /// Comment lines written above a top-level key, by key.
    pub key_comments: Vec<(String, Vec<String>)>,
}

impl Document {
    /// The body as JSON, disabled items left out.
    pub fn to_json(&self) -> Value {
        Node::Map(self.body.clone()).to_json()
    }
}

/// Renders a document. The output ends with one newline.
pub fn render(document: &Document) -> String {
    let mut out: Vec<String> = document
        .header
        .iter()
        .map(|line| comment_line("", line))
        .collect();
    for (k, v) in &document.body {
        if let Some((_, comments)) = document.key_comments.iter().find(|(name, _)| name == k) {
            out.extend(comments.iter().map(|c| comment_line("", c)));
        }
        out.push(format!("{}:", key(k)));
        value_after_key(v, 0, &mut out);
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn scalars_are_plain_only_when_they_read_back() {
        for (text, expected) in [
            ("abc", "abc"),
            ("Ns.Type", "Ns.Type"),
            ("true", "\"true\""),
            ("No", "\"No\""),
            ("12", "\"12\""),
            ("1e3", "\"1e3\""),
            ("", "\"\""),
            ("a b", "\"a b\""),
            ("^src/(a|b)", "\"^src/(a|b)\""),
            ("-x", "\"-x\""),
            ("a: b", "\"a: b\""),
            ("#x", "\"#x\""),
        ] {
            assert_eq!(scalar(text), expected, "{text}");
        }
    }

    proptest! {
        #[test]
        fn every_scalar_reads_back_as_itself(text in "\\PC{0,24}") {
            let yaml = format!("k: {}\n", scalar(&text));
            let back: Value = serde_yaml::from_str(&yaml).map_err(|e| TestCaseError::fail(e.to_string()))?;
            prop_assert_eq!(back["k"].as_str(), Some(text.as_str()));
        }
    }

    #[test]
    fn a_document_renders_in_order_with_comments() {
        let rule = Node::map(vec![
            ("name", Node::str("a")),
            ("select", Node::map(vec![("kind", Node::str("class"))])),
            ("tags", Node::strs(&["x", "yy"])),
            ("empty", Node::List(Vec::new())),
            ("flag", Node::Bool(true)),
            ("n", Node::Int(1)),
        ]);
        let document = Document {
            header: vec!["head".into(), String::new()],
            body: vec![
                ("$schema".into(), Node::str("https://x/y.json")),
                (
                    "rules".into(),
                    Node::map(vec![(
                        "elements",
                        Node::List(vec![
                            Item {
                                comments: vec!["Classes()".into()],
                                node: rule.clone(),
                                disabled: false,
                            },
                            Item {
                                comments: vec!["reason".into()],
                                node: rule,
                                disabled: true,
                            },
                        ]),
                    )]),
                ),
            ],
            key_comments: vec![("rules".into(), vec!["the rules".into()])],
        };
        let text = render(&document);
        let expected = "# head\n#\n$schema: https://x/y.json\n# the rules\nrules:\n  elements:\n    # Classes()\n    - name: a\n      select:\n        kind: class\n      tags: [x, yy]\n      empty: []\n      flag: true\n      n: 1\n    # reason\n    # - name: a\n    #   select:\n    #     kind: class\n    #   tags: [x, yy]\n    #   empty: []\n    #   flag: true\n    #   n: 1\n";
        assert_eq!(text, expected);
        let back: Value = serde_yaml::from_str(&text).unwrap_or_default();
        assert_eq!(back, document.to_json());
        assert_eq!(
            back["rules"]["elements"].as_array().map(Vec::len),
            Some(1),
            "the disabled item is a comment"
        );
    }

    #[test]
    fn a_list_of_only_commented_items_is_an_empty_list() {
        let document = Document {
            body: vec![(
                "elements".into(),
                Node::List(vec![Item {
                    comments: vec!["why".into()],
                    node: Node::map(vec![("name", Node::str("x"))]),
                    disabled: true,
                }]),
            )],
            ..Document::default()
        };
        let text = render(&document);
        assert_eq!(text, "elements: []\n  # why\n  # - name: x\n");
        let back: Value = serde_yaml::from_str(&text).unwrap_or_default();
        assert_eq!(back["elements"], serde_json::json!([]));
    }

    #[test]
    fn nested_lists_and_long_lists_are_block_style() {
        let long: Vec<String> = (0..20).map(|i| format!("item-number-{i}")).collect();
        let node = Node::map(vec![
            ("long", Node::strs(&long)),
            (
                "nested",
                Node::list(vec![
                    Node::strs(&["a"]),
                    Node::map(vec![("k", Node::Int(2))]),
                ]),
            ),
        ]);
        let document = Document {
            body: vec![("x".into(), node)],
            ..Document::default()
        };
        let text = render(&document);
        assert!(text.contains("  long:\n    - item-number-0\n"), "{text}");
        let back: Value = serde_yaml::from_str(&text).unwrap_or_default();
        assert_eq!(back, document.to_json());
    }

    #[test]
    fn json_round_trips_through_nodes() {
        let value = serde_json::json!({"a": [1, "b", {"c": true}], "d": {}, "e": 1.5, "f": null});
        let node = Node::from_json(&value);
        let back = node.to_json();
        assert_eq!(back["a"], value["a"]);
        assert_eq!(back["e"], "1.5");
        assert_eq!(back["f"], "");
    }
}
