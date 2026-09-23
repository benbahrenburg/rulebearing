//! `rb-report`: every dependency-cruiser reporter plus `sarif`, `github-annotations`, `junit`,
//! `trx`, `agent` and `plantuml`.
//!
//! - Architecture: [`docs/architecture.md#outputs-and-ci-contract`](../../../docs/architecture.md#outputs-and-ci-contract)
//! - Decisions: [ADR-0015](../../../docs/adr/0015-stable-violation-id.md),
//!   [ADR-0021](../../../docs/adr/0021-agent-surface-cli-first.md),
//!   [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//! - Plans: [Wave 1, sub-wave 1D](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#wave-1d-reporters-fmt-exit-codes),
//!   [Wave 2, sub-wave 2E](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md),
//!   [Wave 3, sub-wave 3B](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md)
//! - Requirements: [FR-OUT-01](../../../docs/prd.md#fr-out-01) to [FR-OUT-03](../../../docs/prd.md#fr-out-03)
//! - Specification: dependency-cruiser's `test/report/<reporter>` specs, run unmodified against
//!   these reporters by conformance gate 1 layer 3
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//!
//! Reporters read the result as JSON, as dependency-cruiser's do, so a result with a missing or
//! extra field renders the way it would upstream. Wave 1 ships the reporters of
//! [`OUTPUT_TYPES`] marked wave 1; asking for a later one is a named error.

pub mod agent;
pub mod azure_devops;
pub mod csv;
pub mod err;
pub mod github_annotations;
pub mod json;
pub mod style;
pub mod teamcity;
pub mod text;

use serde_json::Value;

/// Every output type, with the wave it lands in, from
/// [coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types)
/// and [design § Reporters](../../../docs/artifacts/design.md#reporters).
pub const OUTPUT_TYPES: &[(&str, u8)] = &[
    ("err", 1),
    ("err-long", 1),
    ("err-html", 2),
    ("json", 1),
    ("text", 1),
    ("csv", 1),
    ("teamcity", 1),
    ("azure-devops", 1),
    ("github-annotations", 1),
    ("agent", 1),
    ("null", 1),
    ("dot", 2),
    ("ddot", 2),
    ("archi", 2),
    ("cdot", 2),
    ("flat", 2),
    ("fdot", 2),
    ("mermaid", 2),
    ("d2", 2),
    ("baseline", 2),
    ("metrics", 2),
    ("sarif", 2),
    ("junit", 2),
    ("trx", 2),
    ("x-dot-webpage", 3),
    ("html", 3),
    ("markdown", 3),
    ("anon", 3),
    ("plantuml", 3),
];

/// The reporters whose exit code is the error count; every other reporter exits 0, as each of
/// dependency-cruiser's reporters decides for itself
/// ([ADR-0030](../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md)).
/// `github-annotations` and `agent` are Rulebearing's, and gate.
pub const GATING: &[&str] = &[
    "err",
    "err-long",
    "null",
    "teamcity",
    "azure-devops",
    "github-annotations",
    "agent",
];

/// Whether `output_type` gates: its exit code is the error count.
pub fn gates(output_type: &str) -> bool {
    GATING.contains(&output_type)
}

/// Whether `name` is a known output type. `plugin:<path>` is always accepted syntactically and
/// resolved at run time ([coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types)).
pub fn is_output_type(name: &str) -> bool {
    name.starts_with("plugin:") || OUTPUT_TYPES.iter().any(|(n, _)| *n == name)
}

/// A reporter's output: the text and the exit code dependency-cruiser's reporter would give.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The report.
    pub output: String,
    /// The reporter's exit code: the error count for the finding reporters, 0 for the data ones.
    pub exit_code: u64,
}

/// Why a report could not be rendered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReportError {
    /// Not an output type.
    #[error("`{0}` is not a valid output type")]
    Unknown(String),
    /// An output type a later wave delivers.
    #[error(
        "the `{name}` reporter arrives in wave {wave}; use err, err-long, json, text, csv, teamcity, azure-devops, github-annotations, agent or null"
    )]
    NotYet {
        /// The type.
        name: String,
        /// The wave.
        wave: u8,
    },
}

/// What a report needs besides the result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportOptions {
    /// Colour the terminal reporters.
    pub color: bool,
    /// `--strict-schema` for `json`.
    pub strict_schema: bool,
    /// `--max-findings` for `agent`. Default 5.
    pub max_findings: Option<usize>,
    /// The timestamp `teamcity` writes, ISO 8601 without `Z`.
    pub timestamp: String,
}

/// Renders `result` as `output_type`.
///
/// # Errors
/// [`ReportError`] for an unknown type or one a later wave delivers.
pub fn render(
    output_type: &str,
    result: &Value,
    options: &ReportOptions,
) -> Result<Rendered, ReportError> {
    render_with(output_type, result, options, None)
}

/// Renders `result` as `output_type` with the reporter options given, rather than those in
/// `summary.optionsUsed.reporterOptions` (how dependency-cruiser's specs call a reporter).
///
/// # Errors
/// See [`render`].
pub fn render_with(
    output_type: &str,
    result: &Value,
    options: &ReportOptions,
    section: Option<&Value>,
) -> Result<Rendered, ReportError> {
    let reporter_options = |name: &str| {
        section.cloned().or_else(|| {
            result
                .get("summary")
                .and_then(|s| s.get("optionsUsed"))
                .and_then(|o| o.get("reporterOptions"))
                .and_then(|r| r.get(name))
                .cloned()
        })
    };
    Ok(match output_type {
        "err" | "err-long" => {
            let long = output_type == "err-long";
            let section = reporter_options(output_type);
            err::render(
                result,
                err::ErrOptions::from_reporter_options(section.as_ref(), long, options.color),
            )
        }
        "json" => json::render(result, options.strict_schema),
        "text" => {
            let highlight = reporter_options("text")
                .and_then(|t| t.get("highlightFocused").cloned())
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            text::render(result, highlight, options.color)
        }
        "csv" => csv::render(result),
        "teamcity" => teamcity::render(result, &options.timestamp),
        "azure-devops" => azure_devops::render(result),
        "github-annotations" => github_annotations::render(result),
        "agent" => agent::render(
            result,
            options.max_findings.unwrap_or(agent::DEFAULT_MAX_FINDINGS),
        ),
        "null" => Rendered {
            output: String::new(),
            exit_code: result
                .get("summary")
                .and_then(|s| s.get("error"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
        },
        other => {
            return Err(match OUTPUT_TYPES.iter().find(|(n, _)| *n == other) {
                Some((name, wave)) => ReportError::NotYet {
                    name: (*name).to_owned(),
                    wave: *wave,
                },
                None => ReportError::Unknown(other.to_owned()),
            });
        }
    })
}

/// A string field as JavaScript's template literal would print it (`undefined` when absent).
pub(crate) fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(v) => js_number(Some(v)),
        None => "undefined".into(),
    }
}

/// A value as `${value}` prints it.
pub(crate) fn js_number(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n
            .as_f64()
            .filter(|f| f.fract() == 0.0 && f.abs() < 1e21)
            .map_or_else(|| n.to_string(), |f| format!("{f:.0}")),
        Some(other) => other.to_string(),
    }
}

/// A number field, `NaN` as JavaScript's arithmetic would give it when absent.
pub(crate) fn num(value: Option<&Value>, key: &str) -> f64 {
    value
        .and_then(|v| v.get(key))
        .and_then(Value::as_f64)
        .unwrap_or(f64::NAN)
}

/// A violation's severity.
pub(crate) fn severity(violation: &Value) -> String {
    violation
        .get("rule")
        .map(|r| text(r, "severity"))
        .unwrap_or_default()
}

/// JavaScript truthiness.
pub(crate) fn truthy(value: Option<&Value>) -> bool {
    rb_rules::js::truthy(value)
}

/// `findRuleByName(ruleSetUsed, name)`.
pub(crate) fn find_rule<'a>(rule_set: Option<&'a Value>, name: &str) -> Option<&'a Value> {
    let rule_set = rule_set?;
    ["forbidden", "required"]
        .iter()
        .filter_map(|k| rule_set.get(*k).and_then(Value::as_array))
        .flatten()
        .find(|r| r.get("name").and_then(Value::as_str) == Some(name))
}

/// The line and column of the edge `from -> to`, when the extractor recorded them.
pub(crate) fn edge_position(result: &Value, from: &str, to: &str) -> Option<(u64, u64)> {
    let dependency = result
        .get("modules")?
        .as_array()?
        .iter()
        .find(|m| m.get("source").and_then(Value::as_str) == Some(from))?
        .get("dependencies")?
        .as_array()?
        .iter()
        .find(|d| d.get("resolved").and_then(Value::as_str) == Some(to))?;
    Some((
        dependency.get("line")?.as_u64()?,
        dependency.get("column")?.as_u64()?,
    ))
}

/// The decision token in a comment.
pub(crate) fn decision(comment: &str) -> Option<String> {
    let bytes = comment.as_bytes();
    let boundary = |i: usize| i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
    for (prefix, allowed) in [("adr:", 0u8), ("plan:", 1u8)] {
        for (i, _) in comment.match_indices(prefix) {
            let rest: String = comment[i + prefix.len()..]
                .chars()
                .take_while(|c| {
                    if allowed == 0 {
                        c.is_ascii_digit()
                    } else {
                        c.is_ascii_alphanumeric() || *c == '-' || *c == '_'
                    }
                })
                .collect();
            if boundary(i) && !rest.is_empty() {
                return Some(format!("{prefix}{rest}"));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn knows_every_dependency_cruiser_output_type() {
        for name in [
            "err",
            "err-long",
            "err-html",
            "json",
            "text",
            "csv",
            "teamcity",
            "azure-devops",
            "dot",
            "ddot",
            "cdot",
            "archi",
            "fdot",
            "flat",
            "x-dot-webpage",
            "mermaid",
            "d2",
            "html",
            "markdown",
            "anon",
            "baseline",
            "metrics",
            "null",
        ] {
            assert!(is_output_type(name), "{name}");
        }
        assert!(is_output_type("plugin:./my-reporter.cjs"));
        assert!(!is_output_type("pdf"));
    }

    #[test]
    fn dispatch_and_errors() {
        let result = json!({ "modules": [], "summary": { "violations": [], "error": 2, "warn": 0, "info": 0, "totalCruised": 0, "totalDependenciesCruised": 0,
                                                          "optionsUsed": { "reporterOptions": { "text": { "highlightFocused": true } } } } });
        let o = ReportOptions::default();
        for t in [
            "err",
            "err-long",
            "json",
            "text",
            "csv",
            "teamcity",
            "azure-devops",
            "github-annotations",
            "agent",
            "null",
        ] {
            assert!(render(t, &result, &o).is_ok(), "{t}");
        }
        assert_eq!(render("null", &result, &o).map(|r| r.exit_code), Ok(2));
        // The gating table agrees with what each ported reporter returns.
        for (t, wave) in OUTPUT_TYPES {
            if *wave == 1 {
                let code = render(t, &result, &o).map(|r| r.exit_code);
                assert_eq!(code, Ok(if gates(t) { 2 } else { 0 }), "{t}");
            }
        }
        assert!(!gates("json") && gates("err") && !gates("dot"));
        assert_eq!(
            render("dot", &result, &o),
            Err(ReportError::NotYet {
                name: "dot".into(),
                wave: 2
            })
        );
        assert_eq!(
            render("pdf", &result, &o),
            Err(ReportError::Unknown("pdf".into()))
        );
        assert!(
            ReportError::NotYet {
                name: "dot".into(),
                wave: 2
            }
            .to_string()
            .contains("wave 2")
        );
    }

    #[test]
    fn helpers() {
        assert_eq!(js_number(Some(&json!(3.0))), "3");
        assert_eq!(js_number(Some(&json!(0.5))), "0.5");
        assert_eq!(js_number(None), "undefined");
        assert_eq!(js_number(Some(&json!(null))), "null");
        assert_eq!(js_number(Some(&json!(true))), "true");
        assert_eq!(text(&json!({ "a": 1 }), "a"), "1");
        assert!(num(None, "x").is_nan());
        assert_eq!(decision("see adr:0003"), Some("adr:0003".into()));
        assert_eq!(decision("see plan:wave-1."), Some("plan:wave-1".into()));
        assert_eq!(decision("nope adr:"), None);
        assert_eq!(edge_position(&json!({}), "a", "b"), None);
    }
}
