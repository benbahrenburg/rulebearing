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
//! **Cross-language keys.** `language`, `namespace`, `project`, `assembly` (each with its `Not`
//! form except `language`) on `from` and `to`, and `dependencyKind` / `dependencyKindNot` on `to`,
//! are additions a dependency-cruiser configuration never sees
//! ([design § Dependency rules](../../../docs/artifacts/design.md#dependency-rules-the-whole-of-dependency-cruiser-1820)):
//! in a `.dependency-cruiser.*` file each is an error (exit 3) with or without `--strict-compat`,
//! because dependency-cruiser would refuse the file and dropping the key would widen the rule.
//! They narrow dependency and orphan rules; on a reachability, dependents, required or
//! folder-scoped rule they are an error, since those rules select through derivations the keys
//! do not reach.
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
    CompatMode, ConfigWarning, CrossLanguageKeys, DependencyRules, Rule, Scope, ToRestriction,
    ViaRestriction,
};
use crate::pattern::{self, Safety};

/// Keys of a dependency rule in dependency-cruiser's schema.
const RULE_KEYS: &[&str] = &[
    "name", "severity", "comment", "scope", "from", "to", "module",
];
/// The native metadata keys.
pub const NATIVE_RULE_KEYS: &[&str] = &["fix", "examples", "owner", "expires", "allowEmpty"];
const FROM_KEYS: &[&str] = &["path", "pathNot", "orphan"];
/// The cross-language keys of `from` and `to`, native configurations only.
pub const CROSS_LANGUAGE_KEYS: &[&str] = &[
    "language",
    "namespace",
    "namespaceNot",
    "project",
    "projectNot",
    "assembly",
    "assemblyNot",
];
/// The cross-language keys of `to` alone: properties of the edge.
pub const EDGE_KEYS: &[&str] = &["dependencyKind", "dependencyKindNot"];
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
    "allowEmpty",
    "languages",
    "ratchets",
    "layers",
    "independence",
    "elements",
    "slices",
    "diagrams",
];
/// The native top-level additions, a warning in a dependency-cruiser file.
const NATIVE_TOP_KEYS: &[&str] = &[
    "defines",
    "allowEmpty",
    "languages",
    "ratchets",
    "layers",
    "independence",
    "elements",
    "slices",
    "diagrams",
];

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
        if let Some(key) = EDGE_KEYS.iter().find(|k| from.get(**k).is_some()) {
            return Err(ConfigError::Invalid(format!(
                "`{at}.from.{key}`: `{key}` is a property of the edge, not of the module that imports; write it under `to`"
            )));
        }
        let mut allowed = FROM_KEYS.to_vec();
        allowed.extend_from_slice(CROSS_LANGUAGE_KEYS);
        check_object(from, &format!("{at}.from"), &allowed)?;
        check_cross_language(from, &format!("{at}.from"), compat)?;
    }
    if let Some(module) = map.get("module") {
        check_object(module, &format!("{at}.module"), MODULE_KEYS)?;
    }
    if let Some(to) = map.get("to") {
        let mut allowed = TO_KEYS.to_vec();
        allowed.extend_from_slice(CROSS_LANGUAGE_KEYS);
        allowed.extend_from_slice(EDGE_KEYS);
        check_object(to, &format!("{at}.to"), &allowed)?;
        check_cross_language(to, &format!("{at}.to"), compat)?;
        for via in ["via", "viaOnly"] {
            if let Some(value @ Value::Object(_)) = to.get(via) {
                check_object(value, &format!("{at}.to.{via}"), VIA_KEYS)?;
            }
        }
    }
    Ok(())
}

/// The cross-language keys written in one side of a rule.
fn cross_language_keys(side: &Value) -> Vec<&'static str> {
    CROSS_LANGUAGE_KEYS
        .iter()
        .chain(EDGE_KEYS)
        .filter(|k| side.get(**k).is_some())
        .copied()
        .collect()
}

/// Refuses a cross-language key in a dependency-cruiser configuration.
fn check_cross_language(side: &Value, at: &str, compat: CompatMode) -> Result<(), ConfigError> {
    if compat != CompatMode::DependencyCruiser {
        return Ok(());
    }
    match cross_language_keys(side).first() {
        Some(key) => Err(ConfigError::Invalid(format!(
            "`{at}.{key}` is a Rulebearing addition for native configurations; a dependency-cruiser configuration never sees it. Move the rule into rulebearing.yaml (`rulebearing config convert` writes one) or remove the key"
        ))),
        None => Ok(()),
    }
}

/// Refuses cross-language keys on a rule whose selection they cannot narrow: reachability,
/// dependents, required and folder-scoped rules.
fn check_cross_language_placement(rule: &Rule, family: &str) -> Result<(), ConfigError> {
    let mut written: Vec<String> = rule
        .from
        .cross
        .written()
        .into_iter()
        .map(|k| format!("from.{k}"))
        .collect();
    written.extend(rule.to.additions().into_iter().map(|k| format!("to.{k}")));
    let Some(first) = written.first() else {
        return Ok(());
    };
    let kind = if rule.is_folder_scope() {
        Some(
            "a folder-scoped rule compares folders, which carry no language, namespace, project or edge kind",
        )
    } else if rule.to.reachable.is_some() {
        Some(
            "a reachability rule selects through `reachable`, which the cross-language keys do not narrow",
        )
    } else if rule.module.is_some() || family == "required" {
        Some(
            "a dependents or required rule selects through `module`, which the cross-language keys do not narrow",
        )
    } else {
        None
    };
    match kind {
        Some(why) => Err(ConfigError::Invalid(format!(
            "rule `{}`: `{first}` cannot apply here: {why}. Narrow the rule with `path` instead, or make it a dependency rule",
            rule.name()
        ))),
        None => Ok(()),
    }
}

/// The warning for a rule whose `from` or `to` can only match .NET modules and that names
/// `type-only`, which no .NET edge carries
/// ([design § Dependency rules](../../../docs/artifacts/design.md#dependency-rules-the-whole-of-dependency-cruiser-1820)).
pub fn type_only_on_dotnet(rule: &Rule) -> Option<String> {
    let dotnet = rb_model::Language::Dotnet;
    let side = if rule.from.cross.only(dotnet) {
        "from"
    } else if rule.to.cross.only(dotnet) {
        "to"
    } else {
        return None;
    };
    let names = |types: Option<&Vec<rb_model::DependencyType>>| {
        types.is_some_and(|t| t.contains(&rb_model::DependencyType::TypeOnly))
    };
    let vias = [rule.to.via.as_ref(), rule.to.via_only.as_ref()];
    let at = if names(rule.to.dependency_types.as_ref()) {
        "to.dependencyTypes"
    } else if names(rule.to.dependency_types_not.as_ref()) {
        "to.dependencyTypesNot"
    } else if vias
        .iter()
        .flatten()
        .any(|v| names(v.dependency_types.as_ref()) || names(v.dependency_types_not.as_ref()))
    {
        "to.via"
    } else {
        return None;
    };
    Some(format!(
        "{at} names `type-only`, but `{side}.language` limits the rule to .NET, where `type-only` has no meaning (a signature reference still loads the assembly) and no edge carries it; use `signature-only` for an edge whose every reference is a signature, or remove `type-only`"
    ))
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
    normalise_cross(&mut to.cross);
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

/// Joins the cross-language pattern lists as `path` lists are joined.
fn normalise_cross(cross: &mut CrossLanguageKeys) {
    for (_, patterns) in cross.patterns_mut() {
        joined(patterns);
    }
}

fn normalise_rule(rule: &mut Rule) {
    rule.meta.severity = Some(rule.severity());
    rule.meta.name = Some(rule.name().to_owned());
    rule.scope = Some(rule.scope.unwrap_or(Scope::Module));
    joined(&mut rule.from.path);
    joined(&mut rule.from.path_not);
    normalise_cross(&mut rule.from.cross);
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
                normalise_cross(&mut rule.from.cross);
                normalise_to(&mut rule.to);
            }
        }
    }
    for rule in forbidden.iter_mut().chain(required.iter_mut()) {
        normalise_rule(rule);
    }
    for (family, list) in [
        ("forbidden", &forbidden),
        ("allowed", &allowed),
        ("required", &required),
    ] {
        for rule in list {
            check_cross_language_placement(rule, family)?;
        }
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

/// `from.<key>` for a cross-language pattern key.
fn from_key(key: &str) -> &'static str {
    match key {
        "namespace" => "from.namespace",
        "namespaceNot" => "from.namespaceNot",
        "project" => "from.project",
        "projectNot" => "from.projectNot",
        "assembly" => "from.assembly",
        _ => "from.assemblyNot",
    }
}

/// `to.<key>` for a cross-language pattern key.
fn to_key(key: &str) -> &'static str {
    match key {
        "namespace" => "to.namespace",
        "namespaceNot" => "to.namespaceNot",
        "project" => "to.project",
        "projectNot" => "to.projectNot",
        "assembly" => "to.assembly",
        _ => "to.assemblyNot",
    }
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
    for (key, patterns) in rule.from.cross.patterns() {
        add(&mut out, from_key(key), patterns);
    }
    for (key, patterns) in rule.to.cross.patterns() {
        add(&mut out, to_key(key), patterns);
    }
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

/// Compiles every pattern of every rule, applies safe-regex's check, and warns about `type-only`
/// on a rule limited to .NET ([`type_only_on_dotnet`]).
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
        if let Some(message) = type_only_on_dotnet(rule) {
            warnings.push(ConfigWarning::about(rule.name(), message));
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
        assert_eq!(
            warnings.len(),
            1,
            "only the nested quantifier: licences and metrics are computed now"
        );
        assert!(matches!(
            check_patterns(&nested, true),
            Err(ConfigError::Strict { .. })
        ));
        let every = rule_set(&object(
            json!({ "forbidden": [{ "from": { "path": "a", "pathNot": "b" }, "to": { "path": "c", "pathNot": "d", "licenseNot": "e", "exoticRequire": "f", "exoticRequireNot": "g", "via": { "path": "h", "pathNot": "i" }, "viaOnly": { "path": "j" } }, "module": { "path": "k", "pathNot": "l" } }] }),
        ))?;
        assert_eq!(rule_patterns(&every.forbidden[0]).len(), 12);
        let cross = rule_set(&object(json!({ "forbidden": [{
            "from": { "namespace": "a", "namespaceNot": "b", "project": "c", "projectNot": "d", "assembly": "e", "assemblyNot": "f" },
            "to": { "namespace": "g", "namespaceNot": "h", "project": "i", "projectNot": "j", "assembly": "k", "assemblyNot": "l" }
        }] })))?;
        let at: Vec<(&str, &str)> = rule_patterns(&cross.forbidden[0]);
        assert_eq!(
            at,
            [
                ("from.namespace", "a"),
                ("from.namespaceNot", "b"),
                ("from.project", "c"),
                ("from.projectNot", "d"),
                ("from.assembly", "e"),
                ("from.assemblyNot", "f"),
                ("to.namespace", "g"),
                ("to.namespaceNot", "h"),
                ("to.project", "i"),
                ("to.projectNot", "j"),
                ("to.assembly", "k"),
                ("to.assemblyNot", "l"),
            ]
        );
        let broken = rule_set(&object(
            json!({ "forbidden": [{ "name": "ns", "from": {}, "to": { "namespace": "(?<=x)" } }] }),
        ))?;
        let error = check_patterns(&broken, false)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("ns") && error.contains("to.namespace"),
            "{error}"
        );
        Ok(())
    }

    #[test]
    fn cross_language_keys_are_native_only() -> Result<(), ConfigError> {
        for (side, key, value) in [
            ("from", "language", json!("dotnet")),
            ("from", "namespace", json!("^A")),
            ("from", "namespaceNot", json!("^A")),
            ("from", "project", json!("^A")),
            ("from", "projectNot", json!("^A")),
            ("from", "assembly", json!("^A")),
            ("from", "assemblyNot", json!("^A")),
            ("to", "language", json!("python")),
            ("to", "namespace", json!("^A")),
            ("to", "assemblyNot", json!("^A")),
            ("to", "dependencyKind", json!("inherits")),
            ("to", "dependencyKindNot", json!(["body"])),
        ] {
            let config = object(json!({ "forbidden": [{ "name": "r", side: { key: value } }] }));
            assert!(
                check_keys(&config, CompatMode::Native, false)?
                    .warnings
                    .is_empty(),
                "{side}.{key} is legal in a native file"
            );
            let error = check_keys(&config, CompatMode::DependencyCruiser, false)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(
                error.contains(&format!("`forbidden[0].{side}.{key}`"))
                    && error.contains("Rulebearing addition for native configurations"),
                "{side}.{key}: {error}"
            );
        }
        let edge_on_from = object(
            json!({ "forbidden": [{ "from": { "dependencyKind": "inherits" }, "to": {} }] }),
        );
        let error = check_keys(&edge_on_from, CompatMode::Native, false)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("forbidden[0].from.dependencyKind") && error.contains("under `to`"),
            "{error}"
        );
        assert_eq!(
            cross_language_keys(
                &json!({ "path": "x", "namespace": "a", "dependencyKind": "body" })
            ),
            ["namespace", "dependencyKind"]
        );
        Ok(())
    }

    #[test]
    fn cross_language_values_normalise_and_name_bad_vocabulary() -> Result<(), ConfigError> {
        let rules = rule_set(&object(json!({ "forbidden": [{
            "from": { "language": "dotnet", "namespace": ["^A\\.", "^B\\."] },
            "to": { "language": ["dotnet", "python"], "dependencyKind": ["inherits", "implements"], "assemblyNot": ["x", "y"] }
        }] })))?;
        let rule = &rules.forbidden[0];
        assert_eq!(
            rule.from.cross.namespace,
            Some(Patterns::One("^A\\.|^B\\.".into()))
        );
        assert_eq!(
            rule.to.cross.assembly_not,
            Some(Patterns::One("x|y".into()))
        );
        assert_eq!(
            rule.from
                .cross
                .language
                .as_ref()
                .map(|l| l.as_slice().to_vec()),
            Some(vec![rb_model::Language::Dotnet])
        );
        assert_eq!(
            rule.to
                .dependency_kind
                .as_ref()
                .map(|k| k.as_slice().to_vec()),
            Some(vec![
                rb_model::DependencyKind::Inherits,
                rb_model::DependencyKind::Implements
            ])
        );
        assert!(rule.from.cross.only(rb_model::Language::Dotnet));
        assert!(!rule.to.cross.only(rb_model::Language::Dotnet));
        assert!(!CrossLanguageKeys::default().only(rb_model::Language::Dotnet));
        assert_eq!(rule.from.cross.written(), ["language", "namespace"]);
        assert_eq!(
            rule.to.additions(),
            ["language", "assemblyNot", "dependencyKind"]
        );
        let back = serde_json::to_value(rule).unwrap_or_default();
        assert_eq!(
            back["from"]["language"], "dotnet",
            "one value writes as one"
        );
        assert_eq!(back["to"]["language"], json!(["dotnet", "python"]));
        for (value, needle) in [
            (
                json!({ "to": { "dependencyKind": "inherit" } }),
                "`inherit` is not a valid dependency kind",
            ),
            (
                json!({ "from": { "language": ["dotnet", "csharp"] } }),
                "`csharp` is not a valid language",
            ),
        ] {
            let error = rule_set(&object(json!({ "forbidden": [value] })))
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(error.contains(needle), "{error}");
        }
        Ok(())
    }

    #[test]
    fn cross_language_keys_refuse_rules_they_cannot_narrow() {
        for (rule, needle) in [
            (
                json!({ "forbidden": [{ "name": "f", "scope": "folder", "from": { "language": "dotnet" }, "to": { "circular": true } }] }),
                "folder-scoped",
            ),
            (
                json!({ "forbidden": [{ "name": "r", "from": { "namespace": "^A" }, "to": { "path": "x", "reachable": false } }] }),
                "reachability",
            ),
            (
                json!({ "forbidden": [{ "name": "d", "from": {}, "module": { "path": "x", "numberOfDependentsLessThan": 2 }, "to": { "project": "p" } }] }),
                "dependents or required",
            ),
            (
                json!({ "required": [{ "name": "q", "module": { "path": "x" }, "to": { "path": "y", "dependencyKind": "body" } }] }),
                "dependents or required",
            ),
            (
                json!({ "allowed": [{ "from": {}, "to": { "path": "y", "reachable": true, "assembly": "A" } }] }),
                "reachability",
            ),
        ] {
            let error = rule_set(&object(rule))
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(error.contains(needle), "{needle}: {error}");
        }
        let orphans = rule_set(&object(
            json!({ "forbidden": [{ "name": "o", "from": { "orphan": true, "language": "dotnet" }, "to": {} }] }),
        ));
        assert!(
            orphans.is_ok(),
            "an orphan rule narrows by the module's keys"
        );
    }

    #[test]
    fn type_only_on_a_dotnet_rule_is_a_warning() -> Result<(), ConfigError> {
        let warned = |value: Value| -> Result<Vec<ConfigWarning>, ConfigError> {
            let rules = rule_set(&object(json!({ "forbidden": [value] })))?;
            check_patterns(&rules, false)
        };
        for (value, at, side) in [
            (
                json!({ "name": "a", "from": { "language": "dotnet" }, "to": { "dependencyTypes": ["type-only"] } }),
                "to.dependencyTypes",
                "from",
            ),
            (
                json!({ "name": "b", "from": {}, "to": { "language": ["dotnet"], "dependencyTypesNot": ["type-only"] } }),
                "to.dependencyTypesNot",
                "to",
            ),
            (
                json!({ "name": "c", "from": { "language": "dotnet" }, "to": { "via": { "dependencyTypes": ["type-only"] } } }),
                "to.via",
                "from",
            ),
            (
                json!({ "name": "d", "from": { "language": "dotnet" }, "to": { "viaOnly": { "dependencyTypesNot": ["type-only"] } } }),
                "to.via",
                "from",
            ),
        ] {
            let warnings = warned(value)?;
            assert_eq!(warnings.len(), 1, "{at}");
            assert!(
                warnings[0]
                    .message
                    .starts_with(&format!("{at} names `type-only`"))
                    && warnings[0].message.contains(&format!("`{side}.language`"))
                    && warnings[0].message.contains("signature-only"),
                "{}",
                warnings[0].message
            );
        }
        for quiet in [
            json!({ "from": { "language": ["dotnet", "python"] }, "to": { "dependencyTypes": ["type-only"] } }),
            json!({ "from": { "language": "python" }, "to": { "dependencyTypes": ["type-only"] } }),
            json!({ "from": {}, "to": { "dependencyTypes": ["type-only"] } }),
            json!({ "from": { "language": "dotnet" }, "to": { "dependencyTypes": ["signature-only"], "via": { "path": "x" } } }),
        ] {
            assert!(warned(quiet.clone())?.is_empty(), "{quiet}");
        }
        Ok(())
    }
}
