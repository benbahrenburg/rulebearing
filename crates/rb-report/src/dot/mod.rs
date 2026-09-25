//! `dot`, `ddot`, `archi` / `cdot` and `flat` / `fdot`: the result as a `GraphViz` digraph at module,
//! folder, custom (collapsed) and flat granularity. dependency-cruiser 18.2.0's
//! `src/report/dot/index.mjs`, ported.
//!
//! - Specification: `test/report/dot/**` (the four level specs, `theming.spec.mjs`,
//!   `module-utl.spec.mjs`), run unmodified by conformance gate 1 layer 3
//!   ([ADR-0009](../../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Coverage: [coverage § Output types](../../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types),
//!   row `dot`, `ddot`, `cdot` / `archi`, `fdot` / `flat`; [coverage § Options](../../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   row `reporterOptions.archi` / `dot` / `ddot` / `flat`
//! - Plan: [Wave 2, Step 10](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Requirement: [FR-OUT-01](../../../../docs/prd.md#fr-out-01)
//!
//! The reporter options are the section the caller passes (`reporterOptions.<output type>`) and,
//! for each of `theme`, `collapsePattern` and `filters` it does not carry, the granularity's own
//! section in `summary.optionsUsed.reporterOptions` (`dot`, `ddot`, `archi`, `flat`), then `dot`'s,
//! exactly as upstream's `normalizeDotReporterOptions` pries them. `archi` collapses to
//! `^(node_modules|packages|src|lib|app|test|spec)/[^/]+` when no pattern is given.

pub mod module_utl;
pub mod theme;

use std::fmt::Write as _;

use serde_json::Value;

use rb_rules::graph::filters::{Filter, Filters};

use crate::Rendered;
use crate::dot::module_utl::{Level, Prepare, prepare};
use crate::dot::theme::{attributize, normalize_theme};
use crate::js;
use crate::utl::option_text;

/// The granularity a `dot` reporter renders at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    /// `dot`: every module, clustered by folder.
    Module,
    /// `ddot`: modules consolidated to their folders.
    Folder,
    /// `archi` / `cdot`: modules collapsed by a pattern.
    Custom,
    /// `flat` / `fdot`: every module, unclustered.
    Flat,
}

impl Granularity {
    /// The granularity of an output type, if it is one of the `dot` family.
    pub fn of(output_type: &str) -> Option<Self> {
        match output_type {
            "dot" => Some(Self::Module),
            "ddot" => Some(Self::Folder),
            "archi" | "cdot" => Some(Self::Custom),
            "flat" | "fdot" => Some(Self::Flat),
            _ => None,
        }
    }

    /// `GRANULARITY2REPORTER_OPTIONS`: the section of `reporterOptions` it reads.
    const fn section(self) -> &'static str {
        match self {
            Self::Module => "dot",
            Self::Folder => "ddot",
            Self::Custom => "archi",
            Self::Flat => "flat",
        }
    }

    const fn level(self) -> Level {
        match self {
            Self::Module | Self::Custom => Level::Custom,
            Self::Folder => Level::Folder,
            Self::Flat => Level::Flat,
        }
    }
}

const ARCHI_COLLAPSE: &str = "^(node_modules|packages|src|lib|app|test|spec)/[^/]+";

/// `summary.optionsUsed.reporterOptions` read with upstream's `get`.
fn result_section(result: &Value, name: &str) -> Option<Value> {
    js::get(
        result,
        &format!("summary.optionsUsed.reporterOptions.{name}"),
    )
}

/// `pryReporterOptionsFromResults`: the granularity's section, else `dot`'s.
fn pried(granularity: Granularity, result: &Value) -> Option<Value> {
    result_section(result, granularity.section()).or_else(|| result_section(result, "dot"))
}

/// A defined (not null or undefined) value, as `??` sees it.
fn defined(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| !v.is_null())
}

/// The options the reporter renders with, after `normalizeDotReporterOptions`.
#[derive(Debug, Clone, Default, PartialEq)]
struct DotOptions {
    theme: Option<Value>,
    collapse_pattern: Option<Value>,
    filters: Option<Value>,
    show_metrics: bool,
}

fn normalize_options(
    section: Option<&Value>,
    granularity: Granularity,
    result: &Value,
) -> DotOptions {
    let own = |key: &str| {
        section
            .and_then(Value::as_object)
            .and_then(|s| s.get(key))
            .cloned()
    };
    let has = |key: &str| {
        section
            .and_then(Value::as_object)
            .is_some_and(|s| s.contains_key(key))
    };
    let pried = pried(granularity, result);
    let dot = result_section(result, "dot");
    let from_result = |key: &str| {
        defined(pried.as_ref().and_then(|p| p.get(key)))
            .or_else(|| defined(dot.as_ref().and_then(|d| d.get(key))))
            .cloned()
    };
    // `{ theme: own.theme || pried, ..., ...own }`: an own key wins whatever its value.
    let pick = |key: &str, fallback: Option<Value>| {
        if has(key) { own(key) } else { fallback }
    };
    let collapse_fallback = defined(pried.as_ref().and_then(|p| p.get("collapsePattern")))
        .cloned()
        .or_else(|| {
            (granularity == Granularity::Custom).then(|| Value::String(ARCHI_COLLAPSE.into()))
        });
    DotOptions {
        theme: pick("theme", from_result("theme")),
        collapse_pattern: pick("collapsePattern", collapse_fallback),
        filters: pick("filters", from_result("filters")),
        show_metrics: js::truthy(own("showMetrics").as_ref()),
    }
}

/// One of `filters`' entries as `applyFilters` reads it: `path` as `new RegExp` would stringify
/// it, `depth` for `focus`.
fn filter(filters: &Value, key: &str) -> Option<Filter> {
    let entry = filters.get(key).filter(|f| js::truthy(Some(f)))?;
    let path = entry.get("path").filter(|p| js::truthy(Some(p)));
    Some(Filter {
        path: path.map(|p| js::to_string(Some(p))),
        depth: entry
            .get("depth")
            .and_then(Value::as_u64)
            .and_then(|d| u32::try_from(d).ok()),
    })
}

fn apply_filters(modules: Vec<Value>, filters: &Value) -> Vec<Value> {
    let filters = Filters {
        exclude: filter(filters, "exclude"),
        include_only: filter(filters, "includeOnly"),
        focus: filter(filters, "focus"),
        reaches: filter(filters, "reaches"),
        highlight: filter(filters, "highlight"),
    };
    rb_rules::graph::filters::apply(modules, &filters)
}

fn general_attributes(theme: &Value) -> String {
    let line = |key: &str, open: &str, close: &str| match theme.get(key) {
        Some(value) if js::truthy(Some(value)) => format!(
            "    {open}{}{close}",
            attributize(&value.as_object().cloned().unwrap_or_default())
        ),
        _ => String::new(),
    };
    format!(
        "{}\n{}\n{}\n",
        line("graph", "", ""),
        line("node", "node [", "]"),
        line("edge", "edge [", "]")
    )
}

fn flat_module(module: &Value) -> String {
    let url = module
        .get("URL")
        .filter(|u| js::truthy(Some(u)))
        .map(|u| format!("URL=\"{}\" ", js::to_string(Some(u))))
        .unwrap_or_default();
    let theme = match module.get("themeAttrs") {
        None | Some(Value::Null) => String::new(),
        other => js::to_string(other),
    };
    format!(
        "\"{}\" [label={} tooltip=\"{}\" {url}{theme}]",
        js::field(module, "source"),
        js::field(module, "label"),
        js::field(module, "tooltip")
    )
}

fn hierarchy(module: &Value, clusters_have_own_node: bool) -> String {
    let path = rb_rules::js::array(module, "path");
    let clusters: Vec<String> = path
        .iter()
        .map(|p| {
            let aggregate = js::field(p, "aggregateSnippet");
            let mut cluster = format!(
                "subgraph \"cluster_{aggregate}\" {{label=\"{}\"",
                js::field(p, "snippet")
            );
            if clusters_have_own_node {
                let _ = write!(
                    cluster,
                    " \"{aggregate}\" [width=\"0.05\" shape=\"point\" style=\"invis\"]"
                );
            }
            cluster
        })
        .collect();
    format!(
        "{} {}{}",
        clusters.join(" "),
        flat_module(module),
        " }".repeat(path.len())
    )
}

fn dependency_line(source: &str, dependency: &Value) -> String {
    let mut line = format!(
        "    \"{source}\" -> \"{}\"",
        js::field(dependency, "resolved")
    );
    if js::truthy(dependency.get("hasExtraAttributes")) {
        let name = dependency.get("rule").and_then(|r| r.get("name"));
        let label = if js::truthy(name) {
            let name = js::to_string(name);
            format!("xlabel=\"{name}\" tooltip=\"{name}\" ")
        } else {
            String::new()
        };
        let theme = match dependency.get("themeAttrs") {
            None | Some(Value::Null) => String::new(),
            other => js::to_string(other),
        };
        let _ = write!(line, " [{label}{theme}]");
    }
    line
}

fn module_block(module: &Value, clusters_have_own_node: bool) -> String {
    let mut block = if js::truthy(module.get("folder")) {
        format!("    {}", hierarchy(module, clusters_have_own_node))
    } else {
        format!("    {}", flat_module(module))
    };
    let dependencies = rb_rules::js::array(module, "dependencies");
    if !dependencies.is_empty() {
        let source = js::field(module, "source");
        let lines: Vec<String> = dependencies
            .iter()
            .map(|d| dependency_line(&source, d))
            .collect();
        block.push('\n');
        block.push_str(&lines.join("\n"));
    }
    block
}

/// Renders `result` at `granularity`, with `section` as the reporter options passed in (the
/// `reporterOptions.<output type>` section, or what a caller gives).
pub fn render(result: &Value, granularity: Granularity, section: Option<&Value>) -> Rendered {
    let options = normalize_options(section, granularity, result);
    let theme = normalize_theme(options.theme.as_ref());
    let mut modules = rb_rules::js::array(result, "modules").to_vec();
    if let Some(filters) = options.filters.as_ref().filter(|f| js::truthy(Some(f))) {
        modules = apply_filters(modules, filters);
    }
    let pattern = options
        .collapse_pattern
        .as_ref()
        .filter(|p| js::truthy(Some(p)))
        .map(|p| js::to_string(Some(p)));
    let prefix = option_text(result, "prefix");
    let suffix = option_text(result, "suffix");
    let prepared = prepare(
        &modules,
        granularity.level(),
        &Prepare {
            theme: &theme,
            collapse_pattern: pattern.as_deref(),
            show_metrics: options.show_metrics,
            prefix: &prefix,
            suffix: &suffix,
        },
    );
    let clusters_have_own_node = granularity == Granularity::Folder;
    let body: Vec<String> = prepared
        .iter()
        .map(|m| module_block(m, clusters_have_own_node))
        .collect();
    Rendered {
        output: format!(
            "strict digraph \"dependency-cruiser output\"{{\n{}\n{}\n}}\n",
            general_attributes(&theme),
            body.join("\n")
        ),
        exit_code: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn result(options: &Value) -> Value {
        json!({
            "modules": [
                { "source": "src/a.js", "dependencies": [{ "resolved": "src/b.js", "rules": [{ "name": "no-b", "severity": "error" }], "dependencyTypes": ["local"] }] },
                { "source": "src/b.js", "dependencies": [], "instability": 0.5 },
                { "source": "root.js", "dependencies": [{ "resolved": "src/a.js", "dynamic": true }] }
            ],
            "summary": { "optionsUsed": options }
        })
    }

    #[test]
    fn module_level_bare() {
        let rendered = render(
            &result(&json!({})),
            Granularity::Module,
            Some(&json!({ "theme": { "replace": true } })),
        );
        assert_eq!(
            rendered.output,
            concat!(
                "strict digraph \"dependency-cruiser output\"{\n",
                "\n\n\n\n",
                "    \"root.js\" [label=<root.js> tooltip=\"root.js\" URL=\"root.js\" ]\n",
                "    \"root.js\" -> \"src/a.js\"\n",
                "    subgraph \"cluster_src\" {label=\"src\" \"src/a.js\" [label=<a.js> tooltip=\"a.js\" URL=\"src/a.js\" ] }\n",
                "    \"src/a.js\" -> \"src/b.js\" [xlabel=\"no-b\" tooltip=\"no-b\" ]\n",
                "    subgraph \"cluster_src\" {label=\"src\" \"src/b.js\" [label=<b.js> tooltip=\"b.js\" URL=\"src/b.js\" ] }\n",
                "}\n"
            )
        );
        assert_eq!(rendered.exit_code, 0);
    }

    #[test]
    fn default_theme_metrics_prefix_and_folders() {
        let options = json!({ "prefix": "https://x/", "reporterOptions": { "dot": { "theme": { "graph": { "splines": "ortho" } } } } });
        let rendered = render(
            &result(&options),
            Granularity::Module,
            Some(&json!({ "showMetrics": true })),
        );
        assert!(
            rendered
                .output
                .contains("    rankdir=\"LR\" splines=\"ortho\"")
        );
        assert!(rendered.output.contains("URL=\"https://x/src/b.js\""));
        assert!(
            rendered
                .output
                .contains("<b.js <FONT color=\"#808080\" point-size=\"8\">50%</FONT>>")
        );
        assert!(
            rendered
                .output
                .contains("[xlabel=\"no-b\" tooltip=\"no-b\" fontcolor=\"red\" color=\"red\"]")
        );
        assert!(
            rendered
                .output
                .contains("\"root.js\" -> \"src/a.js\" [style=\"dashed\"]")
        );
        let folders = render(&result(&json!({})), Granularity::Folder, None);
        assert!(folders.output.contains(
            "    \".\" [label=<.> tooltip=\".\" URL=\".\" shape=\"box3d\"]\n    \".\" -> \"src\" [style=\"dashed\"]\n    \"src\" [label=<src> tooltip=\"src\" URL=\"src\" shape=\"box3d\"]\n}\n"
        ));
        let nested =
            json!({ "modules": [{ "source": "a/b/c.js", "dependencies": [] }], "summary": {} });
        assert!(render(&nested, Granularity::Folder, None).output.contains(
            "subgraph \"cluster_a\" {label=\"a\" \"a\" [width=\"0.05\" shape=\"point\" style=\"invis\"] \"a/b\" [label=<b>"
        ));
    }

    #[test]
    fn options_pry_from_the_result() {
        let options = json!({ "reporterOptions": {
            "dot": { "filters": { "includeOnly": { "path": "^src" } }, "collapsePattern": "^src" },
            "flat": { "theme": { "replace": true } }
        } });
        let flat = normalize_options(None, Granularity::Flat, &result(&options));
        assert_eq!(flat.theme, Some(json!({ "replace": true })));
        assert_eq!(
            flat.filters,
            Some(json!({ "includeOnly": { "path": "^src" } }))
        );
        assert_eq!(
            flat.collapse_pattern, None,
            "flat has its own section, without a pattern"
        );
        let archi = normalize_options(None, Granularity::Custom, &result(&json!({})));
        assert_eq!(archi.collapse_pattern, Some(json!(ARCHI_COLLAPSE)));
        let module = normalize_options(None, Granularity::Module, &result(&options));
        assert_eq!(module.collapse_pattern, Some(json!("^src")));
        let own = normalize_options(
            Some(&json!({ "theme": null, "showMetrics": 1 })),
            Granularity::Flat,
            &result(&options),
        );
        assert_eq!(
            own.theme,
            Some(Value::Null),
            "an own key wins whatever its value"
        );
        assert!(own.show_metrics);
        let filtered = render(&result(&options), Granularity::Module, None);
        assert!(!filtered.output.contains("root.js"));
        assert!(
            filtered.output.contains("\"src\" [label=<src>"),
            "{}",
            filtered.output
        );
    }

    #[test]
    fn granularities() {
        for (t, g) in [
            ("dot", Granularity::Module),
            ("ddot", Granularity::Folder),
            ("archi", Granularity::Custom),
            ("cdot", Granularity::Custom),
            ("flat", Granularity::Flat),
            ("fdot", Granularity::Flat),
        ] {
            assert_eq!(Granularity::of(t), Some(g));
        }
        assert_eq!(Granularity::of("json"), None);
        let flat = render(&result(&json!({})), Granularity::Flat, None);
        assert!(
            flat.output
                .contains("\"src/a.js\" [label=<src/<BR/><B>a.js</B>> tooltip=\"a.js\"")
        );
        let custom = render(&result(&json!({})), Granularity::Custom, None);
        assert!(
            custom.output.contains("\"src/a.js\" [label=<a.js>"),
            "the default pattern keeps a file under src/"
        );
        let collapsed = render(
            &result(&json!({})),
            Granularity::Custom,
            Some(&json!({ "collapsePattern": "^src" })),
        );
        assert!(
            collapsed
                .output
                .contains("\"src\" [label=<src> tooltip=\"src\" URL=\"src\" shape=\"box3d\"]")
        );
    }

    #[test]
    fn filters_read_like_apply_filters() {
        let filters = json!({ "focus": { "path": ["a", "b"], "depth": 2 }, "exclude": { "path": "" }, "reaches": false });
        let focus = filter(&filters, "focus");
        assert_eq!(
            focus.as_ref().and_then(|f| f.path.clone()),
            Some("a,b".into())
        );
        assert_eq!(focus.and_then(|f| f.depth), Some(2));
        assert_eq!(filter(&filters, "exclude").map(|f| f.path), Some(None));
        assert_eq!(filter(&filters, "reaches"), None);
        assert_eq!(filter(&filters, "highlight"), None);
    }
}
