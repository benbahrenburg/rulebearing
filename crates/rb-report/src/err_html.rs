//! `err-html`: the summary, the rules with their violation counts, and every violation, as one
//! self-contained HTML page. dependency-cruiser 18.2.0's `src/report/error-html/index.mjs`,
//! `utl.mjs` and `error-html-template.mjs`, ported; the template is upstream's, verbatim, in
//! `err_html.html`.
//!
//! - Specification: `test/report/error-html/error-html.spec.mjs` and `utl.spec.mjs`, run
//!   unmodified by conformance gate 1 layer 3
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Coverage: [coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types),
//!   row `err-html`; [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   row `reporterOptions.err` / `err-long` / `err-html`
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01)
//!
//! The placeholders are filled in upstream's order with JavaScript's `String.prototype.replace`,
//! first occurrence only and with `$` patterns in the replacement expanded, so the page is the
//! one upstream writes for the same result. The footer names the dependency-cruiser version whose
//! output this reproduces, where upstream names its own, and the run's timestamp.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::err::dependency_to;
use crate::style::percentage;
use crate::utl::one_letter_dependency_type;
use crate::{Rendered, js};

/// The template, verbatim from dependency-cruiser 18.2.0.
const TEMPLATE: &str = include_str!("err_html.html");

/// The dependency-cruiser version whose reporters this crate reproduces, printed in the footer
/// where upstream prints its own.
pub const DEPENDENCY_CRUISER_VERSION: &str = "18.2.0";

/// `getFormattedAllowedRule(ruleSetUsed)`: the `not-in-allowed` pseudo rule when there are
/// `allowed` rules, with the first comment any of them carries.
pub fn formatted_allowed_rule(rule_set: Option<&Value>) -> Option<Value> {
    let allowed = rule_set
        .and_then(|r| r.get("allowed"))
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())?;
    let comment = allowed
        .iter()
        .find(|r| r.as_object().is_some_and(|o| o.contains_key("comment")))
        .and_then(|r| r.get("comment").cloned())
        .unwrap_or_else(|| json!("-"));
    let severity = rule_set
        .and_then(|r| r.get("allowedSeverity"))
        .filter(|s| !s.is_null())
        .cloned()
        .unwrap_or_else(|| json!("warn"));
    Some(json!({ "name": "not-in-allowed", "comment": comment, "severity": severity }))
}

/// `aggregateCountsPerRule`: per rule name, `{ count, ignoredCount }`; an `ignore` violation
/// counts as ignored, any other as a violation.
fn counts_per_rule(violations: &[Value]) -> Map<String, Value> {
    let mut counts: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for violation in violations {
        let rule = violation.get("rule").unwrap_or(&Value::Null);
        let entry = counts.entry(js::field(rule, "name")).or_insert((0, 0));
        if rule.get("severity").and_then(Value::as_str) == Some("ignore") {
            entry.1 += 1;
        } else {
            entry.0 += 1;
        }
    }
    counts
        .into_iter()
        .map(|(name, (count, ignored))| (name, json!({ "count": count, "ignoredCount": ignored })))
        .collect()
}

/// `mergeCountsIntoRule(rule, counts)`: the rule with its `count`, `ignoredCount` and whether it
/// is `unviolated`.
pub fn merge_counts_into_rule(rule: &Value, counts: &Map<String, Value>) -> Value {
    let zero = json!({ "count": 0, "ignoredCount": 0 });
    let found = counts
        .get(&js::field(rule, "name"))
        .filter(|c| js::truthy(Some(c)))
        .unwrap_or(&zero);
    let count = found.get("count").cloned().unwrap_or(Value::Null);
    let mut out: Map<String, Value> = rule.as_object().cloned().unwrap_or_default();
    out.insert(
        "unviolated".into(),
        json!(js::to_number(Some(&count)) <= 0.0),
    );
    out.insert("count".into(), count);
    out.insert(
        "ignoredCount".into(),
        found.get("ignoredCount").cloned().unwrap_or(Value::Null),
    );
    Value::Object(out)
}

/// `aggregateViolations(violations, ruleSetUsed)`: every rule with its counts, the most violated
/// first, then the most ignored, then by name. Upstream lists `forbidden`, `required` and the
/// `allowed` pseudo rule; the native element, slice and diagram rules follow them, so every
/// violation's rule link has its row. A dependency-cruiser rule set has none of those, so its page
/// is upstream's.
pub fn aggregate_violations(violations: &[Value], rule_set: Option<&Value>) -> Vec<Value> {
    let counts = counts_per_rule(violations);
    let list = |key: &str| {
        rule_set
            .and_then(|r| r.get(key))
            .map(|v| match v {
                Value::Array(items) => items.clone(),
                Value::Null => Vec::new(),
                other => vec![other.clone()],
            })
            .unwrap_or_default()
    };
    let mut rules: Vec<Value> = list("forbidden")
        .into_iter()
        .chain(list("required"))
        .chain(formatted_allowed_rule(rule_set))
        .chain(list("elements"))
        .chain(list("slices"))
        .chain(list("diagrams"))
        .map(|rule| merge_counts_into_rule(&rule, &counts))
        .collect();
    let figure = |rule: &Value, key: &str| rule.get(key).and_then(Value::as_u64).unwrap_or(0);
    rules.sort_by(|first, second| {
        figure(second, "count")
            .cmp(&figure(first, "count"))
            .then_with(|| figure(second, "ignoredCount").cmp(&figure(first, "ignoredCount")))
            .then_with(|| {
                rb_model::collate::compare(&js::field(first, "name"), &js::field(second, "name"))
            })
    });
    rules
}

/// `names.map(({ name }) => name).join(separator)`: null and undefined names are empty.
fn names(steps: Option<&Value>, separator: &str) -> String {
    steps
        .and_then(Value::as_array)
        .map(|steps| {
            steps
                .iter()
                .map(|s| match s.get("name") {
                    None | Some(Value::Null) => String::new(),
                    name => js::to_string(name),
                })
                .collect::<Vec<_>>()
                .join(separator)
        })
        .unwrap_or_default()
}

fn instability(violation: &Value, end: &str) -> String {
    let value = violation
        .get("metrics")
        .and_then(|m| m.get(end))
        .and_then(|m| m.get("instability"));
    format!(
        "&nbsp;<span class=\"extra\">(I: {})</span>",
        percentage(js::to_number(value))
    )
}

/// The unresolved flags as `formatDependencyTo` reads them.
fn flags(options: &Map<String, Value>) -> (bool, bool) {
    (
        js::truthy(options.get("showExternalModulesUnresolved")),
        js::truthy(options.get("showAliasedModulesUnresolved")),
    )
}

/// `determineTo(violation, options)`: the "to" cell for each violation type.
pub fn determine_to(violation: &Value, options: &Map<String, Value>) -> String {
    let (external, aliased) = flags(options);
    let to = || dependency_to(violation, external, aliased);
    match violation.get("type").and_then(Value::as_str) {
        Some("module") => String::new(),
        Some("cycle") => names(violation.get("cycle"), " &rightarrow;<br/>"),
        Some("reachability") => format!(
            "{}<br/>{}",
            js::field(violation, "to"),
            names(violation.get("via"), " &rightarrow;<br/>")
        ),
        Some("instability") => format!("{}{}", to(), instability(violation, "to")),
        _ => to(),
    }
}

/// `determineFromExtras(violation)`: the instability of the "from" module for an instability
/// violation.
pub fn determine_from_extras(violation: &Value) -> String {
    if violation.get("type").and_then(Value::as_str) == Some("instability") {
        instability(violation, "from")
    } else {
        String::new()
    }
}

/// `formatSummaryForReport(summary)`: the summary with the version, the run date, and each
/// violation's `fromExtras` and `to` cells.
pub fn format_summary_for_report(summary: &Value, timestamp: &str) -> Value {
    let mut out: Map<String, Value> = summary.as_object().cloned().unwrap_or_default();
    out.insert(
        "depcruiseVersion".into(),
        json!(format!("dependency-cruiser@{DEPENDENCY_CRUISER_VERSION}")),
    );
    out.insert("runDate".into(), json!(format!("{timestamp}Z")));
    let violations: Vec<Value> = rb_rules::js::array(summary, "violations")
        .iter()
        .map(|v| {
            let mut violation: Map<String, Value> = v.as_object().cloned().unwrap_or_default();
            violation.insert("fromExtras".into(), json!(determine_from_extras(v)));
            violation.insert("to".into(), json!(determine_to(v, &Map::new())));
            Value::Object(violation)
        })
        .collect();
    out.insert("violations".into(), Value::Array(violations));
    Value::Object(out)
}

fn violated_rule_row(rule: &Value) -> String {
    let unviolated = js::truthy(rule.get("unviolated"));
    let name = js::field(rule, "name");
    let severity = js::field(rule, "severity");
    let row_class = if unviolated {
        " class=\"unviolated\""
    } else {
        ""
    };
    let cell = if unviolated {
        "<span class=\"ok\">&check;</span>".to_owned()
    } else {
        format!("<span class=\"{severity}\">&cross;</span>")
    };
    let ignored = match rule.get("ignoredCount") {
        None | Some(Value::Null) => "0".to_owned(),
        other => js::to_string(other),
    };
    format!(
        "<tr{row_class}>\n    <td>{cell}</td>\n    <td>{severity}</td>\n    <td class=\"nowrap\">\n      <a href=\"#{name}-instance\"\n         id=\"{name}-definition\" \n         class=\"noiseless\">{name}</a>\n    </td>\n    <td><strong>{}</strong></td>\n    <td><strong>{ignored}</strong></td>\n    <td>{}</td>\n  </tr>",
        js::field(rule, "count"),
        js::field(rule, "comment")
    )
}

fn violated_rules_table(summary: &Value) -> String {
    let violations = rb_rules::js::array(summary, "violations");
    let rows: Vec<String> = aggregate_violations(violations, summary.get("ruleSetUsed"))
        .iter()
        .map(violated_rule_row)
        .collect();
    format!(
        r##"<table>
    <tbody>
      <thead>
        <tr>
          <th></th>
          <th>severity</th>
          <th>rule</th>
          <th>violations</th>
          <th>ignored</th>
          <th>explanation</th>
        </tr>
      </thead>
      {}
      <tr>
        <td colspan="6" class="controls">
          <div id="show-unviolated">
            &downarrow; <a href="#show-all-the-rules">also show unviolated rules</a>
          </div>
          <div id="hide-unviolated">
            &uparrow; <a href="">hide unviolated rules</a>
          </div>
        </td>
      </tr>
    </tbody>
  </table>"##,
        rows.join("\n")
    )
}

fn violation_row(violation: &Value, prefix: &str, options: &Map<String, Value>) -> String {
    let rule = violation.get("rule").unwrap_or(&Value::Null);
    let severity = js::field(rule, "severity");
    let name = js::field(rule, "name");
    let row_class = if rule.get("severity").and_then(Value::as_str) == Some("ignore") {
        " class=\"ignored\""
    } else {
        ""
    };
    let types: Vec<String> = match violation.get("dependencyTypes") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|t| match t {
                Value::Null => String::new(),
                other => js::to_string(Some(other)),
            })
            .collect(),
        _ => Vec::new(),
    };
    let type_refs: Vec<&str> = types.iter().map(String::as_str).collect();
    let from = js::field(violation, "from");
    format!(
        "  <tr{row_class}>\n    <td class=\"{severity}\">{severity}</td>\n    <td class=\"nowrap\">\n      <a href=\"#{name}-definition\" \n         id=\"{name}-instance\"\n         class=\"noiseless\">{name}</a>\n    </td>\n    <td><a href=\"{prefix}{from}\">{from}</a>{}</td>\n    <td><span class=\"dependency-type {}\" title=\"dependency types: {}\">{}</span></td>\n    <td>{}</td>\n  </tr>",
        determine_from_extras(violation),
        types.join(" "),
        types.join(", "),
        one_letter_dependency_type(&type_refs),
        determine_to(violation, options)
    )
}

fn violations_list(summary: &Value, options: &Map<String, Value>) -> String {
    let violations = rb_rules::js::array(summary, "violations");
    if violations.is_empty() {
        return "    <h2><span aria-hidden=\"true\">&hearts;</span> No violations found</h2>\n    <p>Get gummy bears to celebrate.</p>".into();
    }
    let prefix = match summary.get("optionsUsed").and_then(|o| o.get("prefix")) {
        None | Some(Value::Null) => String::new(),
        other => js::to_string(other),
    };
    let rows: Vec<String> = violations
        .iter()
        .map(|v| violation_row(v, &prefix, options))
        .collect();
    let ignored = if js::to_number(summary.get("ignore")) > 0.0 {
        r##"<tr>
        <td colspan="5" class="controls">
          <div id="show-ignored">
            &downarrow; <a href="#show-ignored-violations">also show ignored violations</a>
          </div>
          <div id="hide-ignored">
            &uparrow; <a href="">hide ignored violations</a>
          </div>
        </td>
      </tr>"##
    } else {
        ""
    };
    format!(
        r#"<span id="show-ignored-violations">
      <h2><svg class="p__svg--inline" viewBox="0 0 12 16" version="1.1" aria-hidden="true">
        <path fill-rule="evenodd"
          d="M5.05.31c.81 2.17.41 3.38-.52 4.31C3.55 5.67 1.98 6.45.9 7.98c-1.45 2.05-1.7 6.53 3.53 7.7-2.2-1.16-2.67-4.52-.3-6.61-.61 2.03.53 3.33 1.94 2.86 1.39-.47 2.3.53 2.27 1.67-.02.78-.31 1.44-1.13 1.81 3.42-.59 4.78-3.42 4.78-5.56 0-2.84-2.53-3.22-1.25-5.61-1.52.13-2.03 1.13-1.89 2.75.09 1.08-1.02 1.8-1.86 1.33-.67-.41-.66-1.19-.06-1.78C8.18 5.31 8.68 2.45 5.05.32L5.03.3l.02.01z">
        </path>
      </svg> All violations</h2>
    <table>
      <thead>
        <tr>
          <th>severity</th>
          <th>rule</th>
          <th>from</th>
          <th>types</th>
          <th>to</th>
        </tr>
      </thead>
      <tbody>
      {}
      {ignored}
      </tbody>
    </table>
    </span>"#,
        rows.join("\n")
    )
}

/// Renders `err-html`, with `options` the `reporterOptions.err-html` section and `timestamp` the
/// run's time, ISO 8601 without the `Z`.
pub fn render(result: &Value, options: Option<&Value>, timestamp: &str) -> Rendered {
    let options = options
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let summary = result.get("summary").unwrap_or(&Value::Null);
    let number = |key: &str| js::field(summary, key);
    let ignore = match summary.get("ignore") {
        None | Some(Value::Null) => "0".to_owned(),
        other => js::to_string(other),
    };
    let replacements = [
        ("{{totalCruised}}", number("totalCruised")),
        (
            "{{totalDependenciesCruised}}",
            number("totalDependenciesCruised"),
        ),
        ("{{error}}", number("error")),
        ("{{warn}}", number("warn")),
        ("{{info}}", number("info")),
        ("{{ignore}}", ignore),
        ("{{violatedRulesTable}}", violated_rules_table(summary)),
        ("{{violationsList}}", violations_list(summary, &options)),
        (
            "{{depcruiseVersion}}",
            format!("dependency-cruiser@{DEPENDENCY_CRUISER_VERSION}"),
        ),
        ("{{runDate}}", format!("{timestamp}Z")),
    ];
    let output = replacements
        .iter()
        .fold(TEMPLATE.to_owned(), |page, (needle, value)| {
            js::replace_first(&page, needle, value)
        });
    Rendered {
        output,
        exit_code: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(violations: &Value, extra: &Value) -> Value {
        let mut summary = json!({
            "violations": violations, "error": 0, "warn": 1, "info": 2, "totalCruised": 127,
            "totalDependenciesCruised": 259, "optionsUsed": { "prefix": "https://x/" },
            "ruleSetUsed": { "forbidden": [{ "name": "no-b", "severity": "warn", "comment": "c" }, { "name": "quiet", "severity": "info" }] }
        });
        if let (Some(s), Some(e)) = (summary.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                s.insert(k.clone(), v.clone());
            }
        }
        json!({ "modules": [], "summary": summary })
    }

    #[test]
    fn nothing_found() {
        let page = render(
            &summary(&json!([]), &json!({})),
            None,
            "2026-01-01T00:00:00.000",
        )
        .output;
        assert!(page.contains("Get gummy bears to celebrate."));
        assert!(page.contains("<strong>127</strong> modules"));
        assert!(page.contains("<strong>0</strong> ignored"));
        assert!(
            page.contains("dependency-cruiser@18.2.0</a> /\n      2026-01-01T00:00:00.000Z</p>")
        );
        assert!(page.contains("<tr class=\"unviolated\">"));
        assert!(page.starts_with("<!DOCTYPE html>"));
    }

    #[test]
    fn violations_of_each_type() {
        let violations = json!([
            { "type": "dependency", "from": "a", "to": "b", "unresolvedTo": "pkg", "dependencyTypes": ["npm-no-pkg"], "rule": { "name": "no-b", "severity": "warn" } },
            { "type": "module", "from": "m", "to": "m", "rule": { "name": "quiet", "severity": "ignore" } },
            { "type": "cycle", "from": "c", "to": "d", "cycle": [{ "name": "d" }, { "name": "c" }, {}], "rule": { "name": "cyc", "severity": "error" } },
            { "type": "reachability", "from": "r", "to": "s", "via": [{ "name": "v1" }, { "name": "v2" }], "rule": { "name": "reach", "severity": "info" } },
            { "type": "instability", "from": "i", "to": "j", "metrics": { "from": { "instability": 0.333 }, "to": { "instability": 0.8 } }, "rule": { "name": "sdp", "severity": "info" } }
        ]);
        let result = summary(&violations, &json!({ "ignore": 1 }));
        let page = render(
            &result,
            Some(&json!({ "showExternalModulesUnresolved": 1 })),
            "t",
        )
        .output;
        assert!(page.contains("<td><a href=\"https://x/a\">a</a></td>"));
        assert!(
            page.contains("title=\"dependency types: npm-no-pkg\">n</span></td>\n    <td>pkg</td>")
        );
        assert!(page.contains("<tr class=\"ignored\">"));
        assert!(page.contains("<td>d &rightarrow;<br/>c &rightarrow;<br/></td>"));
        assert!(page.contains("<td>s<br/>v1 &rightarrow;<br/>v2</td>"));
        assert!(page.contains("i</a>&nbsp;<span class=\"extra\">(I: 33%)</span></td>"));
        assert!(page.contains("<td>j&nbsp;<span class=\"extra\">(I: 80%)</span></td>"));
        assert!(page.contains("also show ignored violations"));
        assert!(page.contains(
            "<td><strong>1</strong></td>\n    <td><strong>0</strong></td>\n    <td>c</td>"
        ));
        let plain = render(&result, None, "t").output;
        assert!(
            plain.contains("<td>b</td>"),
            "the resolved name without the flag"
        );
    }

    #[test]
    fn aggregation_orders_and_counts() {
        let violations = vec![
            json!({ "rule": { "name": "b", "severity": "warn" } }),
            json!({ "rule": { "name": "b", "severity": "ignore" } }),
            json!({ "rule": { "name": "a", "severity": "ignore" } }),
        ];
        let rule_set = json!({ "forbidden": [{ "name": "a" }, { "name": "c" }, { "name": "b" }], "required": { "name": "d" },
                               "allowed": [{ "from": {} }, { "comment": "why" }], "allowedSeverity": "error" });
        let rules = aggregate_violations(&violations, Some(&rule_set));
        let order: Vec<String> = rules.iter().map(|r| js::field(r, "name")).collect();
        assert_eq!(order, vec!["b", "a", "c", "d", "not-in-allowed"]);
        assert_eq!(rules[0]["count"], json!(1));
        assert_eq!(rules[0]["ignoredCount"], json!(1));
        assert_eq!(rules[1]["unviolated"], json!(true));
        assert_eq!(rules[4]["comment"], json!("why"));
        assert_eq!(rules[4]["severity"], json!("error"));
        assert_eq!(
            formatted_allowed_rule(Some(&json!({ "allowed": [] }))),
            None
        );
        assert_eq!(
            formatted_allowed_rule(Some(&json!({ "allowed": [{}] }))),
            Some(json!({ "name": "not-in-allowed", "comment": "-", "severity": "warn" }))
        );
        assert_eq!(determine_from_extras(&json!({ "type": "dependency" })), "");
        assert_eq!(
            determine_to(&json!({ "type": "module", "to": "x" }), &Map::new()),
            ""
        );
    }

    #[test]
    fn element_slice_and_diagram_rules_have_rows_too() {
        let violations =
            vec![json!({ "type": "element", "rule": { "name": "sealed", "severity": "error" } })];
        let rule_set = json!({ "forbidden": [{ "name": "a" }], "elements": [{ "name": "sealed" }],
                               "slices": [{ "name": "apart" }], "diagrams": [{ "name": "drawn" }] });
        let rules = aggregate_violations(&violations, Some(&rule_set));
        let order: Vec<String> = rules.iter().map(|r| js::field(r, "name")).collect();
        assert_eq!(order, vec!["sealed", "a", "apart", "drawn"]);
        assert_eq!(rules[0]["count"], json!(1));
    }

    #[test]
    fn replacement_patterns_expand_as_upstream() {
        let result = summary(&json!([]), &json!({ "totalCruised": "$&" }));
        let page = render(&result, None, "t").output;
        assert!(page.contains("<strong>{{totalCruised}}</strong> modules"));
    }
}
