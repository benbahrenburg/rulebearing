//! The graph filters: `exclude`, `includeOnly`, `focus` with `depth`, `reaches` and `highlight`.
//! dependency-cruiser 18.2.0's `filter-bank.mjs` and `add-focus.mjs`, ported.
//!
//! - Specification: `test/graph-utl/filter-bank.spec.mjs`, `add-focus.spec.mjs`, run by
//!   conformance gate 1 layer 2
//! - Coverage: [coverage § Options](../../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   rows `focus.path`, `focus.depth`, `includeOnly.path`, `reaches.path`, `exclude.path`
//! - Plan: [Wave 1, Step 6](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-6-graph-analysis-1b)
//!   (`filters.rs`)
//! - Requirement: [FR-CLI-08](../../../../docs/prd.md#fr-cli-08)
//!
//! The filters run in upstream's order: `exclude`, `includeOnly`, `focus`, `reaches`,
//! `highlight`. `fmt` applies them to a saved result; `cruise` applies `focus`, `reaches` and
//! `highlight` after analysis (the extractor applies `exclude` and `includeOnly` while it walks).

use std::collections::HashSet;

use serde_json::Value;

use crate::graph::indexed::IndexedGraph;
use crate::js;
use crate::patterns;

/// One filter: a pattern and, for `focus`, a depth.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// The pattern; a filter without one does nothing.
    pub path: Option<String>,
    /// `focus.depth`. Default 1.
    pub depth: Option<u32>,
}

impl Filter {
    /// A filter from dependency-cruiser's shape: a string, an array, or `{ path, depth }`.
    pub fn from_value(value: &Value) -> Self {
        let joined = |v: &Value| match v {
            Value::String(s) => Some(s.clone()),
            Value::Array(items) => Some(
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("|"),
            ),
            _ => None,
        };
        match value {
            Value::Object(map) => Self {
                path: map.get("path").and_then(joined),
                depth: map
                    .get("depth")
                    .and_then(Value::as_u64)
                    .and_then(|d| u32::try_from(d).ok()),
            },
            other => Self {
                path: joined(other),
                depth: None,
            },
        }
    }

    fn pattern(&self) -> Option<&str> {
        self.path.as_deref().filter(|p| !p.is_empty())
    }
}

/// The filters `applyFilters` takes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filters {
    /// `exclude`.
    pub exclude: Option<Filter>,
    /// `includeOnly`.
    pub include_only: Option<Filter>,
    /// `focus`.
    pub focus: Option<Filter>,
    /// `reaches`.
    pub reaches: Option<Filter>,
    /// `highlight`.
    pub highlight: Option<Filter>,
}

impl Filters {
    /// Whether any filter is set.
    pub fn is_empty(&self) -> bool {
        self.exclude.is_none()
            && self.include_only.is_none()
            && self.focus.is_none()
            && self.reaches.is_none()
            && self.highlight.is_none()
    }
}

fn module_matches(module: &Value, pattern: &str) -> bool {
    patterns::test(pattern, &js::text(module, "source"))
}

fn dependency_matches(dependency: &Value, pattern: &str) -> bool {
    patterns::test(pattern, &js::text(dependency, "resolved"))
}

fn retain_dependencies(module: &mut Value, keep: impl Fn(&Value) -> bool) {
    if let Some(Value::Array(dependencies)) = module.get_mut("dependencies") {
        dependencies.retain(|d| keep(d));
    }
}

/// `exclude`.
pub fn exclude(modules: Vec<Value>, filter: &Filter) -> Vec<Value> {
    let Some(pattern) = filter.pattern() else {
        return modules;
    };
    modules
        .into_iter()
        .filter(|m| !module_matches(m, pattern))
        .map(|mut m| {
            retain_dependencies(&mut m, |d| !dependency_matches(d, pattern));
            m
        })
        .collect()
}

/// `includeOnly`.
pub fn include_only(modules: Vec<Value>, filter: &Filter) -> Vec<Value> {
    let Some(pattern) = filter.pattern() else {
        return modules;
    };
    modules
        .into_iter()
        .filter(|m| module_matches(m, pattern))
        .map(|mut m| {
            retain_dependencies(&mut m, |d| dependency_matches(d, pattern));
            m
        })
        .collect()
}

/// `addFocus`: the focused modules and their neighbours to `depth` (default 1, 0 for all),
/// each tagged `matchesFocus`.
pub fn add_focus(modules: Vec<Value>, filter: &Filter) -> Vec<Value> {
    let Some(pattern) = filter.pattern() else {
        return modules;
    };
    let depth = filter.depth.unwrap_or(1);
    let focused: Vec<String> = modules
        .iter()
        .filter(|m| module_matches(m, pattern))
        .map(|m| js::text(m, "source").into_owned())
        .collect();
    let graph = IndexedGraph::new(&modules, "source");
    let mut reachable: HashSet<String> = HashSet::new();
    for name in &focused {
        reachable.extend(graph.transitive_dependents(name, depth));
        reachable.extend(graph.transitive_dependencies(name, depth));
    }
    let focused: HashSet<String> = focused.into_iter().collect();
    modules
        .into_iter()
        .filter(|m| reachable.contains(js::text(m, "source").as_ref()))
        .map(|mut m| {
            retain_dependencies(&mut m, |d| {
                reachable.contains(js::text(d, "resolved").as_ref())
            });
            let is_focused = focused.contains(js::text(&m, "source").as_ref());
            js::set(&mut m, "matchesFocus", Value::Bool(is_focused));
            m
        })
        .collect()
}

/// `filterReaches`: the modules that reach a matching module, tagged `matchesReaches`.
pub fn reaches(modules: Vec<Value>, filter: &Filter) -> Vec<Value> {
    let Some(pattern) = filter.pattern() else {
        return modules;
    };
    let to_reach: Vec<String> = modules
        .iter()
        .filter(|m| module_matches(m, pattern))
        .map(|m| js::text(m, "source").into_owned())
        .collect();
    let graph = IndexedGraph::new(&modules, "source");
    let mut reaching: HashSet<String> = HashSet::new();
    for name in &to_reach {
        reaching.extend(graph.transitive_dependents(name, 0));
    }
    let to_reach: HashSet<String> = to_reach.into_iter().collect();
    modules
        .into_iter()
        .filter(|m| reaching.contains(js::text(m, "source").as_ref()))
        .map(|mut m| {
            let matches = to_reach.contains(js::text(&m, "source").as_ref());
            js::set(&mut m, "matchesReaches", Value::Bool(matches));
            retain_dependencies(&mut m, |d| {
                reaching.contains(js::text(d, "resolved").as_ref())
            });
            m
        })
        .collect()
}

/// `tagHighlight`: every module tagged `matchesHighlight`.
pub fn highlight(modules: Vec<Value>, filter: &Filter) -> Vec<Value> {
    let pattern = filter.path.clone().unwrap_or_default();
    modules
        .into_iter()
        .map(|mut m| {
            let matches = module_matches(&m, &pattern);
            js::set(&mut m, "matchesHighlight", Value::Bool(matches));
            m
        })
        .collect()
}

/// `applyFilters`, in upstream's order.
pub fn apply(modules: Vec<Value>, filters: &Filters) -> Vec<Value> {
    let mut out = modules;
    if let Some(f) = &filters.exclude {
        out = exclude(out, f);
    }
    if let Some(f) = &filters.include_only {
        out = include_only(out, f);
    }
    if let Some(f) = &filters.focus {
        out = add_focus(out, f);
    }
    if let Some(f) = &filters.reaches {
        out = reaches(out, f);
    }
    if let Some(f) = &filters.highlight {
        out = highlight(out, f);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn graph() -> Vec<Value> {
        let edge = |to: &str| json!({ "resolved": to });
        vec![
            json!({ "source": "src/main.ts", "dependencies": [edge("src/a.ts"), edge("node_modules/x")], "dependents": [] }),
            json!({ "source": "src/a.ts", "dependencies": [edge("src/b.ts")], "dependents": ["src/main.ts"] }),
            json!({ "source": "src/b.ts", "dependencies": [], "dependents": ["src/a.ts"] }),
            json!({ "source": "node_modules/x", "dependencies": [], "dependents": ["src/main.ts"] }),
        ]
    }

    fn sources(modules: &[Value]) -> Vec<&str> {
        modules
            .iter()
            .filter_map(|m| m["source"].as_str())
            .collect()
    }

    fn filter(path: &str, depth: Option<u32>) -> Filter {
        Filter {
            path: Some(path.into()),
            depth,
        }
    }

    #[test]
    fn exclude_and_include_only() {
        let out = exclude(graph(), &filter("node_modules", None));
        assert_eq!(sources(&out), ["src/main.ts", "src/a.ts", "src/b.ts"]);
        assert_eq!(out[0]["dependencies"].as_array().map(Vec::len), Some(1));
        assert_eq!(out[0]["dependencies"][0]["resolved"], "src/a.ts");
        let out = include_only(graph(), &filter("^src/(main|a)", None));
        assert_eq!(sources(&out), ["src/main.ts", "src/a.ts"]);
        assert_eq!(out[1]["dependencies"].as_array().map(Vec::len), Some(0));
        assert_eq!(exclude(graph(), &Filter::default()).len(), 4);
        assert_eq!(include_only(graph(), &filter("", None)).len(), 4);
    }

    #[test]
    fn focus_takes_neighbours_to_depth() {
        let out = add_focus(graph(), &filter("^src/a", None));
        assert_eq!(sources(&out), ["src/main.ts", "src/a.ts", "src/b.ts"]);
        assert_eq!(out[1]["matchesFocus"], true);
        assert_eq!(out[0]["matchesFocus"], false);
        let deep = add_focus(graph(), &filter("^src/b", Some(0)));
        assert_eq!(sources(&deep), ["src/main.ts", "src/a.ts", "src/b.ts"]);
        let one = add_focus(graph(), &filter("^src/b", Some(1)));
        assert_eq!(sources(&one), ["src/a.ts", "src/b.ts"]);
        assert_eq!(add_focus(graph(), &Filter::default()).len(), 4);
    }

    #[test]
    fn reaches_and_highlight() {
        let out = reaches(graph(), &filter("^src/b", None));
        assert_eq!(sources(&out), ["src/main.ts", "src/a.ts", "src/b.ts"]);
        assert_eq!(out[2]["matchesReaches"], true);
        assert_eq!(out[0]["matchesReaches"], false);
        assert_eq!(out[0]["dependencies"].as_array().map(Vec::len), Some(1));
        let lit = highlight(graph(), &filter("^src/a", None));
        assert_eq!(lit[1]["matchesHighlight"], true);
        assert_eq!(lit[0]["matchesHighlight"], false);
        assert_eq!(reaches(graph(), &Filter::default()).len(), 4);
    }

    #[test]
    fn apply_runs_in_order() {
        let filters = Filters {
            exclude: Some(filter("node_modules", None)),
            include_only: Some(filter("^src", None)),
            focus: Some(filter("^src/b", None)),
            reaches: Some(filter("^src/b", None)),
            highlight: Some(filter("^src/b", None)),
        };
        assert!(!filters.is_empty());
        assert!(Filters::default().is_empty());
        let one = |set: fn(&mut Filters)| {
            let mut filters = Filters::default();
            set(&mut filters);
            filters.is_empty()
        };
        assert!(!one(|f| f.exclude = Some(Filter::default())));
        assert!(!one(|f| f.include_only = Some(Filter::default())));
        assert!(!one(|f| f.focus = Some(Filter::default())));
        assert!(!one(|f| f.reaches = Some(Filter::default())));
        assert!(!one(|f| f.highlight = Some(Filter::default())));
        let out = apply(graph(), &filters);
        assert_eq!(sources(&out), ["src/a.ts", "src/b.ts"]);
        assert_eq!(apply(graph(), &Filters::default()).len(), 4);
    }

    #[test]
    fn filters_read_every_shape() {
        assert_eq!(Filter::from_value(&json!("a")).path.as_deref(), Some("a"));
        assert_eq!(
            Filter::from_value(&json!(["a", "b"])).path.as_deref(),
            Some("a|b")
        );
        let object = Filter::from_value(&json!({ "path": ["a"], "depth": 2 }));
        assert_eq!(object, filter("a", Some(2)));
        assert_eq!(Filter::from_value(&json!(3)), Filter::default());
    }
}
