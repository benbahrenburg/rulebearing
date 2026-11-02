//! `markdown`: the summary, the rules with their counts and every violation as Markdown, for a
//! pull-request comment or a job summary. dependency-cruiser 18.2.0's `src/report/markdown.mjs`,
//! ported; the "to" cell and the rule counts are `err-html`'s ([`crate::err_html`]), as upstream
//! imports them from `error-html/utl.mjs`.
//!
//! - Specification: `test/report/markdown/markdown.spec.mjs`, run unmodified by conformance gate 1
//!   layer 3, and upstream's reporter over every `test/report` mock with each option toggled
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Coverage: [coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types),
//!   row `markdown`; [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   row `reporterOptions.markdown`
//! - Plan: [Wave 3, Step 6](../../../docs/plans/implemented/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01)
//!
//! The options are upstream's `{ ...REPORT_DEFAULTS, ...options }`: a key given as `null` replaces
//! its default, and the `show*` keys are read for their JavaScript truthiness. `showStatsSummary`
//! is one of upstream's defaults that its reporter never reads (the statistics line is always
//! written), and it is the same here, so a report is upstream's for every option set. The default
//! footer names the dependency-cruiser version whose output this reproduces and the run's
//! timestamp, as `err-html`'s does.

use std::fmt::Write as _;

use serde_json::{Map, Value, json};

use crate::err_html::{
    DEPENDENCY_CRUISER_VERSION, aggregate_violations, determine_from_extras, determine_to_marked,
};
use crate::{Edges, Rendered, js};

/// Upstream's default `staleBaselineIntro`, word for word.
const STALE_BASELINE_INTRO: &str = "These violations are in the baseline (typically `.dependency-cruiser-known-violations.json`) but don't match any real violations anymore, e.g. because they were fixed in the meantime. You can remove them with dependency-cruiser's [`--baseline --baseline-mode shrink-only`](https://github.com/sverweij/dependency-cruiser/blob/main/doc/cli.md#--baseline-create-or-update-a-known-violations-baseline) command line options.";

/// Every key of upstream's `REPORT_DEFAULTS` except `footer`, whose default carries the time, with
/// its default.
pub fn defaults() -> Map<String, Value> {
    let pairs = [
        ("showTitle", json!(true)),
        ("title", json!("## Forbidden dependency check - results")),
        ("showSummary", json!(true)),
        ("showSummaryHeader", json!(true)),
        (
            "summaryHeader",
            json!("### :chart_with_upwards_trend: Summary"),
        ),
        ("showStatsSummary", json!(true)),
        ("showRulesSummary", json!(true)),
        ("includeIgnoredInSummary", json!(true)),
        ("showDetails", json!(true)),
        ("includeIgnoredInDetails", json!(true)),
        ("showDetailsHeader", json!(true)),
        ("detailsHeader", json!("### :fire: All violations")),
        ("collapseDetails", json!(true)),
        (
            "collapsedMessage",
            json!("Violations found - click to expand"),
        ),
        ("showExternalModulesUnresolved", json!(false)),
        ("showAliasedModulesUnresolved", json!(false)),
        (
            "noViolationsMessage",
            json!(":revolving_hearts: No violations found. Get gummy bears to celebrate."),
        ),
        ("showStaleBaselineDetails", json!(true)),
        (
            "staleBaselineHeader",
            json!("### :ghost: Stale entries in the baseline"),
        ),
        ("collapseStaleBaseline", json!(true)),
        (
            "collapseStaleBaselineMessage",
            json!("Stale violations in the baseline - click to expand"),
        ),
        ("staleBaselineIntro", json!(STALE_BASELINE_INTRO)),
        ("showFooter", json!(true)),
    ];
    pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

/// The default footer: a rule, then the dependency-cruiser version and the run's time.
pub fn default_footer(timestamp: &str) -> String {
    format!(
        "---\n[dependency-cruiser@{DEPENDENCY_CRUISER_VERSION}](https://www.github.com/sverweij/dependency-cruiser) / {timestamp}Z"
    )
}

/// `{ ...REPORT_DEFAULTS, ...(options || {}) }`.
pub fn merge_options(options: Option<&Value>, timestamp: &str) -> Map<String, Value> {
    let mut merged = defaults();
    merged.insert("footer".into(), Value::String(default_footer(timestamp)));
    if let Some(given) = options
        .filter(|o| js::truthy(Some(o)))
        .and_then(Value::as_object)
    {
        for (key, value) in given {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

/// `severity2Icon(severity)`: the GitHub emoji for a severity, `:warning:` for any other value.
pub fn severity_icon(severity: Option<&Value>) -> &'static str {
    match severity.and_then(Value::as_str) {
        Some("error") => ":exclamation:",
        Some("info") => ":grey_exclamation:",
        Some("ignore") => ":see_no_evil:",
        _ => ":warning:",
    }
}

/// `formatStatsSummary(summary)`: the counts on one line.
pub fn stats_summary(summary: &Value) -> String {
    let spacer = "&nbsp;".repeat(4);
    let count = |key: &str| js::field(summary, key);
    let stale = crate::stale_count(summary).map_or_else(String::new, |stale| {
        format!("{spacer}**{stale}** stale entries in baseline")
    });
    format!(
        "**{}** modules{spacer}**{}** dependencies{spacer}**{}** errors{spacer}**{}** warnings{spacer}**{}** informational{spacer}**{}** ignored{stale}\n",
        count("totalCruised"),
        count("totalDependenciesCruised"),
        count("error"),
        count("warn"),
        count("info"),
        count("ignore"),
    )
}

fn violations(summary: &Value) -> &[Value] {
    rb_rules::js::array(summary, "violations")
}

/// `formatRulesSummary(result, includeIgnoredInSummary)`: one row per violated rule, and per
/// rule with only ignored violations when those are included.
pub fn rules_summary(summary: &Value, include_ignored: bool) -> String {
    let number = |rule: &Value, key: &str| js::to_number(rule.get(key));
    aggregate_violations(violations(summary), summary.get("ruleSetUsed"))
        .iter()
        .filter(|rule| {
            number(rule, "count") > 0.0 || (include_ignored && number(rule, "ignoredCount") > 0.0)
        })
        .fold(
            "|rule|violations|ignored|explanation\n|:---|:---:|:---:|:---|\n".to_owned(),
            |all, rule| {
                format!(
                    "{all}|{}&nbsp;_{}_|**{}**|**{}**|{}|\n",
                    severity_icon(rule.get("severity")),
                    js::field(rule, "name"),
                    js::field(rule, "count"),
                    js::field(rule, "ignoredCount"),
                    js::field(rule, "comment"),
                )
            },
        )
}

/// `formatViolations(violations, options)`: one row per violation, the ignored ones only when
/// `includeIgnoredInDetails` is set.
pub fn violation_rows(summary: &Value, options: &Map<String, Value>) -> String {
    violation_rows_marked(violations(summary), options, &Edges::of(&Value::Null))
}

/// `staleBaselineDetails`: the entries of `summary.optionsUsed.knownViolations` no current
/// violation matches, as a table under its header; empty when there are none
/// ([plan 0011 Step 10](../../../docs/plans/pending/0011-dependency-cruiser-18-7.md#24-steps-for-sub-wave-11d-the-reporters)).
///
/// A result without `summary.baselineStale`, reported without any of the stale-baseline options,
/// was saved by a version that has no such table, and re-reports as that version did, which is
/// what gate 1 compares at the 18.2.0 pin: its mocks carry `knownViolations`. Every Rulebearing
/// run with known violations carries the field. Upstream's 18.7.0 reporter asks for neither, so
/// [plan 0011 Step 16](../../../docs/plans/pending/0011-dependency-cruiser-18-7.md#28-steps-for-sub-wave-11h-the-pin)
/// drops `given` and this condition when the pin moves.
fn stale_baseline_details(summary: &Value, options: &Map<String, Value>, given: bool) -> String {
    if !given && summary.get("baselineStale").is_none() {
        return String::new();
    }
    let Some(stale) = crate::baseline::stale_entries(summary).filter(|s| !s.is_empty()) else {
        return String::new();
    };
    let mut out = option_text(options, "staleBaselineHeader");
    out.push_str("\n\n");
    let collapse = flag(options, "collapseStaleBaseline");
    if collapse {
        let _ = write!(
            out,
            "<details><summary>{}</summary>\n\n",
            option_text(options, "collapseStaleBaselineMessage")
        );
    }
    out.push_str(&option_text(options, "staleBaselineIntro"));
    out.push_str("\n\n");
    out.push_str(&violation_rows_marked(
        &stale,
        options,
        &Edges::of(&Value::Null),
    ));
    out.push_str("\n\n");
    if collapse {
        out.push_str("</details>\n\n");
    }
    out
}

/// [`violation_rows`], with a declared edge marked in the "to" cell
/// ([`crate::err_html::determine_to_marked`]).
fn violation_rows_marked(
    violations: &[Value],
    options: &Map<String, Value>,
    edges: &Edges<'_>,
) -> String {
    let include_ignored = js::truthy(options.get("includeIgnoredInDetails"));
    violations
        .iter()
        .filter(|v| {
            let severity = v.get("rule").and_then(|r| r.get("severity"));
            severity.and_then(Value::as_str) != Some("ignore") || include_ignored
        })
        .fold(
            "|violated rule|module|to|\n|:---|:---|:---|\n".to_owned(),
            |all, v| {
                let rule = v.get("rule").unwrap_or(&Value::Null);
                format!(
                    "{all}|{}&nbsp;_{}_|{}{}|{}|\n",
                    severity_icon(rule.get("severity")),
                    js::field(rule, "name"),
                    js::field(v, "from"),
                    determine_from_extras(v),
                    determine_to_marked(v, options, edges),
                )
            },
        )
}

fn option_text(options: &Map<String, Value>, key: &str) -> String {
    js::to_string(options.get(key))
}

fn flag(options: &Map<String, Value>, key: &str) -> bool {
    js::truthy(options.get(key))
}

fn details(summary: &Value, options: &Map<String, Value>, edges: &Edges<'_>) -> String {
    let mut out = String::new();
    if violations(summary).is_empty() {
        out.push_str(&option_text(options, "noViolationsMessage"));
        out.push_str("\n\n");
        return out;
    }
    if flag(options, "showDetailsHeader") {
        out.push_str(&option_text(options, "detailsHeader"));
        out.push_str("\n\n");
    }
    let collapse = flag(options, "collapseDetails");
    if collapse {
        let _ = write!(
            out,
            "<details><summary>{}</summary>\n\n",
            option_text(options, "collapsedMessage")
        );
    }
    out.push_str(&violation_rows_marked(violations(summary), options, edges));
    out.push_str("\n\n");
    if collapse {
        out.push_str("</details>\n\n");
    }
    out
}

fn summary_section(summary: &Value, options: &Map<String, Value>) -> String {
    let mut out = String::new();
    if flag(options, "showSummaryHeader") {
        out.push_str(&option_text(options, "summaryHeader"));
        out.push_str("\n\n");
    }
    out.push_str(&stats_summary(summary));
    out.push_str("\n\n");
    if !violations(summary).is_empty() && flag(options, "showRulesSummary") {
        out.push_str(&rules_summary(
            summary,
            flag(options, "includeIgnoredInSummary"),
        ));
        out.push_str("\n\n");
    }
    out
}

/// Whether `reporterOptions.markdown` names a stale-baseline key, the option half of the condition
/// on [`stale_baseline_details`], which Step 16 removes.
fn stale_option_given(options: Option<&Value>) -> bool {
    options.and_then(Value::as_object).is_some_and(|given| {
        crate::json::MARKDOWN_AHEAD_OF_PIN
            .iter()
            .any(|k| given.contains_key(*k))
    })
}

/// Renders `markdown`, with `options` the `reporterOptions.markdown` section and `timestamp` the
/// run's time, ISO 8601 without the `Z`. Always exits 0, as upstream's reporter does.
pub fn render(result: &Value, options: Option<&Value>, timestamp: &str) -> Rendered {
    let stale_given = stale_option_given(options);
    let options = merge_options(options, timestamp);
    let summary = result.get("summary").unwrap_or(&Value::Null);
    let mut output = String::new();
    if flag(&options, "showTitle") {
        output.push_str(&option_text(&options, "title"));
        output.push_str("\n\n");
    }
    if flag(&options, "showSummary") {
        output.push_str(&summary_section(summary, &options));
    }
    if flag(&options, "showDetails") {
        output.push_str(&details(summary, &options, &Edges::of(result)));
    }
    if flag(&options, "showStaleBaselineDetails") {
        output.push_str(&stale_baseline_details(summary, &options, stale_given));
    }
    if flag(&options, "showFooter") {
        output.push_str(&option_text(&options, "footer"));
        output.push_str("\n\n");
    }
    Rendered {
        output,
        exit_code: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAMP: &str = "2026-09-27T10:00:00.000";

    fn stale_result() -> Value {
        json!({
            "modules": [],
            "summary": {
                "totalCruised": 1, "totalDependenciesCruised": 1,
                "error": 0, "warn": 0, "info": 0, "ignore": 0, "baselineStale": 1,
                "violations": [],
                "optionsUsed": { "knownViolations": [
                    { "type": "dependency", "from": "src/a.js", "to": "src/b.js",
                      "rule": { "name": "no-b", "severity": "error" } },
                ] },
            },
        })
    }

    const STALE_ROWS: &str = "|violated rule|module|to|\n|:---|:---|:---|\n|:exclamation:&nbsp;_no-b_|src/a.js|src/b.js|\n\n\n";

    fn only_stale(extra: &Value) -> Value {
        let mut options = json!({ "showTitle": false, "showSummary": false, "showDetails": false,
            "showFooter": false, "noViolationsMessage": "" });
        for (key, value) in extra.as_object().into_iter().flatten() {
            options[key] = value.clone();
        }
        options
    }

    #[test]
    fn the_stale_table_defaults_to_a_collapsed_block_with_upstreams_words() {
        let out = render(&stale_result(), Some(&only_stale(&json!({}))), STAMP).output;
        assert_eq!(
            out,
            format!(
                "### :ghost: Stale entries in the baseline\n\n<details><summary>Stale violations in the baseline - click to expand</summary>\n\n{STALE_BASELINE_INTRO}\n\n{STALE_ROWS}</details>\n\n"
            )
        );
        assert_eq!(
            out,
            render(&stale_result(), Some(&only_stale(&json!({}))), STAMP).output
        );
    }

    #[test]
    fn each_stale_option_acts() {
        let options = only_stale(&json!({
            "staleBaselineHeader": "H", "collapseStaleBaseline": false, "staleBaselineIntro": "I",
        }));
        assert_eq!(
            render(&stale_result(), Some(&options), STAMP).output,
            format!("H\n\nI\n\n{STALE_ROWS}")
        );
        let message = only_stale(&json!({
            "staleBaselineHeader": "H", "collapseStaleBaselineMessage": "M", "staleBaselineIntro": "I",
        }));
        assert_eq!(
            render(&stale_result(), Some(&message), STAMP).output,
            format!("H\n\n<details><summary>M</summary>\n\nI\n\n{STALE_ROWS}</details>\n\n")
        );
        let off = only_stale(&json!({ "showStaleBaselineDetails": false }));
        assert_eq!(render(&stale_result(), Some(&off), STAMP).output, "");
    }

    #[test]
    fn no_stale_entry_or_no_baseline_prints_no_table() {
        assert_eq!(
            stale_baseline_details(&json!({ "violations": [] }), &defaults(), false),
            ""
        );
        let summary = json!({ "violations": [], "baselineStale": 0,
            "optionsUsed": { "knownViolations": [] } });
        assert_eq!(stale_baseline_details(&summary, &defaults(), false), "");
        let no_list = json!({ "violations": [], "baselineStale": 0 });
        assert_eq!(stale_baseline_details(&no_list, &defaults(), true), "");
    }

    #[test]
    fn a_result_saved_before_the_counts_existed_prints_no_table() {
        let mut saved = stale_result();
        saved["summary"]
            .as_object_mut()
            .map(|summary| summary.remove("baselineStale"));
        assert_eq!(
            render(&saved, Some(&only_stale(&json!({}))), STAMP).output,
            ""
        );
    }

    /// The two tests of upstream's 18.7.0 spec that set a stale option and no `baselineStale`.
    #[test]
    fn a_stale_option_prints_the_table_without_the_count() {
        let mut saved = stale_result();
        saved["summary"]
            .as_object_mut()
            .map(|summary| summary.remove("baselineStale"));
        for (key, value) in [
            ("staleBaselineHeader", json!("H")),
            ("collapseStaleBaseline", json!(false)),
            ("collapseStaleBaselineMessage", json!("M")),
            ("staleBaselineIntro", json!("I")),
            ("showStaleBaselineDetails", json!(true)),
        ] {
            let options = only_stale(&json!({ key: value }));
            let out = render(&saved, Some(&options), STAMP).output;
            assert!(out.contains(STALE_ROWS), "{key}: {out}");
        }
    }

    /// Upstream's `diffViolationArrays` sorts what it returns, so the table does not depend on
    /// the order of the file; two runs print the same bytes.
    #[test]
    fn the_stale_table_is_in_upstreams_order_whatever_the_files_order() {
        let entry = |rule: &str, from: &str, severity: &str| json!({ "from": from, "to": "x", "rule": { "name": rule, "severity": severity } });
        let (a, b, c) = (
            entry("no-a", "z", "warn"),
            entry("no-b", "a", "error"),
            entry("no-b", "b", "info"),
        );
        let table = |known: Value| {
            let result = json!({ "summary": { "baselineStale": 3, "violations": [],
                "optionsUsed": { "knownViolations": known } } });
            render(&result, Some(&only_stale(&json!({}))), STAMP).output
        };
        let forward = table(json!([a, b, c]));
        assert_eq!(forward, table(json!([c, a, b])));
        assert_eq!(forward, table(json!([a, b, c])));
        let at = |needle: &str| forward.find(needle).unwrap_or(usize::MAX);
        assert!(at("_no-a_|z|") < at("_no-b_|a|"), "{forward}");
        assert!(at("_no-b_|a|") < at("_no-b_|b|"), "{forward}");
    }

    #[test]
    fn the_stats_line_counts_stale_entries_only_when_there_are_some() {
        assert!(
            stats_summary(&json!({ "baselineStale": 2 }))
                .ends_with("ignored&nbsp;&nbsp;&nbsp;&nbsp;**2** stale entries in baseline\n")
        );
        assert!(stats_summary(&json!({ "baselineStale": 0 })).ends_with("ignored\n"));
        assert!(stats_summary(&json!({})).ends_with("ignored\n"));
    }

    fn result() -> Value {
        json!({
            "modules": [],
            "summary": {
                "totalCruised": 12, "totalDependenciesCruised": 30,
                "error": 1, "warn": 1, "info": 0, "ignore": 1,
                "violations": [
                    { "from": "src/a.js", "to": "src/b.js", "type": "dependency",
                      "rule": { "name": "no-b", "severity": "error" } },
                    { "from": "src/c.js", "to": "src/c.js", "type": "module",
                      "rule": { "name": "no-orphans", "severity": "warn" } },
                    { "from": "src/d.js", "to": "node_modules/x/index.js", "unresolvedTo": "x",
                      "dependencyTypes": ["npm"], "type": "dependency",
                      "rule": { "name": "no-x", "severity": "ignore" } },
                    { "from": "src/e.js", "to": "src/f.js", "type": "instability",
                      "metrics": { "from": { "instability": 0.75 }, "to": { "instability": 0.8 } },
                      "rule": { "name": "SDP", "severity": "info" } },
                    { "from": "src/g.js", "to": "src/g.js", "type": "cycle",
                      "cycle": [{ "name": "src/h.js" }, { "name": "src/g.js" }],
                      "rule": { "name": "no-cycles", "severity": "error" } },
                ],
                "ruleSetUsed": {
                    "forbidden": [
                        { "name": "no-b", "severity": "error", "comment": "b is off limits" },
                        { "name": "no-orphans", "severity": "warn", "comment": "orphans" },
                        { "name": "no-x", "severity": "ignore", "comment": "x" },
                        { "name": "SDP", "severity": "info", "comment": "stable" },
                        { "name": "no-cycles", "severity": "error", "comment": "cycles" },
                        { "name": "unviolated", "severity": "error", "comment": "never" },
                    ]
                }
            }
        })
    }

    fn render_with(options: &Value) -> String {
        render(&result(), Some(options), STAMP).output
    }

    #[test]
    fn the_default_report_byte_for_byte() {
        let expected = "## Forbidden dependency check - results\n\n\
### :chart_with_upwards_trend: Summary\n\n\
**12** modules&nbsp;&nbsp;&nbsp;&nbsp;**30** dependencies&nbsp;&nbsp;&nbsp;&nbsp;**1** errors&nbsp;&nbsp;&nbsp;&nbsp;**1** warnings&nbsp;&nbsp;&nbsp;&nbsp;**0** informational&nbsp;&nbsp;&nbsp;&nbsp;**1** ignored\n\n\n\
|rule|violations|ignored|explanation\n|:---|:---:|:---:|:---|\n\
|:exclamation:&nbsp;_no-b_|**1**|**0**|b is off limits|\n\
|:exclamation:&nbsp;_no-cycles_|**1**|**0**|cycles|\n\
|:warning:&nbsp;_no-orphans_|**1**|**0**|orphans|\n\
|:grey_exclamation:&nbsp;_SDP_|**1**|**0**|stable|\n\
|:see_no_evil:&nbsp;_no-x_|**0**|**1**|x|\n\n\n\
### :fire: All violations\n\n\
<details><summary>Violations found - click to expand</summary>\n\n\
|violated rule|module|to|\n|:---|:---|:---|\n\
|:exclamation:&nbsp;_no-b_|src/a.js|src/b.js|\n\
|:warning:&nbsp;_no-orphans_|src/c.js||\n\
|:see_no_evil:&nbsp;_no-x_|src/d.js|node_modules/x/index.js|\n\
|:grey_exclamation:&nbsp;_SDP_|src/e.js&nbsp;<span class=\"extra\">(I: 75%)</span>|src/f.js&nbsp;<span class=\"extra\">(I: 80%)</span>|\n\
|:exclamation:&nbsp;_no-cycles_|src/g.js|src/h.js &rightarrow;<br/>src/g.js|\n\n\n\
</details>\n\n\
---\n[dependency-cruiser@18.2.0](https://www.github.com/sverweij/dependency-cruiser) / 2026-09-27T10:00:00.000Z\n\n";
        let rendered = render(&result(), None, STAMP);
        assert_eq!(rendered.output, expected);
        assert_eq!(rendered.exit_code, 0);
        // Deterministic: the same input renders byte for byte the same.
        assert_eq!(render(&result(), None, STAMP), rendered);
    }

    #[test]
    fn every_option_changes_what_it_names() {
        let base = render_with(&json!({}));
        // Each flag off removes its part.
        for (key, gone) in [
            ("showTitle", "## Forbidden"),
            ("showSummary", "### :chart_with_upwards_trend:"),
            ("showSummaryHeader", "### :chart_with_upwards_trend:"),
            ("showRulesSummary", "|rule|violations|"),
            ("showDetails", "|violated rule|"),
            ("showDetailsHeader", "### :fire:"),
            ("collapseDetails", "<details>"),
            ("showFooter", "[dependency-cruiser@"),
        ] {
            assert!(base.contains(gone), "{key}");
            let off = render_with(&json!({ key: false }));
            assert!(!off.contains(gone), "{key}: {off}");
        }
        // Each text replaces its default.
        for (key, default) in [
            ("title", "## Forbidden dependency check - results"),
            ("summaryHeader", "### :chart_with_upwards_trend: Summary"),
            ("detailsHeader", "### :fire: All violations"),
            ("collapsedMessage", "Violations found - click to expand"),
            ("footer", "---\n[dependency-cruiser@18.2.0]"),
        ] {
            let custom = render_with(&json!({ key: "Aap noot mies" }));
            assert!(
                custom.contains("Aap noot mies") && !custom.contains(default),
                "{key}"
            );
        }
        // The ignored rows go with their flags.
        let hidden = render_with(
            &json!({ "includeIgnoredInSummary": false, "includeIgnoredInDetails": false }),
        );
        assert!(!hidden.contains("_no-x_"), "{hidden}");
        assert!(
            render_with(&json!({ "includeIgnoredInSummary": false }))
                .contains("|:see_no_evil:&nbsp;_no-x_|src/d.js|")
        );
        assert!(
            render_with(&json!({ "includeIgnoredInDetails": false }))
                .contains("|:see_no_evil:&nbsp;_no-x_|**0**|**1**|x|")
        );
        // The unresolved name for an external module when asked.
        let external = render_with(&json!({ "showExternalModulesUnresolved": true }));
        assert!(external.contains("|src/d.js|x|"), "{external}");
        assert!(
            render_with(&json!({ "showAliasedModulesUnresolved": true }))
                .contains("node_modules/x/index.js")
        );
        // showStatsSummary is not read upstream, so the line stays.
        assert_eq!(render_with(&json!({ "showStatsSummary": false })), base);
        // A null replaces its default, as the spread does.
        assert!(render_with(&json!({ "title": null })).starts_with("null\n\n"));
    }

    #[test]
    fn no_violations_prints_the_message() {
        let fine = json!({ "summary": { "violations": [], "totalCruised": 1 } });
        let out = render(&fine, None, STAMP).output;
        assert!(
            out.contains(
                ":revolving_hearts: No violations found. Get gummy bears to celebrate.\n\n"
            )
        );
        assert!(out.contains("**1** modules&nbsp;&nbsp;&nbsp;&nbsp;**undefined** dependencies"));
        assert!(!out.contains("|rule|"));
        let custom = render(&fine, Some(&json!({ "noViolationsMessage": "ok" })), STAMP).output;
        assert!(custom.contains("\n\nok\n\n"));
    }

    #[test]
    fn severities_have_icons() {
        for (severity, icon) in [
            (json!("error"), ":exclamation:"),
            (json!("warn"), ":warning:"),
            (json!("info"), ":grey_exclamation:"),
            (json!("ignore"), ":see_no_evil:"),
            (json!(3), ":warning:"),
        ] {
            assert_eq!(severity_icon(Some(&severity)), icon);
        }
        assert_eq!(severity_icon(None), ":warning:");
        assert_eq!(defaults().len(), 23);
        assert_eq!(
            merge_options(None, STAMP)["footer"],
            json!(default_footer(STAMP))
        );
    }
}
