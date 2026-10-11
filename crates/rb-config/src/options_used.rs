//! `summary.optionsUsed`: dependency-cruiser's option defaults, then the configuration's options,
//! then the output the run was asked for, as upstream's `normalizeCruiseOptions` lays them.
//!
//! - Coverage: [coverage § Command line](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line)
//!   (the options that land in `optionsUsed`)
//! - Decision: [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//!   (`optionsUsed` is part of the `cruise-result` schema)
//! - Plans: [Wave 1, Step 13](../../../docs/plans/implemented/0001-wave-1-typescript-parity.md#step-13-rb-cli-cruise-fmt-exit-codes-flags-1d);
//!   [Wave 4, Step 2](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables)
//!   (moved out of `rb-cli` so the playground's report carries the same options)
//! - Requirements: [FR-CFG-01](../../../docs/prd.md#fr-cfg-01), [FR-REACH-01](../../../docs/prd.md#fr-reach-01)

use std::path::Path;

use serde_json::{Map, Value, json};

use crate::Config;

/// dependency-cruiser's option defaults, which `optionsUsed` carries; `baseDir` is the working
/// directory.
fn defaults(cwd: &str) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("baseDir".into(), json!(cwd));
    map.insert("maxDepth".into(), json!(0));
    map.insert("moduleSystems".into(), json!(["es6", "cjs", "tsd", "amd"]));
    map.insert("detectJSDocImports".into(), json!(false));
    map.insert("detectProcessBuiltinModuleCalls".into(), json!(false));
    map.insert("skipAnalysisNotInRules".into(), json!(false));
    map.insert("tsPreCompilationDeps".into(), json!(false));
    map.insert("preserveSymlinks".into(), json!(false));
    map.insert("combinedDependencies".into(), json!(false));
    map.insert(
        "externalModuleResolutionStrategy".into(),
        json!("node_modules"),
    );
    map.insert("exoticRequireStrings".into(), json!([]));
    map
}

/// The `reporterOptions` patterns dependency-cruiser's `normalizeReporterOptions` turns from an
/// array into one alternation.
const REPORTER_PATTERNS: &[&str] = &[
    "archi.collapsePattern",
    "archi.filters.includeOnly.path",
    "archi.filters.focus.path",
    "archi.filters.exclude.path",
    "dot.collapsePattern",
    "dot.filters.includeOnly.path",
    "dot.filters.focus.path",
    "dot.filters.exclude.path",
    "ddot.collapsePattern",
    "ddot.filters.includeOnly.path",
    "ddot.filters.focus.path",
    "ddot.filters.exclude.path",
];

/// `normalizeReporterOptions`: each pattern of `REPORTER_PATTERNS` given as an array becomes
/// its items joined with `|`; anything else is kept as it is.
pub fn normalize_reporter_options(reporter_options: &mut Value) {
    for path in REPORTER_PATTERNS {
        let pointer = format!("/{}", path.replace('.', "/"));
        if let Some(at) = reporter_options.pointer_mut(&pointer)
            && let Value::Array(items) = at
        {
            let joined = items
                .iter()
                .map(|i| match i {
                    Value::String(s) => s.clone(),
                    Value::Null => String::new(),
                    other => other.to_string(),
                })
                .collect::<Vec<_>>()
                .join("|");
            *at = Value::String(joined);
        }
    }
}

/// The options `summary.optionsUsed` reports for a run in `cwd`: dependency-cruiser's defaults,
/// the configuration's options (and its file, relative to `cwd`) and the output, in that order
/// of precedence.
pub fn options_used(
    config: Option<&Config>,
    cwd: &Path,
    output_type: &str,
    output_to: &str,
) -> Map<String, Value> {
    let mut map = defaults(&cwd.to_string_lossy());
    if let Some(config) = config {
        if let Ok(Value::Object(ts)) = serde_json::to_value(&config.languages.typescript) {
            map.extend(ts);
        }
        if let Ok(Value::Object(options)) = serde_json::to_value(&config.options) {
            map.extend(options);
        }
        if !config.known_violations.is_empty() {
            map.insert(
                "knownViolations".into(),
                serde_json::to_value(&config.known_violations).unwrap_or(Value::Null),
            );
        }
        if let Some(origin) = &config.origin {
            let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
            map.insert(
                "rulesFile".into(),
                json!(
                    origin
                        .strip_prefix(&cwd)
                        .unwrap_or(origin)
                        .to_string_lossy()
                ),
            );
        }
        if config.via_node {
            map.insert("viaNode".into(), json!(true));
        }
    }
    for key in ["doNotFollow", "exclude"] {
        map.entry(key).or_insert_with(|| json!({}));
    }
    if let Some(reporter_options) = map.get_mut("reporterOptions") {
        normalize_reporter_options(reporter_options);
    }
    map.insert("outputType".into(), json!(output_type));
    map.insert("outputTo".into(), json!(output_to));
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_configuration_the_defaults_and_the_output_are_used() {
        let used = options_used(None, Path::new("/repo"), "err", "-");
        assert_eq!(used["baseDir"], "/repo");
        assert_eq!(used["moduleSystems"], json!(["es6", "cjs", "tsd", "amd"]));
        assert_eq!(used["maxDepth"], 0);
        assert_eq!(used["exoticRequireStrings"], json!([]));
        assert_eq!(used["externalModuleResolutionStrategy"], "node_modules");
        assert_eq!(used["doNotFollow"], json!({}));
        assert_eq!(used["exclude"], json!({}));
        assert_eq!(
            (&used["outputType"], &used["outputTo"]),
            (&json!("err"), &json!("-"))
        );
        assert!(used.get("rulesFile").is_none());
        assert!(used.get("viaNode").is_none());
        assert!(used.get("knownViolations").is_none());
    }

    #[test]
    fn the_configuration_s_options_win_over_the_defaults() -> Result<(), crate::ConfigError> {
        let text = r#"{"options":{"maxDepth":3,"doNotFollow":{"path":"node_modules"},
            "reporterOptions":{"dot":{"collapsePattern":["a","b"]}}},
            "forbidden":[{"name":"r","from":{},"to":{}}]}"#;
        let mut config = crate::load_text(
            text,
            crate::read::Syntax::Json,
            Path::new("/"),
            &crate::LoadOptions::default(),
        )?;
        config.origin = Some(std::path::PathBuf::from("/nowhere-real/rules.json"));
        config.via_node = true;
        let used = options_used(
            Some(&config),
            Path::new("/nowhere-real"),
            "json",
            "out.json",
        );
        assert_eq!(used["maxDepth"], 3);
        assert_eq!(used["doNotFollow"], json!({ "path": "node_modules" }));
        assert_eq!(used["reporterOptions"]["dot"]["collapsePattern"], "a|b");
        assert_eq!(used["rulesFile"], "rules.json");
        assert_eq!(used["viaNode"], true);
        assert_eq!(used["outputTo"], "out.json");
        Ok(())
    }

    #[test]
    fn reporter_patterns_are_normalised() {
        let mut options = json!({
            "dot": { "collapsePattern": ["^src/[^/]+", "^lib"], "filters": { "focus": { "path": ["a", "b"] }, "reaches": { "path": ["c"] } } },
            "archi": { "collapsePattern": "^x" },
            "flat": { "collapsePattern": ["kept", "as is"] }
        });
        normalize_reporter_options(&mut options);
        assert_eq!(options["dot"]["collapsePattern"], json!("^src/[^/]+|^lib"));
        assert_eq!(options["dot"]["filters"]["focus"]["path"], json!("a|b"));
        assert_eq!(
            options["dot"]["filters"]["reaches"]["path"],
            json!(["c"]),
            "upstream leaves reaches"
        );
        assert_eq!(options["archi"]["collapsePattern"], json!("^x"));
        assert_eq!(options["flat"]["collapsePattern"], json!(["kept", "as is"]));
        let mut mixed = json!({ "ddot": { "collapsePattern": ["a", null, 1] } });
        normalize_reporter_options(&mut mixed);
        assert_eq!(mixed["ddot"]["collapsePattern"], json!("a||1"));
    }
}
