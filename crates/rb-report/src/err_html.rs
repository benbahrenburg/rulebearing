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
//! - Plan: [Wave 2, Step 10](../../../docs/plans/implemented/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01)
//!
//! The placeholders are filled in upstream's order with JavaScript's `String.prototype.replace`,
//! first occurrence only and with `$` patterns in the replacement expanded, so the page is the
//! one upstream writes for the same result. The footer names the dependency-cruiser version whose
//! output this reproduces, where upstream names its own, and the run's timestamp.
//!
//! A violation on a declared edge ([ADR-0078](../../../docs/adr/0078-fleet-and-declared-edges.md))
//! has ` (declared: <kind>)` after its target in the "to" cell, and a cycle or path step a
//! declared edge reaches after the step (`crate::declared`); `markdown` shares the cell.
//!
//! The stale-baseline count and table are 18.7.0's ([plan 0011 Step 9](../../../docs/plans/pending/0011-dependency-cruiser-18-7.md#24-steps-for-sub-wave-11d-the-reporters),
//! [ADR-0080](../../../docs/adr/0080-gate-1-moves-to-dependency-cruiser-18-7-0.md)). Their two
//! placeholders leave blank lines even when empty, which 18.2.0's page does not have, so a result
//! without `summary.baselineStale` is printed without them until
//! [Step 16](../../../docs/plans/pending/0011-dependency-cruiser-18-7.md#28-steps-for-sub-wave-11h-the-pin)
//! moves the pin (`has_stale_field`).

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::err::dependency_to;
use crate::style::percentage;
use crate::utl::one_letter_dependency_type;
use crate::{Edges, Rendered, declared, js};

/// The template, verbatim from dependency-cruiser 18.7.0.
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

/// `names.map(({ name }) => name).join(separator)`: null and undefined names are empty. A step a
/// declared edge reaches is followed by its mark (`crate::declared`).
fn names(edges: &Edges<'_>, violation: &Value, key: &str, separator: &str) -> String {
    let mut previous = js::field(violation, "from");
    violation
        .get(key)
        .and_then(Value::as_array)
        .map(|steps| {
            steps
                .iter()
                .map(|s| {
                    let name = match s.get("name") {
                        None | Some(Value::Null) => String::new(),
                        name => js::to_string(name),
                    };
                    let marked = format!("{name}{}", declared::edge_mark(edges, &previous, &name));
                    previous = name;
                    marked
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
    determine_to_marked(violation, options, &Edges::of(&Value::Null))
}

/// [`determine_to`], with a declared edge marked ` (declared: <kind>)` after the target or the
/// step it reaches (`crate::declared`), the edges looked up in `edges`.
pub(crate) fn determine_to_marked(
    violation: &Value,
    options: &Map<String, Value>,
    edges: &Edges<'_>,
) -> String {
    let (external, aliased) = flags(options);
    let to = || {
        format!(
            "{}{}",
            dependency_to(violation, external, aliased),
            declared::violation_mark(edges, violation)
        )
    };
    match violation.get("type").and_then(Value::as_str) {
        Some("module") => String::new(),
        Some("cycle") => names(edges, violation, "cycle", " &rightarrow;<br/>"),
        Some("reachability") => format!(
            "{}<br/>{}",
            js::field(violation, "to"),
            names(edges, violation, "via", " &rightarrow;<br/>")
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

fn violation_row(
    violation: &Value,
    prefix: &str,
    options: &Map<String, Value>,
    edges: &Edges<'_>,
) -> String {
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
        determine_to_marked(violation, options, edges)
    )
}

fn violations_list(summary: &Value, options: &Map<String, Value>, edges: &Edges<'_>) -> String {
    let violations = rb_rules::js::array(summary, "violations");
    if violations.is_empty() {
        return "    <h2><span aria-hidden=\"true\">&hearts;</span> No violations found</h2>\n    <p>Get gummy bears to celebrate.</p>".into();
    }
    let prefix = prefix_of(summary);
    let rows: Vec<String> = violations
        .iter()
        .map(|v| violation_row(v, &prefix, options, edges))
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

/// `summary.optionsUsed.prefix`, empty when absent.
fn prefix_of(summary: &Value) -> String {
    match summary.get("optionsUsed").and_then(|o| o.get("prefix")) {
        None | Some(Value::Null) => String::new(),
        other => js::to_string(other),
    }
}

/// The condition the module doc names: Step 16 removes it and prints every page with both
/// placeholders.
fn has_stale_field(summary: &Value) -> bool {
    summary.get("baselineStale").is_some()
}

/// `constructStaleDiv`: the count beside the other counts.
fn stale_div(summary: &Value) -> String {
    crate::stale_count(summary).map_or_else(String::new, |count| {
        format!(
            "<div style=\"float:left;padding-right:20px\" class=\"ignore\">\n    <strong>{count}</strong> stale entries in baseline\n    </div>"
        )
    })
}

/// `constructBaselineStaleTable`: the entries no violation matches. Upstream throws a `TypeError`
/// on a count without `optionsUsed.knownViolations`, which no run writes; this prints no table.
fn stale_table(summary: &Value, options: &Map<String, Value>) -> String {
    let (Some(_), Some(stale)) = (
        crate::stale_count(summary),
        crate::baseline::stale_entries(summary),
    ) else {
        return String::new();
    };
    let prefix = prefix_of(summary);
    let edges = Edges::of(&Value::Null);
    let rows: Vec<String> = stale
        .iter()
        .map(|v| violation_row(v, &prefix, options, &edges))
        .collect();
    format!(
        r#"<span id="stale-baseline-entries">
      <h2><svg class="p__svg--inline" viewBox="144 144 512 512" version="1.1" aria-hidden="true"><path d="M583.48 299.64a202 202 0 0 0-108.32-109.58 194.4 194.4 0 0 0-150.29 0 202.3 202.3 0 0 0-123.43 186.4v224.26a25.2 25.2 0 0 0 5.04 14.7q4.46.63 8.96.61c8.52.2 16.98-1.52 24.74-5.04a89 89 0 0 0 26.8-22.57c2.57-2.82 5.04-5.54 7.66-8.06 3.6-3.67 7.7-6.84 12.14-9.42a41 41 0 0 1 20.81-5.04c19.75 0 29.43 10.53 40.6 22.73a101 101 0 0 0 22.58 20.15 63 63 0 0 0 4.48 2.42 60.4 60.4 0 0 0 49.48 0q2.33-1.08 4.43-2.37c8.5-5.54 16.1-12.34 22.57-20.15 11.18-12.2 20.86-22.73 40.6-22.73A41 41 0 0 1 513.2 571a58 58 0 0 1 12.15 9.42 132 132 0 0 1 7.65 8.07 89 89 0 0 0 27 22.36 57 57 0 0 0 24.75 5.04q4.53 0 9.01-.6c3.2-4.25 4.95-9.4 5.04-14.71v-224.1a202 202 0 0 0-15.32-76.83zm0 124.74v176.33c-18.9-.45-28.41-10.73-39.35-22.67-2.42-2.62-5.04-5.34-7.5-7.96a83 83 0 0 0-11.44-9.73 55.4 55.4 0 0 0-32.8-10.07c-26.4 0-39.9 14.71-51.74 27.66s-20.9 23.02-40.65 23.02-29.43-10.53-40.61-22.72-25.2-27.66-51.74-27.66a55.4 55.4 0 0 0-32.75 10.08 83 83 0 0 0-11.44 9.72c-2.62 2.62-5.04 5.34-7.56 7.96-10.93 11.94-20.45 22.17-39.3 22.67V376.46c0-102.78 82.33-186.4 183.5-186.4 101.16 0 183.49 83.63 183.49 186.4z"/><path d="M511.69 280.75a59.6 59.6 0 1 0 59.6 63.83q.4-3.98 0-7.95a59.55 59.55 0 0 0-59.6-55.88m-44.53 59.65a44.45 44.45 0 0 1 66.1-38.84h-.91a38.92 38.92 0 0 0 0 77.84h.9a44.45 44.45 0 0 1-66.1-38.84zm-38.59-3.98a59.6 59.6 0 1 0 0 7.96q.4-3.99 0-7.96m-104.14 3.98a44.44 44.44 0 0 1 66.1-38.84h-.6a38.91 38.91 0 1 0 0 77.84h.9a44.45 44.45 0 0 1-66.4-39"/>
    </svg> Stale entries in the baseline</h2>
    <p>
      These violations are in the baseline (typically <tt>.dependency-cruiser-known-violations.json</tt>), 
      but don't match any real violations anymore, e.g. because they were fixed in the meantime. You
      can remove them with dependency-cruiser's 
      <a href="https://github.com/sverweij/dependency-cruiser/blob/main/doc/cli.md#--baseline-create-or-update-a-known-violations-baseline"><tt>--baseline --baseline-mode shrink-only</tt>
      command line options</a>.
    </p>
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
    let ahead = has_stale_field(summary);
    let template = if ahead {
        TEMPLATE.to_owned()
    } else {
        TEMPLATE
            .replace("    {{staleDiv}}\n", "")
            .replace("  {{baselineStaleTable}}\n", "")
    };
    let mut replacements = vec![
        ("{{totalCruised}}", number("totalCruised")),
        (
            "{{totalDependenciesCruised}}",
            number("totalDependenciesCruised"),
        ),
        ("{{error}}", number("error")),
        ("{{warn}}", number("warn")),
        ("{{info}}", number("info")),
        ("{{ignore}}", ignore),
    ];
    if ahead {
        replacements.push(("{{staleDiv}}", stale_div(summary)));
    }
    replacements.push(("{{violatedRulesTable}}", violated_rules_table(summary)));
    replacements.push((
        "{{violationsList}}",
        violations_list(summary, &options, &Edges::of(result)),
    ));
    if ahead {
        replacements.push(("{{baselineStaleTable}}", stale_table(summary, &options)));
    }
    replacements.push((
        "{{depcruiseVersion}}",
        format!("dependency-cruiser@{DEPENDENCY_CRUISER_VERSION}"),
    ));
    replacements.push(("{{runDate}}", format!("{timestamp}Z")));
    let output = replacements.iter().fold(template, |page, (needle, value)| {
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

    const STAMP: &str = "2026-01-01T00:00:00.000";

    fn stale_extra(count: u64) -> Value {
        json!({ "baselineStale": count, "optionsUsed": { "prefix": "https://x/", "knownViolations": [
            { "from": "src/z.js", "to": "src/removed.js", "rule": { "severity": "error", "name": "no-old" } },
            { "from": "src/a.js", "to": "src/removed.js", "rule": { "severity": "error", "name": "no-old" } },
        ] } })
    }

    /// Plan 0011 Step 9: upstream's `test/report/error-html/error-html.spec.mjs` case at 18.7.0;
    /// the rows are in upstream's order, whatever the file's, and two runs print the same bytes.
    #[test]
    fn stale_entries_get_a_count_and_a_table() {
        let result = summary(&json!([]), &stale_extra(2));
        let page = render(&result, None, STAMP);
        assert_eq!(page.exit_code, 0);
        let page = page.output;
        assert!(
            page.contains("<div style=\"float:left;padding-right:20px\" class=\"ignore\">\n    <strong>2</strong> stale entries in baseline\n    </div>\n    &nbsp;"),
            "{page}"
        );
        assert!(page.contains("<span id=\"stale-baseline-entries\">"));
        assert!(page.contains("</svg> Stale entries in the baseline</h2>"));
        let at = |needle: &str| page.find(needle).unwrap_or(usize::MAX);
        assert!(at("<a href=\"https://x/src/a.js\">") < at("<a href=\"https://x/src/z.js\">"));
        assert!(at("Get gummy bears") < at("stale-baseline-entries"));
        assert!(at("stale-baseline-entries") < at("<footer>"));
        assert_eq!(page, render(&result, None, STAMP).output);
    }

    #[test]
    fn a_zero_count_keeps_upstreams_blank_lines_and_prints_no_stale_section() {
        let page = render(&summary(&json!([]), &stale_extra(0)), None, STAMP).output;
        assert!(!page.contains("stale entries in baseline"));
        assert!(!page.contains("stale-baseline-entries"));
        assert!(
            page.contains("ignored\n    </div>\n    \n    &nbsp;"),
            "{page}"
        );
        assert!(page.contains("celebrate.</p>\n  \n  <footer>"), "{page}");
    }

    /// The condition Step 16 removes: without the field the page is 18.2.0's, placeholder text in
    /// a rule's comment included.
    #[test]
    fn a_result_without_the_field_prints_the_older_page() {
        let extra = json!({ "ruleSetUsed": { "forbidden": [
            { "name": "r", "severity": "warn", "comment": "{{staleDiv}} {{baselineStaleTable}}" }
        ] } });
        let page = render(&summary(&json!([]), &extra), None, STAMP).output;
        assert!(page.contains("ignored\n    </div>\n    &nbsp;"), "{page}");
        assert!(page.contains("celebrate.</p>\n  <footer>"), "{page}");
        assert!(
            page.contains("{{staleDiv}} {{baselineStaleTable}}"),
            "{page}"
        );
    }

    #[test]
    fn a_count_without_a_list_prints_no_table() {
        let extra = json!({ "baselineStale": 1, "optionsUsed": {} });
        let page = render(&summary(&json!([]), &extra), None, STAMP).output;
        assert!(page.contains("<strong>1</strong> stale entries in baseline"));
        assert!(!page.contains("stale-baseline-entries"));
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
