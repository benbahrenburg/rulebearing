//! `rb-report`: every dependency-cruiser reporter plus `sarif`, `github-annotations`, `junit`,
//! `trx`, `agent` and `plantuml`.
//!
//! - Architecture: [`docs/architecture.md#outputs-and-ci-contract`](../../../docs/architecture.md#outputs-and-ci-contract)
//! - Decisions: [ADR-0015](../../../docs/adr/0015-stable-violation-id.md),
//!   [ADR-0021](../../../docs/adr/0021-agent-surface-cli-first.md),
//!   [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//! - Plans: [Wave 1, sub-wave 1D](../../../docs/plans/implemented/0001-wave-1-typescript-parity.md#wave-1d-reporters-fmt-exit-codes),
//!   [Wave 2, sub-wave 2E](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md),
//!   [Wave 3, sub-wave 3B](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md)
//! - Requirements: [FR-OUT-01](../../../docs/prd.md#fr-out-01) to [FR-OUT-03](../../../docs/prd.md#fr-out-03)
//! - Specification: dependency-cruiser's `test/report/<reporter>` specs, run unmodified against
//!   these reporters by conformance gate 1 layer 3
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//!
//! Reporters read the result as JSON, as dependency-cruiser's do, so a result with a missing or
//! extra field renders the way it would upstream. The reporters of [`OUTPUT_TYPES`] up to the
//! current wave are delivered; asking for a later one is a named error. Wave 2 adds `baseline`,
//! `sarif`, `junit` and `trx`, the graph reporters ([`dot`], [`mermaid`], [`d2`]), [`metrics`] and
//! [`err_html`]. Wave 3 adds [`markdown`], [`html`] (the matrix), [`anon`] and
//! [`dot_webpage`] (`x-dot-webpage`, whose page `rulebearing wrap-html` also writes).
//! Wave 3 also adds [`plantuml`], the diagram an `adhereTo` rule enforces.

pub mod agent;
pub mod anon;
pub mod azure_devops;
pub mod baseline;
pub mod catalog;
pub mod conformance;
pub mod csv;
pub mod d2;
pub mod diff;
pub mod dot;
pub mod dot_webpage;
pub mod err;
pub mod err_html;
pub mod github_annotations;
pub mod html;
pub(crate) mod js;
pub(crate) mod js_sort;
pub mod json;
pub mod junit;
pub mod markdown;
pub mod mermaid;
pub mod metrics;
pub mod plantuml;
pub mod sarif;
pub mod style;
pub mod teamcity;
pub mod text;
pub mod trx;
pub(crate) mod utl;

use serde_json::Value;

/// The output types that read the code layer (`code`): `json` and `anon` print it, `sarif`,
/// `junit` and `trx` place an element violation at its type's declaration, and `plantuml` draws
/// types. Every other reporter renders the same result with or without it, so the command line
/// hands them a result without it: on a compiled .NET graph the code layer is most of the
/// document (plan 0003, 3G, peak memory on compiled .NET graphs). A plugin reporter always gets
/// the whole result.
pub const READS_CODE: &[&str] = &["json", "anon", "sarif", "junit", "trx", "plantuml"];

/// Whether `output_type` reads the code layer ([`READS_CODE`]).
pub fn reads_code(output_type: &str) -> bool {
    READS_CODE.contains(&output_type)
}

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

/// The reporters whose output carries [`ReportOptions::timestamp`]; every other reporter's
/// output is the same at any time, which the `--cache` layer that keeps rendered output relies
/// on ([Wave 3, Step 1](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)).
/// A test renders every output type at two times and holds this list to what changes.
pub const STAMPED: &[&str] = &["err-html", "junit", "markdown", "trx", "teamcity"];

/// Whether `output_type`'s output carries the run's timestamp.
pub fn stamps(output_type: &str) -> bool {
    STAMPED.contains(&output_type)
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
    /// `plantuml` cannot draw the result with the options given.
    #[error(transparent)]
    PlantUml(#[from] plantuml::PlantUmlError),
    /// An output type a later wave delivers.
    #[error(
        "the `{name}` reporter arrives in wave {wave}; use err, err-long, err-html, json, text, csv, teamcity, azure-devops, github-annotations, agent, baseline, sarif, junit, trx, dot, ddot, archi, cdot, flat, fdot, x-dot-webpage, mermaid, d2, metrics, html, markdown, anon, plantuml or null"
    )]
    NotYet {
        /// The type.
        name: String,
        /// The wave.
        wave: u8,
    },
    /// `x-dot-webpage` could not draw the graph: GraphViz' `dot` is missing, is not GraphViz', or
    /// failed. The message is upstream's.
    #[error("{0}")]
    Graphviz(String),
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
    /// `github-annotations` and `sarif`: the run's folder relative to the repository root
    /// (`web/`), put before each path, because GitHub places an annotation or a code-scanning
    /// result by its path from the root. Empty at the root.
    pub path_prefix: String,
    /// `baseline`: the lifecycle fields `rulebearing baseline` gives each entry.
    pub baseline: baseline::Lifecycle,
    /// `collapse` given to `fmt` (or `cruise`): passed to the reporter as `collapsePattern` over its
    /// own section, as dependency-cruiser's `reportWrap` does.
    pub collapse_pattern: Option<String>,
    /// `--from` for `plantuml`: what the diagram's nodes are, over `reporterOptions.plantuml.from`.
    pub plantuml_from: Option<String>,
    /// `x-dot-webpage`: what runs GraphViz' `dot`. Without one, the reporter reports `dot` as
    /// unavailable, as upstream does on a system without it.
    pub graphviz: Option<dot_webpage::GraphvizRunner>,
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
        let own = section.cloned().or_else(|| {
            result
                .get("summary")
                .and_then(|s| s.get("optionsUsed"))
                .and_then(|o| o.get("reporterOptions"))
                .and_then(|r| r.get(name))
                .filter(|r| !r.is_null())
                .cloned()
        });
        match &options.collapse_pattern {
            // `{ ...reportOptions, collapsePattern: formatOptions.collapse }`.
            Some(pattern) => {
                let mut merged = own
                    .as_ref()
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                merged.insert("collapsePattern".into(), Value::String(pattern.clone()));
                Some(Value::Object(merged))
            }
            None => own,
        }
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
        "baseline" => baseline::render(result, &options.baseline),
        "sarif" => sarif::render(result, &options.path_prefix),
        "junit" => junit::render(result, &options.timestamp),
        "trx" => trx::render(result, &options.timestamp),
        "teamcity" => teamcity::render(result, &options.timestamp),
        "azure-devops" => azure_devops::render(result),
        "github-annotations" => github_annotations::render(result, &options.path_prefix),
        "agent" => agent::render(
            result,
            options.max_findings.unwrap_or(agent::DEFAULT_MAX_FINDINGS),
        ),
        "dot" | "ddot" | "archi" | "cdot" | "flat" | "fdot" => {
            let section = reporter_options(output_type);
            match dot::Granularity::of(output_type) {
                Some(granularity) => dot::render(result, granularity, section.as_ref()),
                None => return Err(ReportError::Unknown(output_type.to_owned())),
            }
        }
        "mermaid" => mermaid::render(result, reporter_options("mermaid").as_ref()),
        "d2" => d2::render(result),
        "plantuml" => render_plantuml(result, options, reporter_options("plantuml"))?,
        "metrics" => metrics::render(result, reporter_options("metrics").as_ref(), options.color),
        "err-html" => err_html::render(
            result,
            reporter_options("err-html").as_ref(),
            &options.timestamp,
        ),
        "markdown" => markdown::render(
            result,
            reporter_options("markdown").as_ref(),
            &options.timestamp,
        ),
        "html" => html::render(result),
        "anon" => anon::render(result, reporter_options("anon").as_ref()),
        "x-dot-webpage" => web_page(result, reporter_options("dot").as_ref(), options)?,
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

/// `x-dot-webpage` with the runner in `options`; without one, `dot` is unavailable.
fn web_page(
    result: &Value,
    section: Option<&Value>,
    options: &ReportOptions,
) -> Result<Rendered, ReportError> {
    match &options.graphviz {
        Some(runner) => dot_webpage::render(result, section, runner.0.as_ref()),
        None => Err(ReportError::Graphviz(dot_webpage::NOT_AVAILABLE.into())),
    }
}

/// `plantuml` with its section: `collapse` has already shaped the modules and is not one of the
/// diagram's options, so the `collapsePattern` the wrapper adds is taken out again.
fn render_plantuml(
    result: &Value,
    options: &ReportOptions,
    mut section: Option<Value>,
) -> Result<Rendered, ReportError> {
    if let (Some(_), Some(Value::Object(map))) = (&options.collapse_pattern, &mut section) {
        map.remove("collapsePattern");
    }
    let diagram = plantuml::PlantUmlOptions::from_reporter_options(
        section.as_ref(),
        options.plantuml_from.as_deref(),
    )?;
    Ok(plantuml::render(result, &diagram)?)
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

/// `findRuleByName(ruleSetUsed, name)`, which searches `forbidden` and `required`; then the
/// element, slice and diagram rules Rulebearing adds to `ruleSetUsed`, so a reporter prints their
/// comment and `fix` as it does a dependency rule's.
pub(crate) fn find_rule<'a>(rule_set: Option<&'a Value>, name: &str) -> Option<&'a Value> {
    let rule_set = rule_set?;
    ["forbidden", "required", "elements", "slices", "diagrams"]
        .iter()
        .filter_map(|k| rule_set.get(*k).and_then(Value::as_array))
        .flatten()
        .find(|r| r.get("name").and_then(Value::as_str) == Some(name))
}

/// The report's edges by their module, built once so a reporter that looks up an edge per
/// violation does not scan every module each time: the first module with a source, and within it
/// the first dependency on a target, as [`edge_position`] finds them.
pub(crate) struct Edges<'r> {
    by_source: std::collections::HashMap<&'r str, &'r [Value]>,
}

impl<'r> Edges<'r> {
    /// Indexes `result`'s modules.
    pub(crate) fn of(result: &'r Value) -> Self {
        let mut by_source = std::collections::HashMap::new();
        for module in result
            .get("modules")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let (Some(source), Some(dependencies)) = (
                module.get("source").and_then(Value::as_str),
                module.get("dependencies").and_then(Value::as_array),
            ) {
                by_source.entry(source).or_insert(dependencies.as_slice());
            }
        }
        Self { by_source }
    }

    /// The first dependency of `from` on `to`.
    pub(crate) fn get(&self, from: &str, to: &str) -> Option<&'r Value> {
        self.by_source
            .get(from)?
            .iter()
            .find(|d| d.get("resolved").and_then(Value::as_str) == Some(to))
    }

    /// The edge's line and column, when the extractor recorded them.
    pub(crate) fn position(&self, from: &str, to: &str) -> Option<(u64, u64)> {
        let dependency = self.get(from, to)?;
        Some((
            dependency.get("line")?.as_u64()?,
            dependency.get("column")?.as_u64()?,
        ))
    }
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

    #[test]
    fn a_rule_is_found_by_name_in_every_family() {
        let rule_set = serde_json::json!({
            "forbidden": [{ "name": "shared", "comment": "dependency" }],
            "required": [{ "name": "req" }],
            "elements": [{ "name": "sealed", "fix": "Seal it." }, { "name": "shared", "comment": "element" }],
            "slices": [{ "name": "apart" }],
            "diagrams": [{ "name": "drawn" }]
        });
        for name in ["req", "sealed", "apart", "drawn"] {
            assert_eq!(
                find_rule(Some(&rule_set), name).and_then(|r| r.get("name")),
                Some(&serde_json::json!(name)),
                "{name}"
            );
        }
        assert_eq!(
            find_rule(Some(&rule_set), "shared").and_then(|r| r.get("comment")),
            Some(&serde_json::json!("dependency")),
            "a dependency rule is found first, as upstream finds it"
        );
        assert_eq!(find_rule(Some(&rule_set), "missing"), None);
        assert_eq!(find_rule(None, "sealed"), None);
    }

    #[test]
    fn the_reporters_that_read_the_code_layer_are_output_types() {
        for output_type in READS_CODE {
            assert!(is_output_type(output_type), "{output_type}");
            assert!(reads_code(output_type));
        }
        assert!(!reads_code("err"));
        assert!(!reads_code("dot"));
        assert!(!reads_code("plugin:x.js"));
    }
    use serde_json::json;

    /// A GraphViz that is there and draws every program as `<svg/>`.
    struct Svg;

    impl dot_webpage::Graphviz for Svg {
        fn run(&self, args: &[&str], _input: Option<&str>) -> dot_webpage::Spawned {
            dot_webpage::Spawned {
                status: Some(0),
                stdout: "<svg/>".into(),
                stderr: if args == ["-V"] {
                    "dot - graphviz version 2.43.0".into()
                } else {
                    String::new()
                },
                error: None,
            }
        }
    }

    #[test]
    fn only_the_stamped_reporters_change_with_the_time() {
        let result = json!({
            "modules": [
                { "source": "a.ts", "valid": false, "dependencies": [
                    { "module": "./b", "resolved": "b.ts", "coreModule": false, "followable": true,
                      "couldNotResolve": false, "dependencyTypes": ["local"], "dynamic": false,
                      "exoticallyRequired": false, "moduleSystem": "es6", "circular": false,
                      "valid": false, "rules": [{ "name": "r", "severity": "error" }] } ] },
                { "source": "b.ts", "valid": true, "dependencies": [] }
            ],
            "summary": {
                "violations": [{ "from": "a.ts", "to": "b.ts", "rule": { "name": "r", "severity": "error" } }],
                "error": 1, "warn": 0, "info": 0, "ignore": 0, "totalCruised": 2,
                "totalDependenciesCruised": 1,
                "optionsUsed": {},
                "ruleSetUsed": { "forbidden": [{ "name": "r", "severity": "error", "from": {}, "to": {} }] }
            }
        });
        let at = |timestamp: &str| ReportOptions {
            timestamp: timestamp.to_owned(),
            ..ReportOptions::default()
        };
        let mut rendered = 0;
        for (name, _) in OUTPUT_TYPES {
            let (Ok(one), Ok(two)) = (
                render(name, &result, &at("2026-01-01T00:00:00.000")),
                render(name, &result, &at("2027-02-02T11:11:11.111")),
            ) else {
                continue;
            };
            rendered += 1;
            assert_eq!(one.output != two.output, stamps(name), "{name}");
        }
        assert!(rendered > 20, "{rendered}");
        assert!(stamps("teamcity") && !stamps("err"));
    }

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
            "plantuml",
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
        assert_eq!(
            render("baseline", &result, &o),
            Ok(Rendered {
                output: "[]\n".into(),
                exit_code: 0
            })
        );
        // The gating table agrees with what each ported reporter returns.
        for (t, wave) in OUTPUT_TYPES {
            if *wave == 1 {
                let code = render(t, &result, &o).map(|r| r.exit_code);
                assert_eq!(code, Ok(if gates(t) { 2 } else { 0 }), "{t}");
            }
        }
        assert!(!gates("json") && gates("err") && !gates("dot"));
        // The wave 2E reporters render; a wave 3 one is still a named error.
        for t in [
            "dot", "ddot", "archi", "cdot", "flat", "fdot", "mermaid", "d2", "err-html",
        ] {
            assert_eq!(render(t, &result, &o).map(|r| r.exit_code), Ok(0), "{t}");
        }
        assert_eq!(
            render("metrics", &result, &o).map(|r| r.exit_code),
            Ok(1),
            "no folders"
        );
        // The wave 3B reporters render and exit 0, as each of upstream's does.
        for t in ["markdown", "html", "anon"] {
            assert_eq!(render(t, &result, &o).map(|r| r.exit_code), Ok(0), "{t}");
            assert!(!gates(t), "{t}");
        }
        let markdown = render("markdown", &result, &o).map(|r| r.output);
        assert!(markdown.is_ok_and(|m| m.starts_with("## Forbidden dependency check")));
        // `x-dot-webpage` without a way to run `dot` says what upstream says without `dot`.
        assert_eq!(
            render("x-dot-webpage", &result, &o),
            Err(ReportError::Graphviz(dot_webpage::NOT_AVAILABLE.into()))
        );
        let svg = ReportOptions {
            graphviz: Some(dot_webpage::GraphvizRunner(std::sync::Arc::new(Svg))),
            ..ReportOptions::default()
        };
        let page = render("x-dot-webpage", &result, &svg);
        assert_eq!(page.as_ref().map(|r| r.exit_code), Ok(0), "{page:?}");
        assert!(!gates("x-dot-webpage"));
        assert_eq!(
            page.map(|r| r.output),
            Ok(dot_webpage::wrap_in_html("<svg/>"))
        );
        // Every output type up to wave 3 is delivered: none is still a named later-wave error.
        for (t, _) in OUTPUT_TYPES {
            assert!(
                !matches!(render(t, &result, &o), Err(ReportError::NotYet { .. })),
                "{t}"
            );
        }
        // `collapse` reaches the reporter as `collapsePattern` over its own section.
        let modules =
            json!({ "modules": [{ "source": "src/a/b.js", "dependencies": [] }], "summary": {} });
        let collapsed = ReportOptions {
            collapse_pattern: Some("^src/[^/]+".into()),
            ..ReportOptions::default()
        };
        let dot = render("dot", &modules, &collapsed)
            .map(|r| r.output)
            .unwrap_or_default();
        assert!(dot.contains("\"src/a\" [label=<a>"), "{dot}");
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
    fn plantuml_draws_and_names_what_it_cannot_draw() {
        let result = json!({ "modules": [], "summary": { "violations": [], "error": 0, "warn": 0, "info": 0, "totalCruised": 0, "totalDependenciesCruised": 0, "optionsUsed": {} } });
        let o = ReportOptions::default();
        assert_eq!(
            render("plantuml", &result, &o).map(|r| r.output),
            Ok("@startuml\n\nhide stereotype\n\n@enduml\n".into())
        );
        let from_types = ReportOptions {
            plantuml_from: Some("types".into()),
            ..ReportOptions::default()
        };
        assert!(
            render("plantuml", &result, &from_types)
                .is_ok_and(|r| r.output.starts_with("@startuml\n\n!include "))
        );
        let refused = render_with("plantuml", &result, &o, Some(&json!({ "Typo": true })));
        assert!(
            matches!(&refused, Err(ReportError::PlantUml(e)) if e.to_string().contains("`Typo`")),
            "{refused:?}"
        );
        // `collapse` is not one of the diagram's options and does not reach it.
        let collapsing = ReportOptions {
            collapse_pattern: Some("^src/[^/]+".into()),
            ..ReportOptions::default()
        };
        assert!(render("plantuml", &result, &collapsing).is_ok());
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

    #[test]
    fn the_edge_index_finds_what_a_scan_finds() {
        let result = json!({ "modules": [
            { "source": "a", "dependencies": [
                { "resolved": "b", "line": 3, "column": 1 },
                { "resolved": "b", "line": 9, "column": 9 },
                { "resolved": "c" }
            ] },
            { "source": "a", "dependencies": [{ "resolved": "d", "line": 1, "column": 1 }] },
            { "source": "e" }
        ] });
        let edges = Edges::of(&result);
        for (from, to) in [("a", "b"), ("a", "c"), ("a", "d"), ("e", "a"), ("x", "y")] {
            assert_eq!(
                edges.position(from, to),
                edge_position(&result, from, to),
                "{from} -> {to}"
            );
        }
        assert_eq!(edges.position("a", "b"), Some((3, 1)));
        assert!(edges.get("a", "c").is_some());
        assert!(
            edges.get("a", "d").is_none(),
            "only the first module named a is read"
        );
    }
}
