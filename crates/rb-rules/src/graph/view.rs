//! A rule's narrowed graph: the edges and chains a rule's `graph` takes out.
//!
//! - Decision: [ADR-0038](../../../../docs/adr/0038-a-rule-narrows-the-graph-it-sees.md)
//! - Plan: [Wave 2, Step 11](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f)
//! - Requirements: [FR-RULE-07](../../../../docs/prd.md#fr-rule-07),
//!   [FR-RULE-08](../../../../docs/prd.md#fr-rule-08)
//!
//! [`View::removes`] answers for one edge, from the importer's path, the edge's `resolved` and
//! its dependency types; [`View::passes`] says whether a chain may continue from a module. The
//! direct-edge matcher, the reachability derivation and the slice edges all ask the same two
//! questions, so a rule sees one graph wherever it looks. The engine compares the strings the
//! document carries and never asks which language an edge is in
//! ([ADR-0010](../../../../docs/adr/0010-crate-layout-and-extractor-boundary.md)).

use std::sync::Arc;

use rb_config::model::{GraphFilter, IgnoredEdges};
use rb_config::pattern::Matcher;
use rb_model::options::Patterns;
use serde_json::Value;

use crate::js;
use crate::patterns;

/// One pattern list, joined and compiled; `None` when it is not written. A pattern that does not
/// compile matches nothing, as everywhere in the engine (the loader refuses it first).
#[derive(Debug, Clone)]
enum Compiled {
    Absent,
    Present(Option<Arc<Matcher>>),
}

impl Compiled {
    fn new(patterns: Option<&Patterns>) -> Self {
        patterns.map_or(Self::Absent, |p| Self::Present(patterns::get(&p.joined())))
    }

    /// Whether the text matches; `absent` is the answer when nothing is written.
    fn test(&self, text: &str, absent: bool) -> bool {
        match self {
            Self::Absent => absent,
            Self::Present(matcher) => matcher.as_ref().is_some_and(|m| m.is_match(text)),
        }
    }
}

/// A compiled `graph.ignore` entry.
#[derive(Debug, Clone)]
struct Entry {
    from: Compiled,
    to: Compiled,
}

impl Entry {
    fn new(entry: &IgnoredEdges) -> Self {
        Self {
            from: Compiled::new(entry.from.as_ref()),
            to: Compiled::new(entry.to.as_ref()),
        }
    }

    /// A side left out matches every edge; the loader refuses an entry with neither.
    fn matches(&self, from: &str, to: &str) -> bool {
        self.from.test(from, true) && self.to.test(to, true)
    }
}

/// A rule's `graph`, compiled once.
#[derive(Debug, Clone)]
pub struct View {
    ignore: Vec<Entry>,
    types_not: Vec<&'static str>,
    modules_not: Compiled,
    chains_through: Compiled,
}

impl View {
    /// Compiles a `graph`.
    pub fn new(filter: &GraphFilter) -> Self {
        Self {
            ignore: filter.ignore.iter().map(Entry::new).collect(),
            types_not: filter
                .dependency_types_not
                .iter()
                .flatten()
                .map(|t| t.as_str())
                .collect(),
            modules_not: Compiled::new(filter.modules_not.as_ref()),
            chains_through: Compiled::new(filter.chains_through.as_ref()),
        }
    }

    /// Whether the rule's graph lacks the edge from `from` to `to` carrying `types`.
    pub fn removes<'t>(
        &self,
        from: &str,
        to: &str,
        types: impl IntoIterator<Item = &'t str>,
    ) -> bool {
        self.modules_not.test(from, false)
            || self.modules_not.test(to, false)
            || types.into_iter().any(|t| self.types_not.contains(&t))
            || self.ignore.iter().any(|e| e.matches(from, to))
    }

    /// [`Self::removes`] for a dependency of the module whose path is `from`, read from the
    /// document: its `resolved` and its `dependencyTypes`.
    pub fn removes_dependency(&self, from: &str, dependency: &Value) -> bool {
        self.removes(
            from,
            &js::text(dependency, "resolved"),
            js::array(dependency, "dependencyTypes")
                .iter()
                .filter_map(Value::as_str),
        )
    }

    /// Whether a chain may continue from the module whose path is `module`: always, unless
    /// `chainsThrough` is written and the path does not match it.
    pub fn passes(&self, module: &str) -> bool {
        self.chains_through.test(module, true)
    }

    /// The `graph.ignore` entries, by index, that match none of `edges` (`(from, resolved)`
    /// pairs): each an exception that no longer excuses anything, which liveness reports.
    pub fn unmatched<'e>(&self, edges: impl IntoIterator<Item = (&'e str, &'e str)>) -> Vec<usize> {
        let mut matched = vec![false; self.ignore.len()];
        let mut left = self.ignore.len();
        for (from, to) in edges {
            if left == 0 {
                break;
            }
            for (at, entry) in self.ignore.iter().enumerate() {
                if !matched[at] && entry.matches(from, to) {
                    matched[at] = true;
                    left -= 1;
                }
            }
        }
        matched
            .iter()
            .enumerate()
            .filter(|(_, m)| !**m)
            .map(|(at, _)| at)
            .collect()
    }
}

/// Every `(source, resolved)` edge of the modules, for [`View::unmatched`].
pub fn edges(modules: &[Value]) -> Vec<(&str, &str)> {
    modules
        .iter()
        .flat_map(|m| {
            let source = js::str_of(m, "source").unwrap_or_default();
            js::array(m, "dependencies")
                .iter()
                .map(move |d| (source, js::str_of(d, "resolved").unwrap_or_default()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_model::DependencyType;
    use serde_json::json;

    fn view(value: Value) -> View {
        View::new(&serde_json::from_value(value).unwrap_or_default())
    }

    #[test]
    fn each_key_removes_what_it_names() {
        let local = ["local"];
        let typed = ["local", "type-only"];
        let cases: [(Value, &str, &str, &[&str], bool); 14] = [
            (json!({}), "a", "b", &typed, false),
            (
                json!({ "ignore": [{ "from": "^a$", "to": "^b$" }] }),
                "a",
                "b",
                &local,
                true,
            ),
            (
                json!({ "ignore": [{ "from": "^a$", "to": "^b$" }] }),
                "a",
                "c",
                &local,
                false,
            ),
            (
                json!({ "ignore": [{ "from": "^a$", "to": "^b$" }] }),
                "c",
                "b",
                &local,
                false,
            ),
            (
                json!({ "ignore": [{ "from": "^a$" }] }),
                "a",
                "anything",
                &local,
                true,
            ),
            (
                json!({ "ignore": [{ "to": "^b$" }] }),
                "anyone",
                "b",
                &local,
                true,
            ),
            (
                json!({ "ignore": [{ "to": "^x$" }, { "to": "^b$" }] }),
                "a",
                "b",
                &local,
                true,
            ),
            (
                json!({ "dependencyTypesNot": ["type-only"] }),
                "a",
                "b",
                &typed,
                true,
            ),
            (
                json!({ "dependencyTypesNot": ["type-only"] }),
                "a",
                "b",
                &local,
                false,
            ),
            (
                json!({ "dependencyTypesNot": ["dynamic", "type-only"] }),
                "a",
                "b",
                &typed,
                true,
            ),
            (json!({ "modulesNot": "^n/" }), "n/x", "b", &local, true),
            (json!({ "modulesNot": "^n/" }), "a", "n/x", &local, true),
            (json!({ "modulesNot": "^n/" }), "a", "b", &local, false),
            (json!({ "chainsThrough": "^a" }), "z", "y", &local, false),
        ];
        for (graph, from, to, types, removed) in cases {
            assert_eq!(
                view(graph.clone()).removes(from, to, types.iter().copied()),
                removed,
                "{graph}: {from} -> {to} {types:?}"
            );
        }
    }

    #[test]
    fn chains_pass_where_the_pattern_matches() {
        let open = view(json!({ "modulesNot": "^n" }));
        assert!(open.passes("anything"));
        let through = view(json!({ "chainsThrough": ["^src/", "^lib/"] }));
        assert!(through.passes("src/a.py"));
        assert!(through.passes("lib/b.py"));
        assert!(!through.passes("tests/c.py"));
        let broken = view(json!({ "chainsThrough": "(?=x)" }));
        assert!(
            !broken.passes("x"),
            "an uncompilable pattern matches nothing"
        );
    }

    #[test]
    fn a_document_dependency_is_read_by_resolved_and_types() {
        let v = view(
            json!({ "ignore": [{ "from": "^a$", "to": "^b$" }], "dependencyTypesNot": ["type-only"] }),
        );
        assert!(v.removes_dependency(
            "a",
            &json!({ "resolved": "b", "dependencyTypes": ["local"] })
        ));
        assert!(v.removes_dependency(
            "x",
            &json!({ "resolved": "y", "dependencyTypes": ["local", "type-only"] })
        ));
        assert!(!v.removes_dependency(
            "x",
            &json!({ "resolved": "y", "dependencyTypes": ["local"] })
        ));
        assert!(!v.removes_dependency("x", &json!({ "resolved": "b" })));
        assert_eq!(DependencyType::TypeOnly.as_str(), "type-only");
    }

    #[test]
    fn unmatched_entries_are_the_ones_no_edge_matches() {
        let v = view(
            json!({ "ignore": [{ "from": "^a$" }, { "to": "^gone$" }, { "from": "^b$", "to": "^c$" }, { "to": "^c$" }] }),
        );
        let modules = [
            json!({ "source": "a", "dependencies": [{ "resolved": "x" }] }),
            json!({ "source": "b", "dependencies": [{ "resolved": "c" }, { "resolved": "d" }] }),
            json!({ "source": "lonely" }),
        ];
        let all = edges(&modules);
        assert_eq!(all, [("a", "x"), ("b", "c"), ("b", "d")]);
        assert_eq!(v.unmatched(all.iter().copied()), [1]);
        assert_eq!(v.unmatched(std::iter::empty()), [0, 1, 2, 3]);
        assert!(view(json!({ "modulesNot": "x" })).unmatched(all).is_empty());
        let single = view(json!({ "ignore": [{ "from": "^a$" }] }));
        assert!(single.unmatched([("a", "x"), ("a", "y")]).is_empty());
    }

    proptest::proptest! {
        /// Over any edge, an ignore entry removes exactly the edges both of its sides match, and
        /// `modulesNot` exactly the edges touching a matching module.
        #[test]
        fn removal_is_the_conjunction_of_the_sides(
            from in "[abc]{1,3}",
            to in "[abc]{1,3}",
            entry_from in proptest::option::of("[abc]"),
            entry_to in proptest::option::of("[abc]"),
            module in "[abc]",
        ) {
            proptest::prop_assume!(entry_from.is_some() || entry_to.is_some());
            let mut entry = serde_json::Map::new();
            if let Some(f) = &entry_from {
                entry.insert("from".into(), json!(format!("^{f}")));
            }
            if let Some(t) = &entry_to {
                entry.insert("to".into(), json!(format!("^{t}")));
            }
            let ignore = view(json!({ "ignore": [entry] }));
            let expected = entry_from.as_ref().is_none_or(|f| from.starts_with(f.as_str()))
                && entry_to.as_ref().is_none_or(|t| to.starts_with(t.as_str()));
            proptest::prop_assert_eq!(ignore.removes(&from, &to, ["local"]), expected);
            proptest::prop_assert_eq!(ignore.unmatched([(from.as_str(), to.as_str())]).is_empty(), expected);
            let modules_not = view(json!({ "modulesNot": format!("^{module}") }));
            proptest::prop_assert_eq!(
                modules_not.removes(&from, &to, ["local"]),
                from.starts_with(module.as_str()) || to.starts_with(module.as_str())
            );
        }
    }
}
