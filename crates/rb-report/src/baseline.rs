//! `baseline`: every violation as a `knownViolations[]` entry, the file `depcruise-baseline`
//! writes, keyed by the stable id and carrying `expires`, `owner` and `reason` when they are given.
//!
//! - Contract: [Wave 2 plan § 1.5](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#15-interfaces-and-contracts-this-wave-freezes)
//!   (the `knownViolations[]` entry)
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
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
    violations
        .into_iter()
        .map(|mut entry| {
            let id = entry.get("id").and_then(Value::as_str).map(str::to_owned);
            let earlier = id.as_deref().and_then(|id| {
                previous
                    .iter()
                    .find(|p| p.get("id").and_then(Value::as_str) == Some(id))
            });
            if let Value::Object(map) = &mut entry {
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

/// Renders `baseline`. It exits 0, as dependency-cruiser's does.
pub fn render(result: &Value, lifecycle: &Lifecycle) -> Rendered {
    Rendered {
        output: text(&entries(result, lifecycle, &[])),
        exit_code: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
