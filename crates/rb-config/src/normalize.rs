//! Validation of a canonical configuration's keys, and dependency-cruiser's rule-set
//! normalisation.
//!
//! - Decisions: [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md),
//!   [ADR-0016](../../../docs/adr/0016-linear-time-regex-and-strict-compat.md)
//! - Plan: [Wave 1, Step 1](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-1-config-model-and-the-two-front-ends-1a),
//!   [Step 3](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-3-extends-presets-defines-captures-regex-1a)
//! - Requirements: [FR-CFG-01](../../../docs/prd.md#fr-cfg-01), [FR-RULE-10](../../../docs/prd.md#fr-rule-10)
//!
//! **Keys.** A key dependency-cruiser's configuration schema does not define is an error, as it
//! is in dependency-cruiser (a typo must not quietly widen a rule). The native metadata keys
//! (`fix`, `examples`, `owner`, `expires`, `allowEmpty`) are legal in both formats; in a
//! `.dependency-cruiser.*` file each is a warning that the file no longer runs on
//! dependency-cruiser, and an error under `--strict-compat`.
//!
//! **Normalisation** is `normalizeRuleSet` from dependency-cruiser 18.2.0: `severity` defaults to
//! `warn`, `name` to `unnamed`, `scope` to `module`; arrays of patterns are joined with `|`;
//! `viaNot` becomes `viaOnly.pathNot` and `viaSomeNot` becomes `via.pathNot`; `allowed` rules are
//! all named `not-in-allowed`; an `allowedSeverity` of `ignore` drops the `allowed` list; rules
//! of severity `ignore` are dropped.

use serde_json::{Map, Value};

use rb_model::Severity;
use rb_model::options::Patterns;

use crate::ConfigError;
use crate::model::{
    CompatMode, ConfigWarning, DependencyRules, Rule, Scope, ToRestriction, ViaRestriction,
};
use crate::pattern::{self, Safety};

/// Keys of a dependency rule in dependency-cruiser's schema.
const RULE_KEYS: &[&str] = &[
    "name", "severity", "comment", "scope", "from", "to", "module",
];
/// The native metadata keys.
pub const NATIVE_RULE_KEYS: &[&str] = &["fix", "examples", "owner", "expires", "allowEmpty"];
const FROM_KEYS: &[&str] = &["path", "pathNot", "orphan"];
const TO_KEYS: &[&str] = &[
    "path",
    "pathNot",
    "couldNotResolve",
    "circular",
    "dynamic",
    "exoticallyRequired",
    "exoticRequire",
    "exoticRequireNot",
    "preCompilationOnly",
    "dependencyTypes",
    "dependencyTypesNot",
    "moreThanOneDependencyType",
    "license",
    "licenseNot",
    "via",
    "viaOnly",
    "viaNot",
    "viaSomeNot",
    "moreUnstable",
    "ancestor",
    "reachable",
];
const MODULE_KEYS: &[&str] = &[
    "path",
    "pathNot",
    "numberOfDependentsLessThan",
    "numberOfDependentsMoreThan",
];
const VIA_KEYS: &[&str] = &["path", "pathNot", "dependencyTypes", "dependencyTypesNot"];
/// The top-level keys of a canonical configuration.
const CANONICAL_KEYS: &[&str] = &[
    "$schema",
    "extends",
    "forbidden",
    "allowed",
    "allowedSeverity",
    "required",
    "options",
    "defines",
    "languages",
    "ratchets",
    "layers",
    "independence",
];
/// The native top-level additions, a warning in a dependency-cruiser file.
const NATIVE_TOP_KEYS: &[&str] = &["defines", "languages", "ratchets", "layers", "independence"];

/// Every option key dependency-cruiser 18.2.0's schema defines, plus the command-line-only keys
/// that reach `optionsUsed` (`focusDepth`, `outputType`, `outputTo`, `rulesFile`, `validate`,
/// `args`), plus `baseline`, which dependency-cruiser releases after 18.2.0 accept and its own
/// repository's configuration uses (a wave 2 row of the coverage tab; recorded, not applied).
pub const OPTION_KEYS: &[&str] = &[
    "affected",
    "babelConfig",
    "baseDir",
    "baseline",
    "builtInModules",
    "cache",
    "collapse",
    "combinedDependencies",
    "detectJSDocImports",
    "detectProcessBuiltinModuleCalls",
    "doNotFollow",
    "enhancedResolveOptions",
    "exclude",
    "exoticRequireStrings",
    "experimentalStats",
    "externalModuleResolutionStrategy",
    "extraExtensionsToScan",
    "focus",
    "focusDepth",
    "forceDeriveDependents",
    "highlight",
    "includeOnly",
    "knownViolations",
    "maxDepth",
    "metrics",
    "moduleSystems",
    "outputTo",
    "outputType",
    "parser",
    "prefix",
    "preserveSymlinks",
    "progress",
    "reaches",
    "reporterOptions",
    "rulesFile",
    "skipAnalysisNotInRules",
    "suffix",
    "tsConfig",
    "tsPreCompilationDeps",
    "validate",
    "webpackConfig",
    "args",
];

/// What the key check found.
#[derive(Debug, Default)]
pub struct KeyCheck {
    /// Warnings to report.
    pub warnings: Vec<ConfigWarning>,
}

fn unknown(at: &str, key: &str, allowed: &[&str]) -> ConfigError {
    ConfigError::Invalid(format!(
        "`{at}.{key}` is not a key dependency-cruiser or Rulebearing defines; the keys here are {}",
        allowed.join(", ")
    ))
}

fn check_object(value: &Value, at: &str, allowed: &[&str]) -> Result<(), ConfigError> {
    match value {
        Value::Object(map) => {
            for key in map.keys() {
                if !allowed.contains(&key.as_str()) {
                    return Err(unknown(at, key, allowed));
                }
            }
            Ok(())
        }
        Value::Null => Ok(()),
        _ => Err(ConfigError::Invalid(format!("`{at}` must be an object"))),
    }
}

fn check_rule(
    rule: &Value,
    at: &str,
    compat: CompatMode,
    strict: bool,
    out: &mut KeyCheck,
) -> Result<(), ConfigError> {
    let Value::Object(map) = rule else {
        return Err(ConfigError::Invalid(format!("`{at}` must be an object")));
    };
    let name = map.get("name").and_then(Value::as_str).unwrap_or("unnamed");
    for key in map.keys() {
        if NATIVE_RULE_KEYS.contains(&key.as_str()) {
            if compat == CompatMode::DependencyCruiser {
                let message = format!(
                    "`{key}` is a Rulebearing addition; dependency-cruiser will refuse this file"
                );
                if strict {
                    return Err(ConfigError::Strict {
                        rule: name.to_owned(),
                        message,
                    });
                }
                out.warnings.push(ConfigWarning::about(name, message));
            }
        } else if !RULE_KEYS.contains(&key.as_str()) {
            let mut all = RULE_KEYS.to_vec();
            all.extend_from_slice(NATIVE_RULE_KEYS);
            return Err(unknown(at, key, &all));
        }
    }
    if let Some(from) = map.get("from") {
        check_object(from, &format!("{at}.from"), FROM_KEYS)?;
    }
    if let Some(module) = map.get("module") {
        check_object(module, &format!("{at}.module"), MODULE_KEYS)?;
    }
    if let Some(to) = map.get("to") {
        check_object(to, &format!("{at}.to"), TO_KEYS)?;
        for via in ["via", "viaOnly"] {
            if let Some(value @ Value::Object(_)) = to.get(via) {
                check_object(value, &format!("{at}.to.{via}"), VIA_KEYS)?;
            }
        }
    }
    Ok(())
}

/// Checks every key of a canonical configuration.
///
/// # Errors
/// [`ConfigError::Invalid`] for an unknown key; [`ConfigError::Strict`] for a native addition
/// in a dependency-cruiser file under `--strict-compat`.
pub fn check_keys(
    canonical: &Map<String, Value>,
    compat: CompatMode,
    strict: bool,
) -> Result<KeyCheck, ConfigError> {
    let mut out = KeyCheck::default();
    for key in canonical.keys() {
        if !CANONICAL_KEYS.contains(&key.as_str()) {
            return Err(unknown("config", key, CANONICAL_KEYS));
        }
        if compat == CompatMode::DependencyCruiser && NATIVE_TOP_KEYS.contains(&key.as_str()) {
            let message = format!(
                "`{key}` is a Rulebearing addition; dependency-cruiser will refuse this file"
            );
            if strict {
                return Err(ConfigError::Strict {
                    rule: String::new(),
                    message,
                });
            }
            out.warnings.push(ConfigWarning::general(message));
        }
    }
    for list in ["forbidden", "allowed", "required"] {
        match canonical.get(list) {
            None | Some(Value::Null) => {}
            Some(Value::Array(rules)) => {
                for (index, rule) in rules.iter().enumerate() {
                    check_rule(rule, &format!("{list}[{index}]"), compat, strict, &mut out)?;
                }
            }
            Some(_) => return Err(ConfigError::Invalid(format!("`{list}` must be an array"))),
        }
    }
    if let Some(options) = canonical.get("options") {
        check_object(options, "options", OPTION_KEYS)?;
        if options.get("baseline").is_some() {
            out.warnings.push(ConfigWarning::general(
                "options.baseline is recorded but not applied: baseline modes arrive in wave 2",
            ));
        }
    }
    Ok(out)
}

/// Joins a pattern list the way dependency-cruiser's `normalizeToREAsString` does.
fn joined(patterns: &mut Option<Patterns>) {
    if let Some(Patterns::Many(many)) = patterns {
        *patterns = Some(Patterns::One(many.join("|")));
    }
}

/// One pattern, joining a list.
fn one(patterns: &Patterns) -> Patterns {
    Patterns::One(patterns.joined())
}

fn normalise_via(via: &mut Option<ViaRestriction>) {
    if let Some(via) = via {
        joined(&mut via.path);
        joined(&mut via.path_not);
    }
}

/// `normalizeVias`: folds `viaNot` into `viaOnly.pathNot` and `viaSomeNot` into `via.pathNot`.
fn normalise_to(to: &mut ToRestriction) {
    normalise_via(&mut to.via);
    normalise_via(&mut to.via_only);
    if let Some(via_not) = to.via_not.take().as_ref().map(one) {
        let only = to.via_only.get_or_insert_with(ViaRestriction::default);
        if only.path_not.is_none() {
            only.path_not = Some(via_not);
        }
    }
    if let Some(some_not) = to.via_some_not.take().as_ref().map(one) {
        let via = to.via.get_or_insert_with(ViaRestriction::default);
        if via.path_not.is_none() {
            via.path_not = Some(some_not);
        }
    }
    for patterns in [
        &mut to.path,
        &mut to.path_not,
        &mut to.license,
        &mut to.license_not,
        &mut to.exotic_require,
        &mut to.exotic_require_not,
    ] {
        joined(patterns);
    }
}

fn normalise_rule(rule: &mut Rule) {
    rule.meta.severity = Some(rule.severity());
    rule.meta.name = Some(rule.name().to_owned());
    rule.scope = Some(rule.scope.unwrap_or(Scope::Module));
    joined(&mut rule.from.path);
    joined(&mut rule.from.path_not);
    normalise_to(&mut rule.to);
    if let Some(module) = &mut rule.module {
        joined(&mut module.path);
        joined(&mut module.path_not);
    }
}

fn rules_of(canonical: &Map<String, Value>, key: &str) -> Result<Vec<Rule>, ConfigError> {
    let Some(list) = canonical.get(key) else {
        return Ok(Vec::new());
    };
    let rules: Vec<Value> = match list {
        Value::Array(rules) => rules.clone(),
        Value::Null => Vec::new(),
        _ => return Err(ConfigError::Invalid(format!("`{key}` must be an array"))),
    };
    rules
        .into_iter()
        .enumerate()
        .map(|(index, rule)| {
            serde_json::from_value(rule)
                .map_err(|e| ConfigError::Invalid(format!("`{key}[{index}]`: {e}")))
        })
        .collect()
}

/// Reads and normalises the rule set of a canonical configuration.
///
/// # Errors
/// [`ConfigError::Invalid`] for a rule whose values have the wrong type.
pub fn rule_set(canonical: &Map<String, Value>) -> Result<DependencyRules, ConfigError> {
    let mut forbidden = rules_of(canonical, "forbidden")?;
    let mut required = rules_of(canonical, "required")?;
    let mut allowed = rules_of(canonical, "allowed")?;
    let mut allowed_severity = None;
    if canonical.contains_key("allowed") {
        let severity = canonical
            .get("allowedSeverity")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<Severity>().ok())
            .unwrap_or(Severity::Warn);
        if severity == Severity::Ignore {
            allowed.clear();
        } else {
            allowed_severity = Some(severity);
            for rule in &mut allowed {
                rule.meta.name = Some("not-in-allowed".to_owned());
                joined(&mut rule.from.path);
                joined(&mut rule.from.path_not);
                normalise_to(&mut rule.to);
            }
        }
    }
    for rule in forbidden.iter_mut().chain(required.iter_mut()) {
        normalise_rule(rule);
    }
    forbidden.retain(|r| r.severity() != Severity::Ignore);
    required.retain(|r| r.severity() != Severity::Ignore);
    Ok(DependencyRules {
        forbidden,
        allowed,
        allowed_severity,
        required,
    })
}

/// Every pattern of a rule with where it sits, for compilation and the safety check.
pub fn rule_patterns(rule: &Rule) -> Vec<(&'static str, &str)> {
    fn add<'a>(out: &mut Vec<(&'static str, &'a str)>, at: &'static str, p: Option<&'a Patterns>) {
        if let Some(p) = p {
            for one in p.as_slice() {
                out.push((at, one.as_str()));
            }
        }
    }
    let mut out = Vec::new();
    add(&mut out, "from.path", rule.from.path.as_ref());
    add(&mut out, "from.pathNot", rule.from.path_not.as_ref());
    add(&mut out, "to.path", rule.to.path.as_ref());
    add(&mut out, "to.pathNot", rule.to.path_not.as_ref());
    add(&mut out, "to.license", rule.to.license.as_ref());
    add(&mut out, "to.licenseNot", rule.to.license_not.as_ref());
    add(
        &mut out,
        "to.exoticRequire",
        rule.to.exotic_require.as_ref(),
    );
    add(
        &mut out,
        "to.exoticRequireNot",
        rule.to.exotic_require_not.as_ref(),
    );
    add(&mut out, "to.viaNot", rule.to.via_not.as_ref());
    add(&mut out, "to.viaSomeNot", rule.to.via_some_not.as_ref());
    if let Some(via) = &rule.to.via {
        add(&mut out, "to.via.path", via.path.as_ref());
        add(&mut out, "to.via.pathNot", via.path_not.as_ref());
    }
    if let Some(via) = &rule.to.via_only {
        add(&mut out, "to.viaOnly.path", via.path.as_ref());
        add(&mut out, "to.viaOnly.pathNot", via.path_not.as_ref());
    }
    if let Some(module) = &rule.module {
        add(&mut out, "module.path", module.path.as_ref());
        add(&mut out, "module.pathNot", module.path_not.as_ref());
    }
    out
}

/// Compiles every pattern of every rule, and applies safe-regex's check.
///
/// # Errors
/// [`ConfigError::Pattern`] for a pattern the engine cannot run; [`ConfigError::Strict`] for a
/// pattern safe-regex rejects, under `--strict-compat`.
pub fn check_patterns(
    rules: &DependencyRules,
    strict: bool,
) -> Result<Vec<ConfigWarning>, ConfigError> {
    let mut warnings = Vec::new();
    let all = rules
        .forbidden
        .iter()
        .chain(&rules.allowed)
        .chain(&rules.required);
    for rule in all {
        for (at, text) in rule_patterns(rule) {
            pattern::matcher(text).map_err(|source| ConfigError::Pattern {
                rule: rule.name().to_owned(),
                at: at.to_owned(),
                source,
            })?;
            if pattern::safety(text, pattern::RULE_REPETITION_LIMIT) != Safety::Safe {
                let message = format!(
                    "{at} `{text}` has a nested quantifier that dependency-cruiser's safe-regex refuses; Rulebearing's linear-time engine runs it, but the file no longer runs on dependency-cruiser"
                );
                if strict {
                    return Err(ConfigError::Strict {
                        rule: rule.name().to_owned(),
                        message,
                    });
                }
                warnings.push(ConfigWarning::about(rule.name(), message));
            }
        }
        if rule.to.more_unstable.is_some() {
            warnings.push(ConfigWarning::about(
                rule.name(),
                "to.moreUnstable requires `metrics`, which arrives in wave 2; until then the restriction never matches",
            ));
        }
        if rule.to.license.is_some() || rule.to.license_not.is_some() {
            warnings.push(ConfigWarning::about(
                rule.name(),
                "to.license and to.licenseNot read licences from installed packages, which arrives in wave 2; until then the restriction never matches",
            ));
        }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            _ => Map::new(),
        }
    }

    #[test]
    fn unknown_keys_are_errors_at_every_level() {
        for (value, needle) in [
            (json!({ "bogus": 1 }), "config.bogus"),
            (
                json!({ "forbidden": [{ "title": "x" }] }),
                "forbidden[0].title",
            ),
            (
                json!({ "forbidden": [{ "from": { "pth": "x" } }] }),
                "forbidden[0].from.pth",
            ),
            (
                json!({ "allowed": [{ "to": { "pathnot": "x" } }] }),
                "allowed[0].to.pathnot",
            ),
            (
                json!({ "required": [{ "module": { "x": 1 } }] }),
                "required[0].module.x",
            ),
            (
                json!({ "forbidden": [{ "to": { "via": { "p": 1 } } }] }),
                "to.via.p",
            ),
            (json!({ "options": { "tsconfig": {} } }), "options.tsconfig"),
            (json!({ "forbidden": {} }), "must be an array"),
            (json!({ "forbidden": [1] }), "must be an object"),
            (json!({ "options": [] }), "must be an object"),
        ] {
            let error = check_keys(&object(value), CompatMode::Native, false)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(error.contains(needle), "{needle}: {error}");
        }
    }

    #[test]
    fn native_additions_warn_in_a_dependency_cruiser_file() -> Result<(), ConfigError> {
        let config = object(json!({ "forbidden": [{ "name": "r", "fix": "f" }], "ratchets": [] }));
        let check = check_keys(&config, CompatMode::DependencyCruiser, false)?;
        assert_eq!(check.warnings.len(), 2);
        assert!(
            check_keys(&config, CompatMode::Native, false)?
                .warnings
                .is_empty()
        );
        assert!(matches!(
            check_keys(&config, CompatMode::DependencyCruiser, true),
            Err(ConfigError::Strict { .. })
        ));
        let top = object(json!({ "ratchets": [] }));
        assert!(matches!(
            check_keys(&top, CompatMode::DependencyCruiser, true),
            Err(ConfigError::Strict { .. })
        ));
        Ok(())
    }

    #[test]
    fn normalisation_is_dependency_cruisers() -> Result<(), ConfigError> {
        let config = object(json!({
            "forbidden": [
                { "from": { "path": ["a", "b"] }, "to": { "viaNot": ["x", "y"], "viaSomeNot": "z" } },
                { "name": "gone", "severity": "ignore", "from": {}, "to": {} }
            ],
            "allowed": [{ "from": {}, "to": { "via": ["p", "q"] } }],
            "required": [{ "module": { "path": ["m", "n"] }, "to": { "path": "t" } }]
        }));
        let rules = rule_set(&config)?;
        assert_eq!(rules.forbidden.len(), 1);
        let rule = &rules.forbidden[0];
        assert_eq!(rule.name(), "unnamed");
        assert_eq!(rule.meta.severity, Some(Severity::Warn));
        assert_eq!(rule.scope, Some(Scope::Module));
        assert_eq!(rule.from.path, Some(Patterns::One("a|b".into())));
        assert_eq!(rule.to.via_not, None);
        assert_eq!(
            rule.to.via_only.as_ref().and_then(|v| v.path_not.clone()),
            Some(Patterns::One("x|y".into()))
        );
        assert_eq!(
            rule.to.via.as_ref().and_then(|v| v.path_not.clone()),
            Some(Patterns::One("z".into()))
        );
        assert_eq!(rules.allowed[0].name(), "not-in-allowed");
        assert_eq!(rules.allowed_severity, Some(Severity::Warn));
        assert_eq!(
            rules.allowed[0]
                .to
                .via
                .as_ref()
                .and_then(|v| v.path.clone()),
            Some(Patterns::One("p|q".into()))
        );
        assert_eq!(
            rules.required[0]
                .module
                .as_ref()
                .and_then(|m| m.path.clone()),
            Some(Patterns::One("m|n".into()))
        );
        Ok(())
    }

    #[test]
    fn an_explicit_via_path_not_is_kept() -> Result<(), ConfigError> {
        let config = object(json!({
            "forbidden": [{ "from": {}, "to": { "viaOnly": { "pathNot": "keep" }, "viaNot": "drop" } }]
        }));
        let rules = rule_set(&config)?;
        assert_eq!(
            rules.forbidden[0]
                .to
                .via_only
                .as_ref()
                .and_then(|v| v.path_not.clone()),
            Some(Patterns::One("keep".into()))
        );
        Ok(())
    }

    #[test]
    fn allowed_severity_ignore_drops_allowed() -> Result<(), ConfigError> {
        let config =
            object(json!({ "allowed": [{ "from": {}, "to": {} }], "allowedSeverity": "ignore" }));
        let rules = rule_set(&config)?;
        assert!(rules.allowed.is_empty());
        assert_eq!(rules.allowed_severity, None);
        let error =
            object(json!({ "allowedSeverity": "error", "allowed": [{ "from": {}, "to": {} }] }));
        assert_eq!(rule_set(&error)?.allowed_severity, Some(Severity::Error));
        Ok(())
    }

    #[test]
    fn wrong_types_are_errors() {
        for value in [
            json!({ "forbidden": [{ "severity": "loud" }] }),
            json!({ "forbidden": [{ "to": { "circular": "yes" } }] }),
            json!({ "forbidden": 3 }),
        ] {
            assert!(rule_set(&object(value)).is_err());
        }
    }

    #[test]
    fn patterns_are_compiled_and_measured() -> Result<(), ConfigError> {
        let bad = rule_set(&object(
            json!({ "forbidden": [{ "name": "look", "from": { "path": "a(?=b)" }, "to": {} }] }),
        ))?;
        let error = check_patterns(&bad, false)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("look") && error.contains("from.path"),
            "{error}"
        );
        let nested = rule_set(&object(
            json!({ "forbidden": [{ "name": "n", "from": { "path": "(a+)+" }, "to": { "moreUnstable": true, "license": "GPL" } }] }),
        ))?;
        let warnings = check_patterns(&nested, false)?;
        assert_eq!(warnings.len(), 3);
        assert!(matches!(
            check_patterns(&nested, true),
            Err(ConfigError::Strict { .. })
        ));
        let every = rule_set(&object(
            json!({ "forbidden": [{ "from": { "path": "a", "pathNot": "b" }, "to": { "path": "c", "pathNot": "d", "licenseNot": "e", "exoticRequire": "f", "exoticRequireNot": "g", "via": { "path": "h", "pathNot": "i" }, "viaOnly": { "path": "j" } }, "module": { "path": "k", "pathNot": "l" } }] }),
        ))?;
        assert_eq!(rule_patterns(&every.forbidden[0]).len(), 12);
        Ok(())
    }
}
