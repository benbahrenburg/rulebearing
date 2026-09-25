//! `mermaid`: the result as a Mermaid flowchart, folders as nested subgraphs, which a pull request
//! renders without `GraphViz`. dependency-cruiser 18.2.0's `src/report/mermaid.mjs`, ported.
//!
//! - Specification: `test/report/mermaid/mermaid.spec.mjs`, run unmodified by conformance gate 1
//!   layer 3 ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Coverage: [coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types),
//!   row `mermaid`, `d2`; [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   row `reporterOptions.mermaid.minify`
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01)
//!
//! `minify` (the default) names nodes by a base-36 counter in first-seen order; otherwise each
//! node is named from its path with the characters Mermaid cannot take replaced. Folders are
//! listed in JavaScript's object key order: names that are array indices first, ascending.

use std::collections::HashMap;

use serde_json::Value;

use crate::{Rendered, js};

/// The names of `Object.prototype`'s properties: a path segment with one of these names finds
/// the inherited property in upstream's plain-object tree, so the module is not added below it.
const INHERITED: &[&str] = &[
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "__proto__",
    "toLocaleString",
];

/// `hashToReadableNodeName`.
fn readable(name: &str) -> String {
    let unknown = name.replace('✖', "__unknown__");
    let current = if unknown == "." {
        "__currentPath__".to_owned()
    } else if let Some(rest) = unknown.strip_prefix("./") {
        format!("__currentPath__{rest}")
    } else {
        unknown
    };
    let previous = if current == ".." {
        "__prevPath__".to_owned()
    } else if let Some(rest) = current.strip_prefix("../") {
        format!("__prevPath__{rest}")
    } else {
        current
    };
    previous
        .chars()
        .map(|c| {
            if matches!(c, '[' | ']' | '/' | '.' | '@' | '~' | '-') {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// `Number.prototype.toString(36).toUpperCase()`.
fn base36(mut n: usize) -> String {
    const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[n % 36]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8_lossy(&out).into_owned()
}

/// `hashModuleNames`: every folder and module path to its node name, in first-seen order.
fn names(modules: &[Value], minify: bool) -> HashMap<String, String> {
    let mut names = HashMap::new();
    let mut count = 0usize;
    for module in modules {
        let source = js::field(module, "source");
        let parts: Vec<&str> = source.split('/').collect();
        for i in 0..parts.len() {
            let name = parts[..=i].join("/");
            if let std::collections::hash_map::Entry::Vacant(entry) = names.entry(name) {
                let node = if minify {
                    let node = base36(count);
                    count += 1;
                    node
                } else {
                    readable(entry.key())
                };
                entry.insert(node);
            }
        }
    }
    names
}

/// A folder or module in the subgraph tree.
#[derive(Debug, Default)]
struct Tree {
    /// The children, in insertion order.
    children: Vec<(String, Node)>,
}

#[derive(Debug)]
struct Node {
    name: String,
    text: String,
    tree: Tree,
}

impl Tree {
    fn child(&mut self, key: &str) -> Option<usize> {
        self.children.iter().position(|(k, _)| k == key)
    }
}

/// `convertSubgraphSources`.
fn tree(modules: &[Value], names: &HashMap<String, String>) -> Tree {
    let mut root = Tree::default();
    for module in modules {
        let source = js::field(module, "source");
        let parts: Vec<&str> = source.split('/').collect();
        let mut at = &mut root;
        for (i, part) in parts.iter().enumerate() {
            if INHERITED.contains(part) {
                break;
            }
            let found = at.child(part);
            let index = found.unwrap_or_else(|| {
                let path = parts[..=i].join("/");
                at.children.push((
                    (*part).to_owned(),
                    Node {
                        name: names
                            .get(&path)
                            .cloned()
                            .unwrap_or_else(|| "undefined".into()),
                        text: (*part).to_owned(),
                        tree: Tree::default(),
                    },
                ));
                at.children.len() - 1
            });
            at = &mut at.children[index].1.tree;
        }
    }
    root
}

fn render_node(node: &str, text: &str) -> String {
    format!("{node}[\"{}\"]", if text.is_empty() { " " } else { text })
}

/// `renderSubgraphs`.
fn subgraphs(tree: &Tree, minify: bool, depth: usize) -> String {
    let order = js::keys_in_order(tree.children.iter().map(|(k, _)| k.as_str()));
    order
        .iter()
        .filter_map(|key| tree.children.iter().find(|(k, _)| k == key))
        .map(|(_, source)| {
            let indent = if minify {
                String::new()
            } else {
                "  ".repeat(depth)
            };
            let children = subgraphs(&source.tree, minify, depth + 1);
            if children.is_empty() {
                format!("{indent}{}", render_node(&source.name, &source.text))
            } else {
                format!(
                    "{indent}subgraph {}\n{children}\n{indent}end",
                    render_node(&source.name, &source.text)
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn node_of<'a>(names: &'a HashMap<String, String>, name: &str) -> &'a str {
    names.get(name).map_or("undefined", String::as_str)
}

/// Renders `mermaid`, with `options` the `reporterOptions.mermaid` section.
pub fn render(result: &Value, options: Option<&Value>) -> Rendered {
    let minify = options
        .and_then(Value::as_object)
        .and_then(|o| o.get("minify"))
        .is_none_or(|m| js::truthy(Some(m)));
    let modules = rb_rules::js::array(result, "modules");
    let names = names(modules, minify);
    let tree = tree(modules, &names);
    let edges: Vec<String> = modules
        .iter()
        .flat_map(|module| {
            let from = node_of(&names, &js::field(module, "source")).to_owned();
            rb_rules::js::array(module, "dependencies")
                .iter()
                .map(|d| format!("{from}-->{}", node_of(&names, &js::field(d, "resolved"))))
                .collect::<Vec<_>>()
        })
        .collect();
    let highlights = modules
        .iter()
        .filter(|m| {
            ["matchesFocus", "matchesReaches", "matchesHighlight"]
                .iter()
                .any(|k| js::truthy(m.get(*k)))
        })
        .map(|m| {
            format!(
                "\nstyle {} fill:lime,color:black",
                node_of(&names, &js::field(m, "source"))
            )
        })
        .collect::<Vec<_>>()
        .concat();
    Rendered {
        output: format!(
            "flowchart LR\n\n{}\n{}\n{highlights}",
            subgraphs(&tree, minify, 0),
            edges.join("\n")
        ),
        exit_code: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn result() -> Value {
        json!({ "modules": [
            { "source": "src/main/index.js", "dependencies": [{ "resolved": "src/utl/a-b.js" }, { "resolved": "nowhere" }], "matchesFocus": true },
            { "source": "src/utl/a-b.js", "dependencies": [] },
            { "source": "2019/x.js", "dependencies": [] }
        ] })
    }

    #[test]
    fn readable_and_minified_names() {
        let open = render(&result(), Some(&json!({ "minify": false }))).output;
        assert_eq!(
            open,
            concat!(
                "flowchart LR\n\n",
                "subgraph 2019[\"2019\"]\n  2019_x_js[\"x.js\"]\nend\n",
                "subgraph src[\"src\"]\n  subgraph src_main[\"main\"]\n    src_main_index_js[\"index.js\"]\n  end\n",
                "  subgraph src_utl[\"utl\"]\n    src_utl_a_b_js[\"a-b.js\"]\n  end\nend\n",
                "src_main_index_js-->src_utl_a_b_js\nsrc_main_index_js-->undefined\n",
                "\nstyle src_main_index_js fill:lime,color:black"
            )
        );
        let minified = render(&result(), None).output;
        assert!(minified.starts_with("flowchart LR\n\nsubgraph 5[\"2019\"]\n6[\"x.js\"]\nend\nsubgraph 0[\"src\"]\nsubgraph 1[\"main\"]\n2[\"index.js\"]\nend"));
        assert!(minified.ends_with("\nstyle 2 fill:lime,color:black"));
        assert_eq!(
            render(&json!({ "modules": [] }), Some(&json!({ "minify": null }))).output,
            "flowchart LR\n\n\n\n"
        );
    }

    #[test]
    fn names_are_escaped() {
        assert_eq!(readable("./a/[id].jsx"), "__currentPath__a__id__jsx");
        assert_eq!(readable("."), "__currentPath__");
        assert_eq!(readable(".."), "__prevPath__");
        assert_eq!(readable("../x@y~z"), "__prevPath__x_y_z");
        assert_eq!(readable("✖"), "__unknown__");
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "Z");
        assert_eq!(base36(36), "10");
    }

    #[test]
    fn collapsed_and_inherited_names() {
        let collapsed = json!({ "modules": [{ "source": "src/", "dependencies": [] }] });
        assert_eq!(
            render(&collapsed, None).output,
            "flowchart LR\n\nsubgraph 0[\"src\"]\n1[\" \"]\nend\n\n"
        );
        let inherited = json!({ "modules": [{ "source": "a/constructor", "dependencies": [] }] });
        assert_eq!(
            render(&inherited, Some(&json!({ "minify": false }))).output,
            "flowchart LR\n\na[\"a\"]\n\n"
        );
    }
}
