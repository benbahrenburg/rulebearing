//! `baseline`: every violation as a `knownViolations[]` entry, the file `depcruise-baseline`
//! writes, keyed by the stable id and carrying `expires`, `owner` and `reason` when they are given.
//!
//! - Contract: [Wave 2 plan § 1.5](../../../docs/plans/implemented/0002-wave-2-dotnet-python-element-rules.md#15-interfaces-and-contracts-this-wave-freezes)
//!   (the `knownViolations[]` entry)
//! - Plan: [Wave 2, Step 10](../../../docs/plans/implemented/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Coverage: [dc coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types)
//!   (`baseline`, Parity+)
//! - Decision: [ADR-0015](../../../docs/adr/0015-stable-violation-id.md) (the id is the key)
//! - Requirement: [FR-RULE-09](../../../docs/prd.md#fr-rule-09)
//! - Specification: dependency-cruiser's `test/report/baseline`, run by gate 1 layer 3
//!
//! dependency-cruiser's reporter prints `summary.violations` as JSON with two-space indentation
//! and a newline, and exits 0; so does this one, byte for byte, when no lifecycle field is given.
//! Each violation of a Rulebearing result already carries its `id`. An entry already in the
//! previous baseline (same `id`) keeps its `expires`, `owner` and `reason`, and the command
//! line's values fill the ones an entry lacks. Keys are not sorted: as upstream's `JSON.stringify`,
//! each entry keeps the order its violation carries them (the document's fixed field order), with
//! the lifecycle fields after, so two runs over the same (sorted) violations are byte-identical.
//! An entry for a violation on a declared edge
//! ([ADR-0078](../../../docs/adr/0078-fleet-and-declared-edges.md)) carries `declared: true` and
//! `declaredKind` before them (`crate::declared`); `knownViolations` keeps keys it does not
//! read, and matches an entry by its rule and ends as before.

use rb_rules::compare::{
    compare_violations, compare_violations_ex_severities, diff_violation_arrays,
};
use rb_rules::known::unmatched_as_written;
use serde_json::Value;

use crate::Rendered;

/// The lifecycle fields `rulebearing baseline --expires --owner --reason` gives each entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lifecycle {
    /// The last day the entries apply, `YYYY-MM-DD`.
    pub expires: Option<String>,
    /// Who answers for them.
    pub owner: Option<String>,
    /// Why they stand.
    pub reason: Option<String>,
}

/// The keys [`Lifecycle`] fills.
pub const LIFECYCLE_KEYS: [&str; 3] = ["expires", "owner", "reason"];

impl Lifecycle {
    fn get(&self, key: &str) -> Option<&String> {
        match key {
            "expires" => self.expires.as_ref(),
            "owner" => self.owner.as_ref(),
            _ => self.reason.as_ref(),
        }
    }
}

/// The entries: each violation of `summary.violations`, with the lifecycle fields of the entry
/// with the same `id` in `previous`, then those of `lifecycle` for the fields still missing.
pub fn entries(result: &Value, lifecycle: &Lifecycle, previous: &[Value]) -> Vec<Value> {
    let violations = result
        .get("summary")
        .and_then(|s| s.get("violations"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let edges = crate::Edges::of(result);
    violations
        .into_iter()
        .map(|mut entry| {
            let declared = crate::declared::on_one_edge(&entry)
                .then(|| edges.declared(&crate::text(&entry, "from"), &crate::text(&entry, "to")))
                .flatten();
            let id = entry.get("id").and_then(Value::as_str).map(str::to_owned);
            let earlier = id.as_deref().and_then(|id| {
                previous
                    .iter()
                    .find(|p| p.get("id").and_then(Value::as_str) == Some(id))
            });
            if let Value::Object(map) = &mut entry {
                if let Some(kinds) = declared {
                    map.insert("declared".into(), Value::Bool(true));
                    if !kinds.is_empty() {
                        map.insert("declaredKind".into(), Value::String(kinds.join(", ")));
                    }
                }
                for key in LIFECYCLE_KEYS {
                    let value = earlier
                        .and_then(|e| e.get(key))
                        .filter(|v| !v.is_null())
                        .cloned()
                        .or_else(|| lifecycle.get(key).map(|v| Value::String(v.clone())));
                    if let Some(value) = value {
                        map.insert(key.to_owned(), value);
                    }
                }
            }
            entry
        })
        .collect()
}

/// The entries as the file holds them: `JSON.stringify(entries, null, 2)` and a newline.
pub fn text(entries: &[Value]) -> String {
    let mut output = serde_json::to_string_pretty(entries).unwrap_or_else(|_| "[]".into());
    output.push('\n');
    output
}

/// The entries of `known` by whether a violation of `current` still matches them, in the order
/// given: (still matched, stale). The matching is the engine's, so these lists and
/// `summary.baselineStale` always agree; a list the engine cannot read matches nothing.
pub fn partition(known: &[Value], current: &[Value]) -> (Vec<Value>, Vec<Value>) {
    let stale = unmatched_as_written(known, current);
    let (old, same): (Vec<_>, Vec<_>) = known
        .iter()
        .enumerate()
        .partition(|(at, _)| stale.contains(at));
    let values = |list: Vec<(usize, &Value)>| list.into_iter().map(|(_, v)| v.clone()).collect();
    (values(same), values(old))
}

/// `diffViolationArrays(knownViolations, violations).old`: the entries of
/// `summary.optionsUsed.knownViolations` no current violation matches, in upstream's order, or
/// `None` on a result without that list. `err-html` and `markdown` print these.
pub(crate) fn stale_entries(summary: &Value) -> Option<Vec<Value>> {
    let known = summary
        .get("optionsUsed")?
        .get("knownViolations")?
        .as_array()?;
    let current = summary
        .get("violations")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let (_, mut stale) = partition(known, current);
    stale.sort_by(compare_violations_ex_severities);
    Some(stale)
}

/// Renders `baseline`, in the mode `summary.optionsUsed.baseline.mode` names (`full` when absent).
/// `shrink-only` holds the known entries a violation still matches and `format` every known entry,
/// each sorted as upstream's reporter sorts them; `full` holds the current violations. It exits 0,
/// as dependency-cruiser's does.
pub fn render(result: &Value, lifecycle: &Lifecycle) -> Rendered {
    let summary = result.get("summary").unwrap_or(&Value::Null);
    let options = summary.get("optionsUsed").unwrap_or(&Value::Null);
    let mode = options
        .get("baseline")
        .and_then(|b| b.get("mode"))
        .and_then(Value::as_str);
    let known = || {
        options
            .get("knownViolations")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
    };
    let current = || {
        summary
            .get("violations")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
    };
    let kept = match mode {
        Some("shrink-only") => Some(partition(known(), current()).0),
        Some("format") => Some(known().to_vec()),
        _ => None,
    };
    let output = kept.map_or_else(
        || text(&entries(result, lifecycle, &[])),
        |mut kept| {
            kept.sort_by(compare_violations);
            text(&kept)
        },
    );
    Rendered {
        output,
        exit_code: 0,
    }
}

/// The lines upstream's 18.7.0 `baseline` reporter returns as `meta` and its command line prints
/// on stderr after writing the file: how many violations the file now holds, and how many are
/// new, the same and stale against `summary.optionsUsed.knownViolations`, by
/// `diffViolationArrays`, with what the mode did to each
/// ([plan 0011 Step 6](../../../docs/plans/pending/0011-dependency-cruiser-18-7.md#23-steps-for-sub-wave-11c-the-command-line)).
/// The count the file holds is the length of [`render`]'s output, so the two always agree.
pub fn meta(result: &Value) -> String {
    fn list(value: Option<&Value>) -> &[Value] {
        value
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
    }
    let summary = result.get("summary").unwrap_or(&Value::Null);
    let options = summary.get("optionsUsed").unwrap_or(&Value::Null);
    let known = list(options.get("knownViolations"));
    let current = list(summary.get("violations"));
    let mode = options
        .get("baseline")
        .and_then(|b| b.get("mode"))
        .and_then(Value::as_str)
        .unwrap_or("full");
    let diff = diff_violation_arrays(known, current);
    let (emitted, total, added, removed) = match mode {
        "shrink-only" => (
            partition(known, current).0.len(),
            "",
            " (running in 'shrink-only' mode => not added to the baseline)",
            " (running in 'shrink-only' mode => removed from the baseline)",
        ),
        "format" => (
            known.len(),
            " (running in 'format' mode so no updates made)",
            "",
            "",
        ),
        _ => (
            current.len(),
            "",
            " (running in 'full' mode => added to the baseline)",
            " (running in 'full' mode => removed from the baseline)",
        ),
    };
    let with = |count: usize, addition: &str| {
        if count > 0 {
            format!("{count}{addition}")
        } else {
            count.to_string()
        }
    };
    format!(
        "\nbaseline  : {emitted} violations{total}\n\n  new     : {}\n  same    : {}\n  stale   : {}\n",
        with(diff.new.len(), added),
        diff.same.len(),
        with(diff.old.len(), removed)
    )
}

#[cfg(test)]
mod meta_tests {
    use super::*;
    use serde_json::json;

    /// Six violations, as upstream's `dc-result-with-violations.json` mock has.
    fn six() -> Vec<Value> {
        (0..6)
            .map(|i| {
                json!({ "type": "dependency", "from": format!("src/{i}.js"), "to": "src/z.js",
                        "rule": { "name": "no-z", "severity": "error" } })
            })
            .collect()
    }

    fn result(known: &[Value], mode: Option<&str>) -> Value {
        let mut options = json!({ "knownViolations": known });
        if let Some(mode) = mode {
            options["baseline"] = json!({ "mode": mode });
        }
        json!({ "summary": { "violations": six(), "optionsUsed": options } })
    }

    // Each case is one of test/report/baseline/baseline.spec.mjs at 18.7.0.
    #[test]
    fn the_lines_are_upstreams_in_every_mode() {
        let five = &six()[..5];
        let mut with_stale = six();
        let mut stale = six()[0].clone();
        stale["from"] = json!("stale");
        stale["to"] = json!("stale");
        with_stale.push(stale);
        let table = [
            (
                five,
                None,
                "\nbaseline  : 6 violations\n\n  new     : 1 (running in 'full' mode => added to the baseline)\n  same    : 5\n  stale   : 0\n",
            ),
            (
                five,
                Some("shrink-only"),
                "\nbaseline  : 5 violations\n\n  new     : 1 (running in 'shrink-only' mode => not added to the baseline)\n  same    : 5\n  stale   : 0\n",
            ),
            (
                five,
                Some("format"),
                "\nbaseline  : 5 violations (running in 'format' mode so no updates made)\n\n  new     : 1\n  same    : 5\n  stale   : 0\n",
            ),
            (
                &six()[..],
                None,
                "\nbaseline  : 6 violations\n\n  new     : 0\n  same    : 6\n  stale   : 0\n",
            ),
            (
                &with_stale[..],
                None,
                "\nbaseline  : 6 violations\n\n  new     : 0\n  same    : 6\n  stale   : 1 (running in 'full' mode => removed from the baseline)\n",
            ),
            (
                &with_stale[..],
                Some("shrink-only"),
                "\nbaseline  : 6 violations\n\n  new     : 0\n  same    : 6\n  stale   : 1 (running in 'shrink-only' mode => removed from the baseline)\n",
            ),
        ];
        for (known, mode, expected) in table {
            let result = result(known, mode);
            assert_eq!(meta(&result), expected, "{mode:?}");
            let held: Vec<Value> =
                serde_json::from_str(&render(&result, &Lifecycle::default()).output)
                    .unwrap_or_default();
            assert!(expected.contains(&format!("baseline  : {} violations", held.len())));
        }
    }

    #[test]
    fn a_result_without_known_violations_has_everything_new() {
        let result = json!({ "summary": { "violations": six() } });
        assert_eq!(
            meta(&result),
            "\nbaseline  : 6 violations\n\n  new     : 6 (running in 'full' mode => added to the baseline)\n  same    : 0\n  stale   : 0\n"
        );
        assert_eq!(
            meta(&json!({})),
            "\nbaseline  : 0 violations\n\n  new     : 0\n  same    : 0\n  stale   : 0\n"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn violation(from: &str, severity: &str) -> Value {
        json!({ "type": "dependency", "from": from, "to": "z.ts",
                "rule": { "name": "no-z", "severity": severity } })
    }

    fn with_mode(mode: &str) -> Value {
        json!({ "summary": {
            "violations": [violation("a.ts", "ignore"), violation("new.ts", "error")],
            "optionsUsed": {
                "baseline": { "mode": mode },
                "knownViolations": [violation("stale.ts", "error"), violation("a.ts", "error")],
            },
        } })
    }

    #[test]
    fn the_mode_decides_what_the_reporter_holds() {
        let from = |output: &str| -> Vec<String> {
            serde_json::from_str::<Vec<Value>>(output)
                .unwrap_or_default()
                .iter()
                .map(|v| crate::text(v, "from"))
                .collect()
        };
        let lifecycle = Lifecycle::default();
        assert_eq!(
            from(&render(&with_mode("full"), &lifecycle).output),
            ["a.ts", "new.ts"]
        );
        assert_eq!(
            from(&render(&with_mode("shrink-only"), &lifecycle).output),
            ["a.ts"]
        );
        assert_eq!(
            from(&render(&with_mode("format"), &lifecycle).output),
            ["a.ts", "stale.ts"]
        );
        for mode in ["full", "shrink-only", "format"] {
            let rendered = render(&with_mode(mode), &lifecycle);
            assert_eq!(rendered.exit_code, 0);
            assert_eq!(rendered, render(&with_mode(mode), &lifecycle), "{mode}");
        }
    }

    #[test]
    fn partition_keeps_the_order_and_ignores_severity() {
        let known = [violation("b.ts", "error"), violation("a.ts", "warn")];
        let current = [violation("c.ts", "error"), violation("a.ts", "ignore")];
        let (same, old) = partition(&known, &current);
        assert_eq!(same, [violation("a.ts", "warn")]);
        assert_eq!(old, [violation("b.ts", "error")]);
        assert_eq!(partition(&[], &[]), (Vec::new(), Vec::new()));
    }

    #[test]
    fn an_entry_with_an_id_matches_on_the_id() {
        let entry = json!({ "id": "RB-1", "rule": { "name": "no-z" } });
        let mut current = violation("a.ts", "error");
        current["id"] = json!("RB-1");
        let other = json!({ "id": "RB-2", "rule": { "name": "no-z" } });
        let (same, old) = partition(&[entry.clone(), other.clone()], &[current]);
        assert_eq!(same, [entry]);
        assert_eq!(old, [other]);
    }

    #[test]
    fn a_list_the_engine_cannot_read_is_all_stale() {
        let odd = json!({ "rule": 3 });
        let (same, old) = partition(std::slice::from_ref(&odd), &[]);
        assert!(same.is_empty());
        assert_eq!(old, [odd]);
    }

    fn result() -> Value {
        json!({ "summary": { "error": 2, "violations": [
            { "type": "dependency", "from": "a.ts", "to": "b.ts", "rule": { "name": "no-b", "severity": "error" }, "id": "RB-1" },
            { "type": "element", "from": "c.cs", "to": "C", "rule": { "name": "sealed", "severity": "error" }, "id": "RB-2" },
            { "type": "module", "from": "d.ts", "to": "d.ts", "rule": { "name": "no-orphans", "severity": "warn" } }
        ] } })
    }

    #[test]
    fn without_lifecycle_it_is_dependency_cruisers_output() {
        let rendered = render(&result(), &Lifecycle::default());
        assert_eq!(rendered.exit_code, 0);
        let expected = serde_json::to_string_pretty(&result()["summary"]["violations"])
            .unwrap_or_default()
            + "\n";
        assert_eq!(rendered.output, expected);
        assert_eq!(render(&json!({}), &Lifecycle::default()).output, "[]\n");
        assert_eq!(text(&[]), "[]\n");
    }

    #[test]
    fn lifecycle_fields_follow_the_violation_and_earlier_entries_keep_theirs() {
        let lifecycle = Lifecycle {
            expires: Some("2026-12-31".into()),
            owner: Some("@team".into()),
            reason: Some("port lands".into()),
        };
        let previous = [
            json!({ "id": "RB-2", "expires": "2026-10-01", "owner": "@me", "reason": null }),
            json!({ "id": "RB-9", "owner": "@gone" }),
        ];
        let got = entries(&result(), &lifecycle, &previous);
        assert_eq!(
            got[0],
            json!({ "type": "dependency", "from": "a.ts", "to": "b.ts", "rule": { "name": "no-b", "severity": "error" },
                    "id": "RB-1", "expires": "2026-12-31", "owner": "@team", "reason": "port lands" })
        );
        assert_eq!(got[1]["expires"], "2026-10-01");
        assert_eq!(got[1]["owner"], "@me");
        assert_eq!(got[1]["reason"], "port lands", "a null field is filled");
        assert_eq!(got[2]["owner"], "@team", "an entry without an id is new");
        let bare = entries(&result(), &Lifecycle::default(), &previous);
        assert_eq!(bare[0].get("owner"), None);
        assert_eq!(bare[1]["owner"], "@me");
        let only_reason = Lifecycle {
            reason: Some("r".into()),
            ..Lifecycle::default()
        };
        let got = entries(&result(), &only_reason, &[]);
        assert_eq!((got[0].get("expires"), got[0].get("owner")), (None, None));
        assert_eq!(got[0]["reason"], "r");
    }
}
