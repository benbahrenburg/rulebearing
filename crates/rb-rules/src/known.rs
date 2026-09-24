//! Known violations: dependency-cruiser's `softenKnownViolations`, the id-keyed entries
//! Rulebearing writes, expiry, and which entries still occur.
//!
//! - Contract: [Wave 2 plan § 1.5](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#15-interfaces-and-contracts-this-wave-freezes)
//!   (the `knownViolations[]` entry)
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Decisions: [ADR-0015](../../../docs/adr/0015-stable-violation-id.md) (the id is the baseline
//!   key), [ADR-0031](../../../docs/adr/0031-a-saved-result-carries-what-the-exit-code-counts.md)
//!   (an expired entry is reported in `summary.expired[]`)
//! - Requirement: [FR-RULE-09](../../../docs/prd.md#fr-rule-09)
//! - Source: [design § import-linter contracts](../../../docs/artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them)
//!   (`shrink-only` replaces import-linter's unmatched-ignore alerting)
//!
//! An entry matches a violation when its `id` equals the violation's, or, for an entry without
//! one, the way dependency-cruiser matches it: a `module` or `reachability` entry on `from` and
//! the rule name, a `dependency`, `cycle` or `instability` entry by `isSameViolation`. An
//! `element` or `slice` entry (a Rulebearing type) matches on the rule name and whichever of
//! `from` and `to` it gives, so an entry naming only the object (`to`) or only the slice (`from`)
//! covers every violation of it, as `ArchUnitNET`'s violation store is keyed by the evaluated
//! object. A matched violation is softened to `ignore`. An entry past its `expires` date is not
//! honoured and is reported as expired; whether or not it is honoured, every entry records
//! whether some violation still matches it, which is what `baseline --baseline-mode shrink-only`
//! reads.

use chrono::NaiveDate;
use rb_config::model::KnownViolation;
use std::collections::BTreeMap;

use rb_model::violation_id::violation_id;
use rb_model::{GraphDocument, Summary};
use serde_json::{Value, json};

use crate::evaluate::Expired;
use crate::js;
use crate::summarize::{is_same_violation, violation_stats};

/// One entry, as the matching reads it.
struct Entry {
    shape: Value,
    id: Option<String>,
    honoured: bool,
}

/// The known violations of a run, and which of them some violation matched.
pub struct KnownSet {
    entries: Vec<Entry>,
    matched: Vec<bool>,
}

fn kind_of(value: &Value) -> Option<&str> {
    js::str_of(value, "type")
}

fn rule_name(value: &Value) -> Option<&str> {
    value.get("rule").and_then(|r| js::str_of(r, "name"))
}

/// The name an expired entry is reported under: its id, else `from -> to`.
fn display_name(entry: &KnownViolation) -> String {
    entry.id.clone().unwrap_or_else(|| {
        format!(
            "{} -> {}",
            entry.from.as_deref().unwrap_or("?"),
            entry.to.as_deref().unwrap_or("?")
        )
    })
}

/// Whether an `element` or `slice` entry without an id covers `violation`: same type, same rule,
/// and every one of `from` and `to` the entry gives is equal. An entry that gives neither covers
/// nothing, so a stray entry never silences a whole rule.
fn object_entry_matches(shape: &Value, violation: &Value) -> bool {
    let (from, to) = (shape.get("from"), shape.get("to"));
    if from.is_none() && to.is_none() {
        return false;
    }
    kind_of(shape) == kind_of(violation)
        && rule_name(shape).is_some()
        && rule_name(shape) == rule_name(violation)
        && from.is_none_or(|f| violation.get("from") == Some(f))
        && to.is_none_or(|t| violation.get("to") == Some(t))
}

/// Whether an entry without an id covers a violation of `summary.violations[]`, by the entry's
/// type.
fn shape_matches(shape: &Value, violation: &Value) -> bool {
    let edge = |kind: Option<&str>| matches!(kind, Some("dependency" | "cycle" | "instability"));
    let module = |kind: Option<&str>| matches!(kind, Some("module" | "reachability"));
    match kind_of(shape) {
        Some("element" | "slice") => object_entry_matches(shape, violation),
        kind if module(kind) => {
            module(kind_of(violation))
                && js::str_of(shape, "from").is_some()
                && js::str_of(shape, "from") == js::str_of(violation, "from")
                && rule_name(shape).is_some()
                && rule_name(shape) == rule_name(violation)
        }
        kind if edge(kind) => edge(kind_of(violation)) && is_same_violation(shape, violation),
        _ => false,
    }
}

impl KnownSet {
    /// The entries of `entries`, with those past their date on `today` not honoured and pushed
    /// onto `past`. An entry expiring today still applies today.
    pub fn new(entries: &[KnownViolation], today: NaiveDate, past: &mut Vec<Expired>) -> Self {
        let mut out = Vec::with_capacity(entries.len());
        for entry in entries {
            let honoured = match entry.expires {
                Some(expires) if today > expires => {
                    past.push(Expired {
                        name: display_name(entry),
                        expires,
                        kind: "knownViolation".into(),
                    });
                    false
                }
                _ => true,
            };
            out.push(Entry {
                shape: serde_json::to_value(entry).unwrap_or(Value::Null),
                id: entry.id.clone(),
                honoured,
            });
        }
        let matched = vec![false; out.len()];
        Self {
            entries: out,
            matched,
        }
    }

    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Marks every entry `test` accepts as matched; true when one of them is honoured, so the
    /// violation is softened.
    fn hit(&mut self, test: impl Fn(&Entry) -> bool) -> bool {
        let mut soften = false;
        for (entry, matched) in self.entries.iter().zip(self.matched.iter_mut()) {
            if test(entry) {
                *matched = true;
                soften |= entry.honoured;
            }
        }
        soften
    }

    /// The indices of the entries no violation matched, in the order given.
    pub fn unmatched(&self) -> Vec<usize> {
        self.matched
            .iter()
            .enumerate()
            .filter(|(_, m)| !**m)
            .map(|(i, _)| i)
            .collect()
    }

    /// `softenKnownViolations` over the annotated modules: each module and dependency rule a
    /// known entry matches becomes `ignore`.
    pub fn soften_modules(&mut self, modules: &mut [Value]) {
        if self.entries.is_empty() {
            return;
        }
        for module in modules.iter_mut() {
            let source = js::text(module, "source").into_owned();
            if module.get("valid") == Some(&Value::Bool(false))
                && let Some(Value::Array(rules)) = module.get_mut("rules")
            {
                for rule in rules.iter_mut() {
                    let name = js::text(rule, "name").into_owned();
                    let id = violation_id(&name, &source, &source, "");
                    let key = json!({ "type": "module", "rule": { "name": name }, "from": source });
                    if self.hit(|k| {
                        k.id.as_deref() == Some(id.as_str())
                            || (matches!(kind_of(&k.shape), Some("module" | "reachability"))
                                && shape_matches(&k.shape, &key))
                    }) {
                        js::set(rule, "severity", json!("ignore"));
                    }
                }
            }
            if let Some(Value::Array(dependencies)) = module.get_mut("dependencies") {
                for dependency in dependencies.iter_mut() {
                    self.soften_dependency(&source, dependency);
                }
            }
        }
    }

    fn soften_dependency(&mut self, source: &str, dependency: &mut Value) {
        if dependency.get("valid") != Some(&Value::Bool(false)) {
            return;
        }
        let to = js::text(dependency, "resolved").into_owned();
        let kind = js::str_of(dependency, "dependencyKind")
            .unwrap_or("")
            .to_owned();
        let cycle = dependency.get("cycle").cloned();
        if let Some(Value::Array(rules)) = dependency.get_mut("rules") {
            for rule in rules.iter_mut() {
                let name = js::text(rule, "name").into_owned();
                let id = violation_id(&name, source, &to, &kind);
                let mut key =
                    json!({ "type": "dependency", "rule": rule.clone(), "from": source, "to": to });
                if let Some(cycle) = &cycle {
                    js::set(&mut key, "cycle", cycle.clone());
                }
                if self.hit(|k| {
                    k.id.as_deref() == Some(id.as_str())
                        || (matches!(
                            kind_of(&k.shape),
                            Some("dependency" | "cycle" | "instability")
                        ) && is_same_violation(&k.shape, &key))
                }) {
                    js::set(rule, "severity", json!("ignore"));
                }
            }
        }
    }

    /// Softens each violation of `summary.violations[]` an entry matches: by its `id`, or by the
    /// entry's shape, as the module and dependency rules are matched.
    pub fn soften_violations(&mut self, violations: &mut [Value]) {
        if self.entries.is_empty() {
            return;
        }
        for violation in violations.iter_mut() {
            let id = js::str_of(violation, "id").map(str::to_owned);
            let snapshot = violation.clone();
            if self.hit(|k| (k.id.is_some() && k.id == id) || shape_matches(&k.shape, &snapshot))
                && let Some(rule) = violation.get_mut("rule")
            {
                js::set(rule, "severity", json!("ignore"));
            }
        }
    }
}

/// What [`apply_to_document`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The entries past their date, which the saved result now records.
    pub expired: Vec<Expired>,
    /// The indices of the entries no violation matched.
    pub unmatched: Vec<usize>,
}

/// Recounts `summary.error`, `warn`, `info` and `ignore` from its violations.
fn recount(summary: &mut Summary, violations: &[Value]) {
    let stats = violation_stats(violations);
    let count = |k: &str| stats.get(k).and_then(Value::as_u64).unwrap_or(0);
    summary.error = count("error");
    summary.warn = count("warn");
    summary.info = count("info");
    summary.ignore = Some(count("ignore"));
}

fn to_values<T: serde::Serialize>(items: &[T]) -> Result<Vec<Value>, serde_json::Error> {
    items.iter().map(serde_json::to_value).collect()
}

fn from_values<T: serde::de::DeserializeOwned>(
    items: Vec<Value>,
) -> Result<Vec<T>, serde_json::Error> {
    items.into_iter().map(serde_json::from_value).collect()
}

/// `fmt --ignore-known`: the known violations applied to a saved result as `cruise` applies
/// them: the matched module rules, dependency rules and violations softened to `ignore`, the
/// counts redone, and each expired entry added to `summary.expired[]`
/// ([ADR-0031](../../../docs/adr/0031-a-saved-result-carries-what-the-exit-code-counts.md)).
///
/// # Errors
/// A [`serde_json::Error`] when the document does not round-trip, which would be a bug.
pub fn apply_to_document(
    document: &mut GraphDocument,
    entries: &[KnownViolation],
    today: NaiveDate,
) -> Result<Applied, serde_json::Error> {
    let mut expired = Vec::new();
    let mut known = KnownSet::new(entries, today, &mut expired);
    let mut modules = to_values(&document.modules)?;
    known.soften_modules(&mut modules);
    document.modules = from_values(modules)?;
    let mut violations = to_values(&document.summary.violations)?;
    known.soften_violations(&mut violations);
    recount(&mut document.summary, &violations);
    document.summary.violations = from_values(violations)?;
    if !expired.is_empty() {
        let list = document.summary.expired.get_or_insert_with(Vec::new);
        for entry in expired.iter().map(Expired::entry) {
            if !list.contains(&entry) {
                list.push(entry);
            }
        }
    }
    Ok(Applied {
        expired,
        unmatched: known.unmatched(),
    })
}

/// Each rule's severity as `summary.ruleSetUsed` records it: the dependency rules by name, the
/// `allowed` rules as `not-in-allowed` at `allowedSeverity` (dependency-cruiser's default
/// `warn`), and the element, slice and diagram rules.
fn severities(summary: &Summary) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(rules) = &summary.rule_set_used else {
        return out;
    };
    for list in ["forbidden", "required", "elements", "slices", "diagrams"] {
        for rule in rules
            .get(list)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let (Some(name), Some(severity)) =
                (js::str_of(rule, "name"), js::str_of(rule, "severity"))
            {
                out.entry(name.to_owned())
                    .or_insert_with(|| severity.to_owned());
            }
        }
    }
    if rules.get("allowed").is_some() {
        let severity = rules
            .get("allowedSeverity")
            .and_then(Value::as_str)
            .unwrap_or("warn");
        out.insert("not-in-allowed".into(), severity.to_owned());
    }
    out
}

/// Puts a softened rule back at its configured severity.
fn restore(rule: &mut Value, severities: &BTreeMap<String, String>) {
    if js::str_of(rule, "severity") != Some("ignore") {
        return;
    }
    if let Some(severity) = js::str_of(rule, "name").and_then(|n| severities.get(n)) {
        let severity = severity.clone();
        js::set(rule, "severity", json!(severity));
    }
}

/// `fmt --no-ignore-known`: a saved result re-reported without its baseline. Every finding the
/// known violations softened goes back to the severity its rule has in `summary.ruleSetUsed`,
/// the counts are redone, and the expired known-violation entries leave `summary.expired[]`,
/// since no entry applies. A rule configured at `ignore` stays `ignore`.
///
/// # Errors
/// A [`serde_json::Error`] when the document does not round-trip, which would be a bug.
pub fn restore_severities(document: &mut GraphDocument) -> Result<(), serde_json::Error> {
    let severities = severities(&document.summary);
    let mut modules = to_values(&document.modules)?;
    for module in &mut modules {
        if let Some(Value::Array(rules)) = module.get_mut("rules") {
            for rule in rules {
                restore(rule, &severities);
            }
        }
        if let Some(Value::Array(dependencies)) = module.get_mut("dependencies") {
            for dependency in dependencies {
                if let Some(Value::Array(rules)) = dependency.get_mut("rules") {
                    for rule in rules {
                        restore(rule, &severities);
                    }
                }
            }
        }
    }
    document.modules = from_values(modules)?;
    let mut violations = to_values(&document.summary.violations)?;
    for violation in &mut violations {
        if let Some(rule) = violation.get_mut("rule") {
            restore(rule, &severities);
        }
    }
    recount(&mut document.summary, &violations);
    document.summary.violations = from_values(violations)?;
    if let Some(list) = &mut document.summary.expired {
        list.retain(|e| e.kind != "knownViolation");
        if list.is_empty() {
            document.summary.expired = None;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(value: Value) -> Vec<KnownViolation> {
        serde_json::from_value(value).unwrap_or_default()
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default()
    }

    fn violation(kind: &str, rule: &str, from: &str, to: &str) -> Value {
        json!({ "type": kind, "rule": { "name": rule, "severity": "error" }, "from": from, "to": to })
    }

    fn severities(violations: &[Value]) -> Vec<String> {
        violations
            .iter()
            .map(|v| js::text(&v["rule"], "severity").into_owned())
            .collect()
    }

    #[test]
    fn expiry_is_the_day_after_and_the_name_is_the_id_or_the_edge() {
        let list = entries(json!([
            { "id": "RB-00000001", "expires": "2026-09-21" },
            { "from": "a.ts", "to": "b.ts", "expires": "2026-09-21" },
            { "from": "c.ts", "expires": "2026-09-22" },
            { "to": "d.ts", "expires": "2026-09-20" }
        ]));
        let mut expired = Vec::new();
        let set = KnownSet::new(&list, day(2026, 9, 22), &mut expired);
        let names: Vec<&str> = expired.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["RB-00000001", "a.ts -> b.ts", "? -> d.ts"]);
        assert!(expired.iter().all(|e| e.kind == "knownViolation"));
        assert_eq!(expired[0].expires, day(2026, 9, 21));
        let honoured: Vec<bool> = set.entries.iter().map(|e| e.honoured).collect();
        assert_eq!(honoured, [false, false, true, false]);
        assert!(!set.is_empty());
        assert!(KnownSet::new(&[], day(2026, 1, 1), &mut Vec::new()).is_empty());
    }

    #[test]
    fn an_id_softens_its_violation_and_marks_the_entry() {
        let list = entries(json!([{ "id": "RB-1" }, { "id": "RB-2" }]));
        let mut set = KnownSet::new(&list, day(2026, 1, 1), &mut Vec::new());
        let mut found = vec![
            json!({ "id": "RB-1", "type": "element", "rule": { "name": "r", "severity": "error" }, "from": "f", "to": "o" }),
            json!({ "id": "RB-3", "type": "element", "rule": { "name": "r", "severity": "warn" }, "from": "f", "to": "p" }),
        ];
        set.soften_violations(&mut found);
        assert_eq!(severities(&found), ["ignore", "warn"]);
        assert_eq!(set.unmatched(), [1]);
    }

    #[test]
    fn an_expired_entry_is_matched_but_does_not_soften() {
        let list = entries(json!([{ "id": "RB-1", "expires": "2020-01-01" }]));
        let mut expired = Vec::new();
        let mut set = KnownSet::new(&list, day(2026, 1, 1), &mut expired);
        let mut found = vec![
            json!({ "id": "RB-1", "type": "element", "rule": { "name": "r", "severity": "error" } }),
        ];
        set.soften_violations(&mut found);
        assert_eq!(severities(&found), ["error"]);
        assert!(set.unmatched().is_empty(), "it still occurs");
        assert_eq!(expired.len(), 1);
    }

    #[test]
    fn element_and_slice_entries_match_on_what_they_name() {
        let list = entries(json!([
            { "type": "element", "rule": { "name": "sealed" }, "to": "A" },
            { "type": "slice", "rule": { "name": "apart" }, "from": "Slice1" },
            { "type": "element", "rule": { "name": "sealed" }, "from": "b.cs", "to": "B" },
            { "type": "element", "rule": { "name": "sealed" } },
            { "type": "slice", "rule": { "name": "sealed" }, "to": "A" },
            { "type": "element", "to": "C" }
        ]));
        let mut set = KnownSet::new(&list, day(2026, 1, 1), &mut Vec::new());
        let mut found = vec![
            violation("element", "sealed", "a.cs", "A"),
            violation("slice", "apart", "Slice1", "Slice2"),
            violation("slice", "apart", "Slice2", "Slice1"),
            violation("element", "sealed", "b.cs", "B"),
            violation("element", "sealed", "other.cs", "B"),
            violation("element", "sealed", "c.cs", "C"),
        ];
        set.soften_violations(&mut found);
        assert_eq!(
            severities(&found),
            ["ignore", "ignore", "error", "ignore", "error", "error"]
        );
        assert_eq!(set.unmatched(), [3, 4, 5]);
    }

    #[test]
    fn dependency_cruiser_entries_match_their_own_kind_of_violation() {
        let list = entries(json!([
            { "type": "module", "from": "lonely.ts", "rule": { "name": "no-orphans" } },
            { "type": "dependency", "from": "a.ts", "to": "b.ts", "rule": { "name": "no-b" } },
            { "type": "cycle", "from": "x.ts", "to": "y.ts", "rule": { "name": "no-circular" },
              "cycle": [{ "name": "y.ts" }, { "name": "x.ts" }] },
            { "type": "module", "from": "a.ts", "rule": { "name": "no-b" } },
            { "type": "module", "rule": { "name": "no-orphans" } },
            { "from": "a.ts", "to": "b.ts", "rule": { "name": "no-b" } }
        ]));
        let mut set = KnownSet::new(&list, day(2026, 1, 1), &mut Vec::new());
        let mut cycle = violation("cycle", "no-circular", "y.ts", "x.ts");
        cycle["cycle"] = json!([{ "name": "x.ts" }, { "name": "y.ts" }]);
        let mut found = vec![
            violation("module", "no-orphans", "lonely.ts", "lonely.ts"),
            violation("reachability", "no-orphans", "other.ts", "other.ts"),
            violation("dependency", "no-b", "a.ts", "b.ts"),
            cycle,
            violation("element", "no-b", "a.ts", "b.ts"),
        ];
        set.soften_violations(&mut found);
        assert_eq!(
            severities(&found),
            ["ignore", "error", "ignore", "ignore", "error"]
        );
        assert_eq!(set.unmatched(), [3, 4, 5]);
    }

    #[test]
    fn modules_soften_by_id_and_by_shape() {
        let lonely_id = violation_id("no-orphans", "lonely.ts", "lonely.ts", "");
        let edge_id = violation_id("no-b", "a.ts", "b.ts", "import");
        let list = entries(json!([
            { "id": lonely_id },
            { "id": edge_id },
            { "type": "dependency", "from": "a.ts", "to": "c.ts", "rule": { "name": "no-c" } },
            { "type": "reachability", "from": "a.ts", "rule": { "name": "reach" } },
            { "id": "RB-ffffffff" }
        ]));
        let mut set = KnownSet::new(&list, day(2026, 1, 1), &mut Vec::new());
        let rule = |name: &str| json!({ "name": name, "severity": "error" });
        let mut modules = vec![
            json!({ "source": "lonely.ts", "valid": false, "rules": [rule("no-orphans")], "dependencies": [] }),
            json!({ "source": "a.ts", "valid": false, "rules": [rule("reach"), rule("other")], "dependencies": [
                { "resolved": "b.ts", "dependencyKind": "import", "valid": false, "rules": [rule("no-b")] },
                { "resolved": "c.ts", "valid": false, "rules": [rule("no-c"), rule("no-d")] },
                { "resolved": "d.ts", "valid": true, "rules": [rule("no-b")] }
            ] }),
        ];
        set.soften_modules(&mut modules);
        let sev = |v: &Value| js::text(v, "severity").into_owned();
        assert_eq!(sev(&modules[0]["rules"][0]), "ignore");
        assert_eq!(sev(&modules[1]["rules"][0]), "ignore");
        assert_eq!(sev(&modules[1]["rules"][1]), "error");
        let deps = &modules[1]["dependencies"];
        assert_eq!(sev(&deps[0]["rules"][0]), "ignore");
        assert_eq!(sev(&deps[1]["rules"][0]), "ignore");
        assert_eq!(sev(&deps[1]["rules"][1]), "error");
        assert_eq!(sev(&deps[2]["rules"][0]), "error", "a valid edge is left");
        assert_eq!(set.unmatched(), [4]);
    }

    #[test]
    fn a_valid_module_keeps_its_rules_and_nothing_is_done_without_entries() {
        let rule = json!({ "name": "no-orphans", "severity": "error" });
        let mut modules = vec![json!({ "source": "a.ts", "valid": true, "rules": [rule] })];
        let list = entries(
            json!([{ "type": "module", "from": "a.ts", "rule": { "name": "no-orphans" } }]),
        );
        let mut set = KnownSet::new(&list, day(2026, 1, 1), &mut Vec::new());
        set.soften_modules(&mut modules);
        assert_eq!(modules[0]["rules"][0]["severity"], "error");
        assert_eq!(set.unmatched(), [0]);
        let mut none = KnownSet::new(&[], day(2026, 1, 1), &mut Vec::new());
        let mut found = vec![violation("element", "r", "f", "o")];
        none.soften_violations(&mut found);
        none.soften_modules(&mut modules);
        assert_eq!(severities(&found), ["error"]);
        assert!(none.unmatched().is_empty());
    }

    fn saved() -> GraphDocument {
        let rule = |name: &str, severity: &str| json!({ "name": name, "severity": severity });
        serde_json::from_value(json!({
            "modules": [
                { "source": "a.ts", "valid": true, "dependencies": [
                    { "module": "./b", "resolved": "b.ts", "coreModule": false, "followable": true,
                      "couldNotResolve": false, "dependencyTypes": ["local"], "moduleSystem": "es6",
                      "dynamic": false, "exoticallyRequired": false, "circular": false,
                      "valid": false, "rules": [rule("no-b", "error")] }
                ] },
                { "source": "b.ts", "valid": false, "rules": [rule("no-orphans", "warn")], "dependencies": [] }
            ],
            "summary": {
                "violations": [
                    { "type": "dependency", "from": "a.ts", "to": "b.ts", "rule": rule("no-b", "error"), "id": "RB-1" },
                    { "type": "module", "from": "b.ts", "to": "b.ts", "rule": rule("no-orphans", "warn") },
                    { "type": "element", "from": "c.cs", "to": "C", "rule": rule("sealed", "error"), "id": "RB-3" },
                    { "type": "dependency", "from": "x.ts", "to": "y.ts", "rule": rule("not-in-allowed", "error") }
                ],
                "error": 3, "warn": 1, "info": 0, "ignore": 0, "totalCruised": 2,
                "optionsUsed": {},
                "ruleSetUsed": {
                    "forbidden": [rule("no-b", "error"), rule("no-orphans", "warn")],
                    "allowed": [{ "from": {}, "to": {} }], "allowedSeverity": "error",
                    "elements": [rule("sealed", "error")]
                }
            }
        }))
        .unwrap_or_default()
    }

    #[test]
    fn a_saved_result_takes_a_baseline_and_gives_it_back() -> Result<(), serde_json::Error> {
        let mut document = saved();
        let list = entries(json!([
            { "id": "RB-1", "type": "dependency", "from": "a.ts", "to": "b.ts", "rule": { "name": "no-b" } },
            { "type": "module", "from": "b.ts", "rule": { "name": "no-orphans" } },
            { "type": "element", "rule": { "name": "sealed" }, "to": "C", "expires": "2020-01-01" },
            { "type": "dependency", "from": "x.ts", "to": "y.ts", "rule": { "name": "not-in-allowed" } },
            { "id": "RB-gone" }
        ]));
        let applied = apply_to_document(&mut document, &list, day(2026, 1, 1))?;
        assert_eq!(applied.unmatched, [4]);
        assert_eq!(applied.expired.len(), 1);
        let summary = &document.summary;
        assert_eq!(
            (summary.error, summary.warn, summary.info, summary.ignore),
            (1, 0, 0, Some(3))
        );
        assert_eq!(summary.expired.as_ref().map(Vec::len), Some(1));
        assert_eq!(
            document.modules[0].dependencies[0]
                .rules
                .as_ref()
                .map(|r| r[0].severity),
            Some(rb_model::Severity::Ignore),
            "the edge's rule in the modules is softened by the entry's shape"
        );
        assert_eq!(
            document.modules[1].rules.as_ref().map(|r| r[0].severity),
            Some(rb_model::Severity::Ignore)
        );
        // Applying the same expired entry twice records it once.
        apply_to_document(&mut document, &list, day(2026, 1, 1))?;
        assert_eq!(document.summary.expired.as_ref().map(Vec::len), Some(1));

        restore_severities(&mut document)?;
        let summary = &document.summary;
        assert_eq!(
            (summary.error, summary.warn, summary.info, summary.ignore),
            (3, 1, 0, Some(0))
        );
        let severities: Vec<rb_model::Severity> =
            summary.violations.iter().map(|v| v.rule.severity).collect();
        assert_eq!(
            severities,
            [
                rb_model::Severity::Error,
                rb_model::Severity::Warn,
                rb_model::Severity::Error,
                rb_model::Severity::Error
            ]
        );
        assert_eq!(
            summary.expired, None,
            "no entry applies, so none has expired"
        );
        assert_eq!(
            document.modules[0].dependencies[0]
                .rules
                .as_ref()
                .map(|r| r[0].severity),
            Some(rb_model::Severity::Error)
        );
        assert_eq!(
            document.modules[1].rules.as_ref().map(|r| r[0].severity),
            Some(rb_model::Severity::Warn)
        );
        Ok(())
    }

    #[test]
    fn restoring_keeps_other_expiries_and_rules_it_cannot_place() -> Result<(), serde_json::Error> {
        let mut document = saved();
        document.summary.rule_set_used = None;
        document.summary.expired = Some(vec![
            rb_model::ExpiredEntry {
                name: "temp".into(),
                expires: "2020-01-01".into(),
                kind: "rule".into(),
            },
            rb_model::ExpiredEntry {
                name: "RB-9".into(),
                expires: "2020-01-01".into(),
                kind: "knownViolation".into(),
            },
        ]);
        document.summary.violations[0].rule.severity = rb_model::Severity::Ignore;
        restore_severities(&mut document)?;
        assert_eq!(
            document.summary.violations[0].rule.severity,
            rb_model::Severity::Ignore,
            "without ruleSetUsed the severity is unknown and stays"
        );
        assert_eq!(document.summary.ignore, Some(1));
        assert_eq!(
            document
                .summary
                .expired
                .as_ref()
                .map(|e| e.iter().map(|x| x.name.clone()).collect::<Vec<_>>()),
            Some(vec!["temp".to_owned()])
        );
        let mut allowed = saved();
        if let Some(rules) = allowed.summary.rule_set_used.as_mut() {
            rules.remove("allowedSeverity");
        }
        allowed.summary.violations[3].rule.severity = rb_model::Severity::Ignore;
        restore_severities(&mut allowed)?;
        assert_eq!(
            allowed.summary.violations[3].rule.severity,
            rb_model::Severity::Warn,
            "allowedSeverity defaults to warn"
        );
        Ok(())
    }
}
