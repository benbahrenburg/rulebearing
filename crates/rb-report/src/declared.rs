//! The mark a reporter prints on a declared edge, so a declared edge is told apart from a detected
//! one in every reporter that prints dependencies or violations.
//!
//! - Requirement: [FR-REACH-03](../../../docs/prd.md#fr-reach-03) ("distinguishable from a
//!   detected edge in every reporter")
//! - Decisions: [ADR-0078](../../../docs/adr/0078-fleet-and-declared-edges.md) (the fields of a
//!   declared dependency), [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md)
//!   (declared edges are marked, never inferred), [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//!   (additions are additive)
//! - Plan: [plan 0004, Step 12](../../../docs/plans/pending/0004-wave-4-reach.md#24-steps-for-sub-wave-4d-fleet-and-declared-edges)
//! - Reference: [reporters.md § Declared edges](../../../docs/reporters.md#declared-edges)
//!
//! A dependency is declared when it carries `declared: true`. An edge between two modules is
//! declared when the report has at least one dependency of the one on the other and every such
//! dependency is declared: when a detected edge joins the same two modules, the finding stands on
//! the detected edge and carries no mark. The text forms print ` (declared: <kind>)` after the
//! target, with each distinct `declaredKind` in the order the dependencies give them, joined by
//! `, `, or ` (declared)` when none carries a kind. A result without declared edges, which is every
//! result dependency-cruiser writes and every `cruise` result, gets no mark anywhere, so every
//! byte-compared output is unchanged.

use serde_json::Value;

use crate::{Edges, text};

/// Whether `dependency` is declared: `declared: true`.
pub(crate) fn is_declared(dependency: &Value) -> bool {
    dependency.get("declared").and_then(Value::as_bool) == Some(true)
}

/// The kinds of the declared edge `dependencies` stand for: `Some` when there is at least one and
/// every one is declared, with each distinct `declaredKind` in order (none when no dependency
/// carries one); `None` otherwise.
pub(crate) fn kinds<'a>(dependencies: impl IntoIterator<Item = &'a Value>) -> Option<Vec<String>> {
    let mut kinds: Vec<String> = Vec::new();
    let mut any = false;
    for dependency in dependencies {
        if !is_declared(dependency) {
            return None;
        }
        any = true;
        if let Some(kind) = dependency.get("declaredKind").and_then(Value::as_str)
            && !kinds.iter().any(|k| k == kind)
        {
            kinds.push(kind.to_owned());
        }
    }
    any.then_some(kinds)
}

/// The words of the mark: `declared`, or `declared: http, queue`.
pub(crate) fn label(kinds: &[String]) -> String {
    if kinds.is_empty() {
        "declared".to_owned()
    } else {
        format!("declared: {}", kinds.join(", "))
    }
}

/// The mark the text forms print after a target: ` (declared: http)`.
pub(crate) fn mark(kinds: &[String]) -> String {
    format!(" ({})", label(kinds))
}

/// The mark for the edge `from -> to`, empty when the edge is not declared.
pub(crate) fn edge_mark(edges: &Edges<'_>, from: &str, to: &str) -> String {
    edges
        .declared(from, to)
        .map(|kinds| mark(&kinds))
        .unwrap_or_default()
}

/// Whether a violation stands on one edge, `from -> to`: a dependency or instability violation
/// (or one without a `type`, as dependency-cruiser's older results have).
pub(crate) fn on_one_edge(violation: &Value) -> bool {
    matches!(
        violation.get("type").and_then(Value::as_str),
        None | Some("dependency" | "instability")
    )
}

/// The mark a violation's target gets: its edge's, for a violation on one edge; empty otherwise.
pub(crate) fn violation_mark(edges: &Edges<'_>, violation: &Value) -> String {
    if on_one_edge(violation) {
        edge_mark(edges, &text(violation, "from"), &text(violation, "to"))
    } else {
        String::new()
    }
}

/// [`violation_mark`] without an index, for a reporter that looks up one violation's edge at a
/// time: the first module named `from` is found by a scan, as [`crate::edge_position`] finds it.
pub(crate) fn scanned_violation_mark(result: &Value, violation: &Value) -> String {
    if !on_one_edge(violation) {
        return String::new();
    }
    let from = text(violation, "from");
    let to = text(violation, "to");
    result
        .get("modules")
        .and_then(Value::as_array)
        .and_then(|modules| {
            modules
                .iter()
                .find(|m| m.get("source").and_then(Value::as_str) == Some(from.as_str()))
        })
        .and_then(|module| {
            kinds(
                module
                    .get("dependencies")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|d| d.get("resolved").and_then(Value::as_str) == Some(to.as_str())),
            )
        })
        .map(|kinds| mark(&kinds))
        .unwrap_or_default()
}

/// The steps of a cycle (`key` `cycle`) or a reachability path (`via`), each name followed by the
/// mark of the edge that reaches it: the first from the violation's `from`, each next from the step
/// before it.
pub(crate) fn step_names(edges: &Edges<'_>, violation: &Value, key: &str) -> Vec<String> {
    let mut previous = text(violation, "from");
    violation
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|step| {
            let name = text(step, "name");
            let marked = format!("{name}{}", edge_mark(edges, &previous, &name));
            previous = name;
            marked
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn result() -> Value {
        json!({ "modules": [
            { "source": "a", "dependencies": [
                { "resolved": "b", "declared": true, "declaredKind": "http" },
                { "resolved": "b", "declared": true, "declaredKind": "queue" },
                { "resolved": "b", "declared": true, "declaredKind": "http" },
                { "resolved": "c", "declared": true },
                { "resolved": "d", "declared": true, "declaredKind": "http" },
                { "resolved": "d" },
                { "resolved": "e", "declared": "yes" }
            ] },
            { "source": "b", "dependencies": [{ "resolved": "a" }] },
            { "source": "c", "dependencies": [{ "resolved": "b", "declared": true, "declaredKind": "grpc" }] }
        ] })
    }

    #[test]
    fn an_edge_is_declared_only_when_every_dependency_is() {
        let result = result();
        let edges = Edges::of(&result);
        assert_eq!(edge_mark(&edges, "a", "b"), " (declared: http, queue)");
        assert_eq!(edge_mark(&edges, "a", "c"), " (declared)", "no kind");
        assert_eq!(
            edge_mark(&edges, "a", "d"),
            "",
            "a detected edge joins them too"
        );
        assert_eq!(edge_mark(&edges, "a", "e"), "", "only `true` declares");
        assert_eq!(edge_mark(&edges, "b", "a"), "", "detected");
        assert_eq!(edge_mark(&edges, "a", "x"), "", "no edge");
        assert_eq!(edge_mark(&edges, "x", "a"), "", "no module");
        assert_eq!(kinds(std::iter::empty()), None);
        assert_eq!(label(&[]), "declared");
        assert_eq!(label(&["http".into()]), "declared: http");
    }

    #[test]
    fn a_violation_is_marked_on_its_one_edge_and_its_steps() {
        let result = result();
        let edges = Edges::of(&result);
        for kind in [None, Some("dependency"), Some("instability")] {
            let mut v = json!({ "from": "a", "to": "b" });
            if let Some(kind) = kind {
                v["type"] = json!(kind);
            }
            assert_eq!(
                violation_mark(&edges, &v),
                " (declared: http, queue)",
                "{kind:?}"
            );
        }
        for kind in ["module", "cycle", "reachability", "folder", "element"] {
            let v = json!({ "type": kind, "from": "a", "to": "b" });
            assert_eq!(violation_mark(&edges, &v), "", "{kind}");
        }
        let cycle = json!({ "type": "cycle", "from": "a", "cycle": [{ "name": "c" }, { "name": "b" }, { "name": "a" }] });
        assert_eq!(
            step_names(&edges, &cycle, "cycle"),
            ["c (declared)", "b (declared: grpc)", "a"]
        );
        assert!(step_names(&edges, &json!({ "from": "a" }), "via").is_empty());
        for v in [
            json!({ "from": "a", "to": "b" }),
            json!({ "type": "dependency", "from": "a", "to": "d" }),
            json!({ "type": "module", "from": "a", "to": "b" }),
            json!({ "type": "dependency", "from": "x", "to": "b" }),
        ] {
            assert_eq!(
                scanned_violation_mark(&result, &v),
                violation_mark(&edges, &v),
                "{v}"
            );
        }
        assert_eq!(
            scanned_violation_mark(&json!({}), &json!({ "from": "a", "to": "b" })),
            ""
        );
    }
}
