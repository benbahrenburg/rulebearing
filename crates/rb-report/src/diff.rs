//! `diff`: what changed between two cruise results, and its three renderings (`json`,
//! `markdown`, `agent`).
//!
//! - Contract: [Wave 3 plan § 1.5](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//!   (the `diff` JSON shape)
//! - Plan: [Wave 3, Step 4](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)
//! - Source: [design § The subcommands a guard reaches for](../../../docs/artifacts/design.md#the-subcommands-a-guard-reaches-for)
//!   ("prints added and removed edges and new violations, for a review comment")
//! - Decisions: [ADR-0015](../../../docs/adr/0015-stable-violation-id.md) (violations are matched
//!   by their stable id), [ADR-0029](../../../docs/adr/0029-ratchets-enforced-by-cruise-and-reported-in-the-summary.md)
//!   (ratchet counts come from `summary.ratchets[]`)
//! - Requirement: [FR-CLI-01](../../../docs/prd.md#fr-cli-01)
//!
//! [`compute`] compares two graph documents:
//!
//! | Section | What it holds |
//! | --- | --- |
//! | `addedEdges`, `removedEdges` | each edge (`from` = the module's `source`, `to` = the dependency's `resolved`) on one side only; the position is the first dependency of that pair in document order, from the new side for an added edge and the old side for a removed one |
//! | `newViolations`, `resolvedViolations` | each violation whose stable id is on one side only; a result without ids (dependency-cruiser's) has them computed as the engine computes them. A violation at severity `ignore` (a known violation) is not a finding and is left out on both sides |
//! | `ratchets` | each ratchet whose count differs, or that exists on one side only (its other count is then absent); an unchanged ratchet is not listed, since the section says what moved |
//!
//! Every list is sorted: edges by `from` then `to`, violations by rule, `from`, `to` and id,
//! ratchets by name. Two violations with the same id are one finding and appear once. `base` and
//! `head` carry the revision and commit when they are known (the results' `revisionData.SHA1`,
//! or what `diff --base` resolved) and are left out, not null, when they are not.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use rb_model::violation_id::violation_id;
use rb_model::{GraphDocument, Severity, Violation, ViolationType};
use serde::Serialize;
use serde_json::Value;

/// The output types `diff` renders.
pub const DIFF_OUTPUT_TYPES: &[&str] = &["json", "markdown", "agent"];

/// Why a diff could not be rendered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DiffError {
    /// An output type `diff` does not render.
    #[error("`{0}` is not an output type of diff; use json, markdown or agent")]
    OutputType(String),
}

/// One side of the comparison: which revision it is, when known.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Side {
    /// The revision as it was named (`main`), when one was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// The commit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
}

impl Side {
    /// Whether nothing is known about the side.
    pub fn is_unknown(&self) -> bool {
        self.revision.is_none() && self.sha.is_none()
    }

    /// The side as a document's receipt records it: the commit of `revisionData.SHA1`.
    pub fn of(document: &GraphDocument) -> Self {
        Self {
            revision: None,
            sha: document
                .revision_data
                .as_ref()
                .map(|r| r.sha1.clone())
                .filter(|s| !s.is_empty()),
        }
    }
}

/// An edge on one side only.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    /// The importing module.
    pub from: String,
    /// The imported module, as resolved.
    pub to: String,
    /// The line of the import, when the extractor recorded it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// The column of the import, when the extractor recorded it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

/// A violation on one side only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// The stable id ([ADR-0015](../../../docs/adr/0015-stable-violation-id.md)).
    pub id: String,
    /// The rule's name.
    pub rule: String,
    /// The rule's severity.
    pub severity: Severity,
    /// Where the violation starts.
    pub from: String,
    /// Where it ends.
    pub to: String,
    /// The line of the edge, when the violation is an edge whose position was recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// The column of the edge, likewise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// The rule's `fix` text, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

/// A ratchet whose count moved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RatchetChange {
    /// The ratchet's name.
    pub name: String,
    /// The count on the old side; absent when the ratchet did not exist there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<u64>,
    /// The count on the new side; absent when the ratchet does not exist there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<u64>,
}

/// What changed between two results: the frozen JSON shape of plan 0003 § 1.5.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    /// The old side, when anything is known about it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<Side>,
    /// The new side, when anything is known about it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<Side>,
    /// Edges only the new side has.
    pub added_edges: Vec<Edge>,
    /// Edges only the old side has.
    pub removed_edges: Vec<Edge>,
    /// Violations only the new side has.
    pub new_violations: Vec<Finding>,
    /// Violations only the old side has.
    pub resolved_violations: Vec<Finding>,
    /// Ratchets whose count differs.
    pub ratchets: Vec<RatchetChange>,
}

impl Diff {
    /// Whether nothing changed (the sides are not compared).
    pub fn is_empty(&self) -> bool {
        self.added_edges.is_empty()
            && self.removed_edges.is_empty()
            && self.new_violations.is_empty()
            && self.resolved_violations.is_empty()
            && self.ratchets.is_empty()
    }

    /// The new violations at severity `error`: what `diff --exit-code` counts.
    pub fn new_errors(&self) -> u64 {
        self.new_violations
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count() as u64
    }

    /// Sets the sides, leaving out one that is entirely unknown.
    #[must_use]
    pub fn with_sides(mut self, base: Side, head: Side) -> Self {
        self.base = (!base.is_unknown()).then_some(base);
        self.head = (!head.is_unknown()).then_some(head);
        self
    }
}

/// Edges keyed by `(from, to)`, each with the line and column of its first dependency.
type Positions = BTreeMap<(String, String), (Option<u32>, Option<u32>)>;

/// Each edge of `document` with the position of its first dependency, keyed by `(from, to)`.
fn edges(document: &GraphDocument) -> Positions {
    let mut out = BTreeMap::new();
    for module in &document.modules {
        for dependency in &module.dependencies {
            out.entry((module.source.clone(), dependency.resolved.clone()))
                .or_insert((dependency.line, dependency.column));
        }
    }
    out
}

/// The dependency kind of each edge, the last dependency of a pair winning, as the engine's
/// annotation reads it when it computes the id.
fn kinds(document: &GraphDocument) -> BTreeMap<(&str, &str), &'static str> {
    let mut out = BTreeMap::new();
    for module in &document.modules {
        for dependency in &module.dependencies {
            if let Some(kind) = dependency.dependency_kind {
                out.insert(
                    (module.source.as_str(), dependency.resolved.as_str()),
                    kind.as_str(),
                );
            }
        }
    }
    out
}

/// The stable id of `violation`: its own, else computed over the rule, the ends and, for an edge
/// violation, the edge's dependency kind ([ADR-0015](../../../docs/adr/0015-stable-violation-id.md)).
pub fn id_of(violation: &Violation, kinds: &BTreeMap<(&str, &str), &'static str>) -> String {
    if let Some(id) = violation.id.as_ref().filter(|i| !i.is_empty()) {
        return id.clone();
    }
    let edge = matches!(
        violation.violation_type,
        Some(ViolationType::Dependency | ViolationType::Cycle | ViolationType::Instability)
    );
    let kind = if edge {
        kinds
            .get(&(violation.from.as_str(), violation.to.as_str()))
            .copied()
            .unwrap_or_default()
    } else {
        ""
    };
    violation_id(&violation.rule.name, &violation.from, &violation.to, kind)
}

/// The rule's `fix`: the violation's own, else the rule's in `summary.ruleSetUsed`.
fn fix_of(violation: &Violation, document: &GraphDocument) -> Option<String> {
    violation.fix.clone().or_else(|| {
        let rule_set = document.summary.rule_set_used.as_ref()?;
        ["forbidden", "required"]
            .iter()
            .filter_map(|k| rule_set.get(*k).and_then(Value::as_array))
            .flatten()
            .find(|r| r.get("name").and_then(Value::as_str) == Some(&violation.rule.name))
            .and_then(|r| r.get("fix"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
}

/// Each finding of `document` by stable id, `ignore` left out.
fn findings(document: &GraphDocument) -> BTreeMap<String, Finding> {
    let kinds = kinds(document);
    let positions = edges(document);
    let mut out = BTreeMap::new();
    for violation in &document.summary.violations {
        if violation.rule.severity == Severity::Ignore {
            continue;
        }
        let id = id_of(violation, &kinds);
        let (line, column) = positions
            .get(&(violation.from.clone(), violation.to.clone()))
            .copied()
            .unwrap_or_default();
        out.entry(id.clone()).or_insert_with(|| Finding {
            id,
            rule: violation.rule.name.clone(),
            severity: violation.rule.severity,
            from: violation.from.clone(),
            to: violation.to.clone(),
            line,
            column,
            fix: fix_of(violation, document),
        });
    }
    out
}

fn ratchet_counts(document: &GraphDocument) -> BTreeMap<String, u64> {
    document
        .summary
        .ratchets
        .iter()
        .flatten()
        .map(|r| (r.name.clone(), r.count))
        .collect()
}

/// The edges of `one` that `other` lacks, sorted.
fn only_in(one: &Positions, other: &Positions) -> Vec<Edge> {
    one.iter()
        .filter(|(k, _)| !other.contains_key(*k))
        .map(|((from, to), (line, column))| Edge {
            from: from.clone(),
            to: to.clone(),
            line: *line,
            column: *column,
        })
        .collect()
}

/// The findings of `one` whose id `other` lacks, sorted by rule, `from`, `to` and id.
fn findings_only_in(
    one: &BTreeMap<String, Finding>,
    other: &BTreeMap<String, Finding>,
) -> Vec<Finding> {
    let mut out: Vec<Finding> = one
        .iter()
        .filter(|(id, _)| !other.contains_key(*id))
        .map(|(_, f)| f.clone())
        .collect();
    out.sort_by(|a, b| (&a.rule, &a.from, &a.to, &a.id).cmp(&(&b.rule, &b.from, &b.to, &b.id)));
    out
}

/// What changed from `old` to `new`, with the sides their receipts record.
pub fn compute(old: &GraphDocument, new: &GraphDocument) -> Diff {
    let (old_edges, new_edges) = (edges(old), edges(new));
    let (old_findings, new_findings) = (findings(old), findings(new));
    let (before, after) = (ratchet_counts(old), ratchet_counts(new));
    let mut names: Vec<&String> = before.keys().chain(after.keys()).collect();
    names.sort();
    names.dedup();
    let ratchets = names
        .into_iter()
        .filter_map(|name| {
            let (b, a) = (before.get(name).copied(), after.get(name).copied());
            (b != a).then(|| RatchetChange {
                name: name.clone(),
                before: b,
                after: a,
            })
        })
        .collect();
    Diff {
        added_edges: only_in(&new_edges, &old_edges),
        removed_edges: only_in(&old_edges, &new_edges),
        new_violations: findings_only_in(&new_findings, &old_findings),
        resolved_violations: findings_only_in(&old_findings, &new_findings),
        ratchets,
        ..Diff::default()
    }
    .with_sides(Side::of(old), Side::of(new))
}

/// Renders `diff` as `output_type`: `json`, `markdown` or `agent`.
///
/// # Errors
/// [`DiffError::OutputType`] for any other type.
pub fn render(output_type: &str, diff: &Diff) -> Result<String, DiffError> {
    match output_type {
        "json" => Ok(json(diff)),
        "markdown" => Ok(markdown(diff)),
        "agent" => Ok(agent(diff)),
        other => Err(DiffError::OutputType(other.to_owned())),
    }
}

/// The frozen JSON shape, pretty-printed with a trailing newline, as the `json` reporter prints.
pub fn json(diff: &Diff) -> String {
    let mut out = serde_json::to_string_pretty(diff).unwrap_or_default();
    out.push('\n');
    out
}

/// `path:line:column`, or as much of it as is known.
fn position(path: &str, line: Option<u32>, column: Option<u32>) -> String {
    match (line, column) {
        (Some(l), Some(c)) => format!("{path}:{l}:{c}"),
        (Some(l), None) => format!("{path}:{l}"),
        _ => path.to_owned(),
    }
}

/// Text as a Markdown code span: fenced with one more backtick than the longest run inside,
/// padded when it starts or ends with one, and with `|` escaped so a table cell stays one cell.
fn code(text: &str) -> String {
    let longest = text
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or_default();
    let fence = "`".repeat(longest + 1);
    let pad = if text.starts_with('`') || text.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{}{pad}{fence}", text.replace('|', "\\|"))
}

/// Prose in a table cell: one line, `|` escaped.
fn cell(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

fn side_text(side: &Side) -> String {
    match (&side.revision, &side.sha) {
        (Some(r), Some(s)) => format!("{} at {}", code(r), code(s)),
        (Some(r), None) => code(r),
        (None, Some(s)) => code(s),
        (None, None) => String::new(),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn finding_table(out: &mut String, findings: &[Finding]) {
    out.push_str(
        "| Id | Rule | Severity | From | To | Fix |\n| --- | --- | --- | --- | --- | --- |\n",
    );
    for f in findings {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} |",
            code(&f.id),
            code(&f.rule),
            f.severity,
            code(&position(&f.from, f.line, f.column)),
            code(&f.to),
            f.fix.as_deref().map(cell).unwrap_or_default()
        );
    }
}

fn edge_table(out: &mut String, edges: &[Edge]) {
    out.push_str("| From | To |\n| --- | --- |\n");
    for e in edges {
        let _ = writeln!(
            out,
            "| {} | {} |",
            code(&position(&e.from, e.line, e.column)),
            code(&e.to)
        );
    }
}

/// The `markdown` rendering: a heading, the two sides when known, then one section per list, a
/// table when it has entries and one line when it has none. It is the body of the pull-request
/// comment of wave 4.
pub fn markdown(diff: &Diff) -> String {
    let mut out = String::from("## Architecture diff\n\n");
    let base = diff.base.as_ref().map(side_text).unwrap_or_default();
    let head = diff.head.as_ref().map(side_text).unwrap_or_default();
    match (base.is_empty(), head.is_empty()) {
        (false, false) => {
            let _ = writeln!(out, "Base {base}, head {head}.\n");
        }
        (false, true) => {
            let _ = writeln!(out, "Base {base}.\n");
        }
        (true, false) => {
            let _ = writeln!(out, "Head {head}.\n");
        }
        (true, true) => {}
    }
    let _ = writeln!(
        out,
        "{}, {}; {} added, {} removed; {}.\n",
        plural(diff.new_violations.len(), "new violation", "new violations"),
        plural(diff.resolved_violations.len(), "resolved", "resolved"),
        plural(diff.added_edges.len(), "edge", "edges"),
        diff.removed_edges.len(),
        plural(diff.ratchets.len(), "ratchet changed", "ratchets changed"),
    );
    out.push_str("### New violations\n\n");
    if diff.new_violations.is_empty() {
        out.push_str("No new violations.\n");
    } else {
        finding_table(&mut out, &diff.new_violations);
    }
    out.push_str("\n### Resolved violations\n\n");
    if diff.resolved_violations.is_empty() {
        out.push_str("No violations resolved.\n");
    } else {
        finding_table(&mut out, &diff.resolved_violations);
    }
    out.push_str("\n### Ratchets\n\n");
    if diff.ratchets.is_empty() {
        out.push_str("No ratchet counts changed.\n");
    } else {
        out.push_str("| Ratchet | Before | After |\n| --- | --- | --- |\n");
        let count = |n: Option<u64>| n.map_or_else(|| "absent".to_owned(), |n| n.to_string());
        for r in &diff.ratchets {
            let _ = writeln!(
                out,
                "| {} | {} | {} |",
                code(&r.name),
                count(r.before),
                count(r.after)
            );
        }
    }
    out.push_str("\n### Added edges\n\n");
    if diff.added_edges.is_empty() {
        out.push_str("No edges added.\n");
    } else {
        edge_table(&mut out, &diff.added_edges);
    }
    out.push_str("\n### Removed edges\n\n");
    if diff.removed_edges.is_empty() {
        out.push_str("No edges removed.\n");
    } else {
        edge_table(&mut out, &diff.removed_edges);
    }
    out
}

/// The `agent` rendering: one line per new violation, with its id, severity, rule, position and
/// `fix`, then one line of counts.
pub fn agent(diff: &Diff) -> String {
    let mut out = String::new();
    for f in &diff.new_violations {
        let _ = write!(
            out,
            "new {} {} {}: {} -> {}",
            f.id,
            f.severity,
            f.rule,
            position(&f.from, f.line, f.column),
            f.to
        );
        match &f.fix {
            Some(fix) => {
                let _ = writeln!(out, ". Fix: {}", cell(fix).replace("\\|", "|"));
            }
            None => out.push('\n'),
        }
    }
    let _ = writeln!(
        out,
        "diff: {}, {}; {} added, {} removed; {}",
        plural(diff.new_violations.len(), "new violation", "new violations"),
        plural(diff.resolved_violations.len(), "resolved", "resolved"),
        plural(diff.added_edges.len(), "edge", "edges"),
        diff.removed_edges.len(),
        plural(diff.ratchets.len(), "ratchet changed", "ratchets changed"),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rb_model::{
        Dependency, DependencyKind, Module, ModuleSystem, RatchetResult, RatchetStatus, RuleSummary,
    };

    fn module(source: &str, deps: &[(&str, u32)]) -> Module {
        let mut m = Module::new(source);
        m.dependencies = deps
            .iter()
            .map(|(to, line)| {
                let mut d = Dependency::new(*to, *to, ModuleSystem::Es6);
                d.line = Some(*line);
                d.column = Some(1);
                d
            })
            .collect();
        m
    }

    fn violation(rule: &str, severity: Severity, from: &str, to: &str) -> Violation {
        Violation {
            from: from.into(),
            to: to.into(),
            unresolved_to: None,
            dependency_types: None,
            violation_type: Some(ViolationType::Dependency),
            rule: RuleSummary {
                name: rule.into(),
                severity,
            },
            cycle: None,
            via: None,
            metrics: None,
            comment: None,
            id: None,
            fix: None,
            decision: None,
        }
    }

    fn ratchet(name: &str, count: u64) -> RatchetResult {
        RatchetResult {
            name: name.into(),
            budget: "b.json".into(),
            count,
            ceiling: Some(9),
            status: RatchetStatus::Held,
        }
    }

    fn old() -> GraphDocument {
        let mut d = GraphDocument {
            modules: vec![
                module("r/a.ts", &[("db/s.ts", 1)]),
                module("r/b.ts", &[("db/s.ts", 2)]),
            ],
            ..GraphDocument::default()
        };
        d.summary.violations = vec![
            violation("no-db", Severity::Error, "r/a.ts", "db/s.ts"),
            violation("no-db", Severity::Error, "r/b.ts", "db/s.ts"),
            violation("soft", Severity::Ignore, "r/a.ts", "db/s.ts"),
        ];
        d.summary.ratchets = Some(vec![ratchet("via-service", 2), ratchet("same", 4)]);
        d
    }

    fn new() -> GraphDocument {
        let mut d = GraphDocument {
            modules: vec![
                module("r/a.ts", &[("db/s.ts", 1)]),
                module("r/b.ts", &[("web/v.ts", 3)]),
            ],
            ..GraphDocument::default()
        };
        let mut added = violation("no-web", Severity::Warn, "r/b.ts", "web/v.ts");
        added.fix = Some("Return data | not views".into());
        d.summary.violations = vec![
            violation("no-db", Severity::Error, "r/a.ts", "db/s.ts"),
            added,
        ];
        d.summary.ratchets = Some(vec![
            ratchet("via-service", 1),
            ratchet("same", 4),
            ratchet("fresh", 0),
        ]);
        d
    }

    #[test]
    fn one_edge_each_way_one_violation_each_way_and_the_ratchets_that_moved() {
        let diff = compute(&old(), &new());
        assert_eq!(
            diff.added_edges,
            [Edge {
                from: "r/b.ts".into(),
                to: "web/v.ts".into(),
                line: Some(3),
                column: Some(1)
            }]
        );
        assert_eq!(
            diff.removed_edges,
            [Edge {
                from: "r/b.ts".into(),
                to: "db/s.ts".into(),
                line: Some(2),
                column: Some(1)
            }]
        );
        assert_eq!(diff.new_violations.len(), 1);
        let new = &diff.new_violations[0];
        assert_eq!(new.id, violation_id("no-web", "r/b.ts", "web/v.ts", ""));
        assert_eq!((new.line, new.column), (Some(3), Some(1)));
        assert_eq!(new.fix.as_deref(), Some("Return data | not views"));
        assert_eq!(new.severity, Severity::Warn);
        assert_eq!(diff.resolved_violations.len(), 1);
        assert_eq!(diff.resolved_violations[0].from, "r/b.ts");
        assert_eq!(diff.resolved_violations[0].rule, "no-db");
        assert_eq!(
            diff.ratchets,
            [
                RatchetChange {
                    name: "fresh".into(),
                    before: None,
                    after: Some(0)
                },
                RatchetChange {
                    name: "via-service".into(),
                    before: Some(2),
                    after: Some(1)
                }
            ]
        );
        assert_eq!(diff.new_errors(), 0, "the new violation is a warning");
        assert!(diff.base.is_none() && diff.head.is_none());
        assert!(!diff.is_empty());
    }

    #[test]
    fn the_json_shape_is_the_frozen_one() {
        let diff = compute(&old(), &new()).with_sides(
            Side {
                revision: Some("main".into()),
                sha: Some("abc".into()),
            },
            Side {
                revision: None,
                sha: Some("def".into()),
            },
        );
        let value: Value = serde_json::from_str(&json(&diff)).unwrap_or_default();
        let keys: Vec<&String> = value
            .as_object()
            .map(|o| o.keys().collect())
            .unwrap_or_default();
        assert_eq!(
            keys,
            [
                "base",
                "head",
                "addedEdges",
                "removedEdges",
                "newViolations",
                "resolvedViolations",
                "ratchets"
            ]
        );
        assert_eq!(
            value["base"],
            serde_json::json!({ "revision": "main", "sha": "abc" })
        );
        assert_eq!(value["head"], serde_json::json!({ "sha": "def" }));
        assert_eq!(
            value["addedEdges"][0],
            serde_json::json!({ "from": "r/b.ts", "to": "web/v.ts", "line": 3, "column": 1 })
        );
        let finding = &value["newViolations"][0];
        let keys: Vec<&String> = finding
            .as_object()
            .map(|o| o.keys().collect())
            .unwrap_or_default();
        assert_eq!(
            keys,
            [
                "id", "rule", "severity", "from", "to", "line", "column", "fix"
            ]
        );
        assert_eq!(finding["severity"], "warn");
        assert_eq!(
            value["ratchets"][0],
            serde_json::json!({ "name": "fresh", "after": 0 })
        );
        assert!(json(&Diff::default()).ends_with('\n'));
        assert_eq!(
            serde_json::from_str::<Value>(&json(&Diff::default())).unwrap_or_default(),
            serde_json::json!({ "addedEdges": [], "removedEdges": [], "newViolations": [], "resolvedViolations": [], "ratchets": [] })
        );
    }

    #[test]
    fn ids_are_the_results_own_else_computed_with_the_edge_kind() {
        let mut document = old();
        document.modules[0].dependencies[0].dependency_kind = Some(DependencyKind::Import);
        let kinds = kinds(&document);
        let plain = violation("no-db", Severity::Error, "r/a.ts", "db/s.ts");
        assert_eq!(
            id_of(&plain, &kinds),
            violation_id("no-db", "r/a.ts", "db/s.ts", "import")
        );
        let mut module_rule = plain.clone();
        module_rule.violation_type = Some(ViolationType::Module);
        assert_eq!(
            id_of(&module_rule, &kinds),
            violation_id("no-db", "r/a.ts", "db/s.ts", "")
        );
        let mut untyped = plain.clone();
        untyped.violation_type = None;
        assert_eq!(
            id_of(&untyped, &kinds),
            violation_id("no-db", "r/a.ts", "db/s.ts", "")
        );
        for kind in [ViolationType::Cycle, ViolationType::Instability] {
            let mut edge = plain.clone();
            edge.violation_type = Some(kind);
            assert_eq!(
                id_of(&edge, &kinds),
                violation_id("no-db", "r/a.ts", "db/s.ts", "import")
            );
        }
        let mut own = plain.clone();
        own.id = Some("RB-00000000".into());
        assert_eq!(id_of(&own, &kinds), "RB-00000000");
        own.id = Some(String::new());
        assert_eq!(
            id_of(&own, &kinds),
            violation_id("no-db", "r/a.ts", "db/s.ts", "import")
        );
    }

    #[test]
    fn the_last_dependency_of_a_pair_gives_the_kind_and_the_first_the_position() {
        let mut document = GraphDocument {
            modules: vec![module("a", &[("b", 5), ("b", 2)])],
            ..GraphDocument::default()
        };
        document.modules[0].dependencies[0].dependency_kind = Some(DependencyKind::Import);
        document.modules[0].dependencies[1].dependency_kind = Some(DependencyKind::Call);
        assert_eq!(kinds(&document).get(&("a", "b")), Some(&"call"));
        assert_eq!(
            edges(&document).get(&("a".to_owned(), "b".to_owned())),
            Some(&(Some(5), Some(1)))
        );
    }

    #[test]
    fn a_fix_comes_from_the_violation_else_the_rule_set() {
        let mut document = old();
        document.summary.rule_set_used = Some(
            serde_json::json!({
                "forbidden": [{ "name": "other" }, { "name": "no-db", "fix": "Use the service" }],
                "required": [{ "name": "req", "fix": "Import the shell" }]
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
        );
        let v = violation("no-db", Severity::Error, "r/a.ts", "db/s.ts");
        assert_eq!(fix_of(&v, &document).as_deref(), Some("Use the service"));
        let r = violation("req", Severity::Error, "r/a.ts", "r/a.ts");
        assert_eq!(fix_of(&r, &document).as_deref(), Some("Import the shell"));
        let unknown = violation("none", Severity::Error, "r/a.ts", "db/s.ts");
        assert_eq!(fix_of(&unknown, &document), None);
        let mut own = v.clone();
        own.fix = Some("Own".into());
        assert_eq!(fix_of(&own, &document).as_deref(), Some("Own"));
        assert_eq!(fix_of(&v, &old()), None);
    }

    #[test]
    fn the_sides_come_from_the_receipts_and_an_unknown_side_is_left_out() {
        let mut document = old();
        assert!(Side::of(&document).is_unknown());
        document.revision_data =
            serde_json::from_value(serde_json::json!({ "SHA1": "0123abc", "changes": [] })).ok();
        assert_eq!(
            Side::of(&document),
            Side {
                revision: None,
                sha: Some("0123abc".into())
            }
        );
        let diff = compute(&document, &new());
        assert_eq!(diff.base.and_then(|b| b.sha).as_deref(), Some("0123abc"));
        assert!(diff.head.is_none());
        let empty_sha: GraphDocument = serde_json::from_value(serde_json::json!({
            "modules": [], "summary": { "violations": [], "error": 0, "warn": 0, "info": 0,
            "totalCruised": 0, "optionsUsed": {} }, "revisionData": { "SHA1": "", "changes": [] }
        }))
        .unwrap_or_default();
        assert!(Side::of(&empty_sha).is_unknown());
        let revision_only = Side {
            revision: Some("main".into()),
            sha: None,
        };
        assert!(!revision_only.is_unknown());
    }

    #[test]
    fn markdown_has_a_table_per_section_and_one_line_for_an_empty_one() {
        let diff = compute(&old(), &new()).with_sides(
            Side {
                revision: Some("main".into()),
                sha: Some("abc".into()),
            },
            Side {
                revision: None,
                sha: Some("def".into()),
            },
        );
        let text = markdown(&diff);
        assert!(text.starts_with("## Architecture diff\n\nBase `main` at `abc`, head `def`.\n"));
        assert!(text.contains(
            "1 new violation, 1 resolved; 1 edge added, 1 removed; 2 ratchets changed.\n"
        ));
        assert!(text.contains("| `RB-"));
        assert!(text.contains(
            "| `no-web` | warn | `r/b.ts:3:1` | `web/v.ts` | Return data \\| not views |\n"
        ));
        assert!(text.contains("| `fresh` | absent | 0 |\n| `via-service` | 2 | 1 |\n"));
        assert!(text.contains("| `r/b.ts:2:1` | `db/s.ts` |\n"));
        let empty = markdown(&Diff::default());
        for line in [
            "No new violations.",
            "No violations resolved.",
            "No ratchet counts changed.",
            "No edges added.",
            "No edges removed.",
        ] {
            assert!(empty.contains(line), "{line}");
        }
        assert!(empty.contains(
            "0 new violations, 0 resolved; 0 edges added, 0 removed; 0 ratchets changed."
        ));
        assert!(!empty.contains("Base"));
        let base_only = markdown(&Diff::default().with_sides(
            Side {
                revision: Some("main".into()),
                sha: None,
            },
            Side::default(),
        ));
        assert!(base_only.contains("\nBase `main`.\n"));
        let head_only = markdown(&Diff::default().with_sides(
            Side::default(),
            Side {
                revision: None,
                sha: Some("f00".into()),
            },
        ));
        assert!(head_only.contains("\nHead `f00`.\n"));
    }

    #[test]
    fn agent_prints_one_line_per_new_violation_then_the_counts() {
        let diff = compute(&old(), &new());
        let id = violation_id("no-web", "r/b.ts", "web/v.ts", "");
        assert_eq!(
            agent(&diff),
            format!(
                "new {id} warn no-web: r/b.ts:3:1 -> web/v.ts. Fix: Return data | not views\ndiff: 1 new violation, 1 resolved; 1 edge added, 1 removed; 2 ratchets changed\n"
            )
        );
        let mut without_fix = diff.clone();
        without_fix.new_violations[0].fix = None;
        without_fix.new_violations[0].line = None;
        assert!(
            agent(&without_fix).starts_with(&format!("new {id} warn no-web: r/b.ts -> web/v.ts\n"))
        );
        assert_eq!(
            agent(&Diff::default()),
            "diff: 0 new violations, 0 resolved; 0 edges added, 0 removed; 0 ratchets changed\n"
        );
    }

    #[test]
    fn render_dispatches_and_names_an_unknown_type() {
        let diff = compute(&old(), &new());
        assert_eq!(render("json", &diff), Ok(json(&diff)));
        assert_eq!(render("markdown", &diff), Ok(markdown(&diff)));
        assert_eq!(render("agent", &diff), Ok(agent(&diff)));
        let err = render("err", &diff);
        assert_eq!(err, Err(DiffError::OutputType("err".into())));
        assert!(
            err.err()
                .is_some_and(|e| e.to_string().contains("use json, markdown or agent"))
        );
        for t in DIFF_OUTPUT_TYPES {
            assert!(render(t, &diff).is_ok());
        }
    }

    #[test]
    fn positions_and_code_spans() {
        assert_eq!(position("a", Some(1), Some(2)), "a:1:2");
        assert_eq!(position("a", Some(1), None), "a:1");
        assert_eq!(position("a", None, Some(2)), "a");
        assert_eq!(position("a", None, None), "a");
        assert_eq!(code("a|b"), "`a\\|b`");
        assert_eq!(code("a`b"), "``a`b``");
        assert_eq!(code("`a"), "`` `a ``");
        assert_eq!(code("a``"), "``` a`` ```");
        assert_eq!(cell("one\n two | three"), "one two \\| three");
    }

    #[test]
    fn new_errors_counts_error_severity_only() {
        let mut diff = compute(&old(), &new());
        assert_eq!(diff.new_errors(), 0);
        diff.new_violations[0].severity = Severity::Error;
        assert_eq!(diff.new_errors(), 1);
        diff.new_violations.push(diff.new_violations[0].clone());
        assert_eq!(diff.new_errors(), 2);
    }

    fn arbitrary_document() -> impl Strategy<Value = GraphDocument> {
        let names = prop::sample::select(vec!["a", "b", "c", "d"]);
        let edge = (names.clone(), names.clone(), 1u32..5);
        let finding = (
            prop::sample::select(vec!["r1", "r2"]),
            prop::sample::select(vec![Severity::Error, Severity::Warn, Severity::Ignore]),
            names.clone(),
            names,
        );
        (
            prop::collection::vec(edge, 0..8),
            prop::collection::vec(finding, 0..6),
            prop::collection::vec((prop::sample::select(vec!["x", "y"]), 0u64..4), 0..3),
        )
            .prop_map(|(edges, findings, ratchets)| {
                let mut modules: BTreeMap<&str, Module> = BTreeMap::new();
                for (from, to, line) in edges {
                    let entry = modules.entry(from).or_insert_with(|| Module::new(from));
                    let mut d = Dependency::new(to, to, ModuleSystem::Es6);
                    d.line = Some(line);
                    entry.dependencies.push(d);
                }
                let mut document = GraphDocument {
                    modules: modules.into_values().collect(),
                    ..GraphDocument::default()
                };
                document.summary.violations = findings
                    .into_iter()
                    .map(|(r, s, f, t)| violation(r, s, f, t))
                    .collect();
                let mut seen = std::collections::BTreeSet::new();
                document.summary.ratchets = Some(
                    ratchets
                        .into_iter()
                        .filter(|(n, _)| seen.insert(*n))
                        .map(|(n, c)| ratchet(n, c))
                        .collect(),
                );
                document
            })
    }

    proptest! {
        #[test]
        fn a_result_against_itself_is_empty(a in arbitrary_document()) {
            prop_assert!(compute(&a, &a).is_empty());
        }

        #[test]
        fn swapping_the_sides_swaps_added_and_removed(a in arbitrary_document(), b in arbitrary_document()) {
            let forward = compute(&a, &b);
            let backward = compute(&b, &a);
            let strip = |edges: &[Edge]| -> Vec<(String, String)> {
                edges.iter().map(|e| (e.from.clone(), e.to.clone())).collect()
            };
            prop_assert_eq!(strip(&forward.added_edges), strip(&backward.removed_edges));
            prop_assert_eq!(strip(&forward.removed_edges), strip(&backward.added_edges));
            prop_assert_eq!(&forward.new_violations, &backward.resolved_violations);
            prop_assert_eq!(&forward.resolved_violations, &backward.new_violations);
            let flipped: Vec<RatchetChange> = forward.ratchets.iter().map(|r| RatchetChange {
                name: r.name.clone(), before: r.after, after: r.before }).collect();
            prop_assert_eq!(flipped, backward.ratchets);
        }

        #[test]
        fn every_list_is_sorted_and_rendering_is_deterministic(a in arbitrary_document(), b in arbitrary_document()) {
            let diff = compute(&a, &b);
            let keys: Vec<(String, String)> = diff.added_edges.iter().map(|e| (e.from.clone(), e.to.clone())).collect();
            let mut sorted = keys.clone();
            sorted.sort();
            prop_assert_eq!(keys, sorted);
            let order: Vec<_> = diff.new_violations.iter().map(|f| (f.rule.clone(), f.from.clone(), f.to.clone(), f.id.clone())).collect();
            let mut sorted = order.clone();
            sorted.sort();
            prop_assert_eq!(order, sorted);
            prop_assert!(diff.new_violations.iter().chain(&diff.resolved_violations).all(|f| f.severity != Severity::Ignore));
            for t in DIFF_OUTPUT_TYPES {
                prop_assert_eq!(render(t, &diff), render(t, &compute(&a, &b)));
            }
        }
    }
}
