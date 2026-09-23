//! The engine entry point: one graph document and one configuration in, the annotated document,
//! the violations, the vacuous rules and the per-rule statistics out.
//!
//! - Contract: [Wave 1 plan § 1.5](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#15-interfaces-and-contracts-frozen-by-this-wave)
//!   (`evaluate(doc, cfg, opts) -> Evaluation`)
//! - Decisions: [ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md) (liveness),
//!   [ADR-0015](../../../docs/adr/0015-stable-violation-id.md) (ids),
//!   [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md) (additions)
//! - Plan: [Wave 1, Steps 5 to 7](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-7-liveness-severity-ids-receipts-expires-ratchets-1b)
//! - Requirements: [FR-CORE-04](../../../docs/prd.md#fr-core-04), [FR-CORE-05](../../../docs/prd.md#fr-core-05),
//!   [FR-RULE-01](../../../docs/prd.md#fr-rule-01), [FR-CFG-07](../../../docs/prd.md#fr-cfg-07)
//!
//! The stages are dependency-cruiser's `analyze`, in its order: cycles, dependents, orphans,
//! reachability, instability, `focus`, validation, known violations, folders, summary. Rulebearing
//! adds four things after them, none of which changes a dependency-cruiser field: the stable id,
//! `fix` and decision token on each violation; liveness (a rule whose selecting side matches no
//! module is vacuous); expiry of rules and known violations; and the per-rule statistics
//! `rules --json` prints.

use std::collections::HashMap;

use chrono::NaiveDate;
use rb_config::model::{DependencyRules, Family, KnownViolation};
use rb_config::{Config, Rule, decision_token};
use rb_model::violation_id::violation_id;
use rb_model::{ExpiredEntry, Folder, GraphDocument, Module, Summary, VacuousRule, Violation};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::derive::{self, DependentsWhen};
use crate::folders::folders;
use crate::graph::filters::{Filter, add_focus};
use crate::js;
use crate::matchers::pattern;
use crate::patterns;
use crate::summarize::{
    is_same_violation, options_used, rule_set_used, summarize_folders, summarize_modules,
    violation_stats,
};
use crate::validate::{validate_dependency, validate_module};

/// Why the engine could not evaluate.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The annotated graph did not convert back into the document types.
    #[error("the annotated graph is not a valid document: {0}")]
    Document(#[from] serde_json::Error),
}

/// How to evaluate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalOptions {
    /// Report rules whose selecting side matches nothing ([ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md)).
    pub liveness: bool,
    /// Validate against the rules; `false` marks everything valid, as dependency-cruiser without
    /// a rule set does.
    pub validate: bool,
    /// Compute instability and folders (`--metrics`); a folder-scoped or `moreUnstable` rule
    /// turns it on regardless, as upstream's `shouldCalculateMetrics` does.
    pub metrics: bool,
    /// The day `expires` is compared against.
    pub today: NaiveDate,
    /// The positional arguments, for `optionsUsed.args`.
    pub args: Vec<String>,
    /// The options for `optionsUsed`, already normalised by the caller.
    pub options_used: Map<String, Value>,
}

impl Default for EvalOptions {
    fn default() -> Self {
        Self {
            liveness: true,
            validate: true,
            metrics: false,
            today: NaiveDate::MIN,
            args: Vec::new(),
            options_used: Map::new(),
        }
    }
}

/// What one rule matched, for `rules --json`, `explain` and liveness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleStats {
    /// The rule name (`allowed[n]` for an `allowed` entry).
    pub name: String,
    /// Which list the rule is in.
    pub family: Family,
    /// Modules the selecting side matches.
    pub from_matches: usize,
    /// Dependencies (or, for a reachability or required rule, modules) `to.path` matches.
    pub to_matches: usize,
    /// Violations of the rule in the summary.
    pub violations: usize,
}

/// A rule or known violation past its `expires` date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Expired {
    /// The rule, or the known violation's id or `from -> to`.
    pub name: String,
    /// The last day it applied.
    pub expires: NaiveDate,
    /// `rule` or `knownViolation`.
    pub kind: String,
}

impl Expired {
    /// The entry `summary.expired[]` carries
    /// ([ADR-0031](../../../docs/adr/0031-a-saved-result-carries-what-the-exit-code-counts.md)).
    pub fn entry(&self) -> ExpiredEntry {
        ExpiredEntry {
            name: self.name.clone(),
            expires: self.expires.format("%Y-%m-%d").to_string(),
            kind: self.kind.clone(),
        }
    }
}

/// The result of an evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    /// The annotated document, summary included.
    pub document: GraphDocument,
    /// Rules whose selecting side matched nothing, with liveness on and no `allowEmpty`.
    pub vacuous: Vec<VacuousRule>,
    /// Per-rule statistics, in rule order.
    pub rule_stats: Vec<RuleStats>,
    /// Rules and known violations past their date; each fails the run.
    pub expired: Vec<Expired>,
}

impl Evaluation {
    /// `summary.violations`.
    pub fn violations(&self) -> &[Violation] {
        &self.document.summary.violations
    }

    /// The error-severity violation count plus the expired entries: what the exit code counts
    /// ([ADR-0008](../../../docs/adr/0008-exit-code-contract.md)).
    pub fn error_count(&self) -> u64 {
        self.document.summary.error + self.expired.len() as u64
    }
}

/// Whether a rule needs metrics: `to.moreUnstable` or `scope: folder`.
pub fn needs_metrics(rules: &DependencyRules) -> bool {
    rules
        .forbidden
        .iter()
        .chain(&rules.allowed)
        .any(|r| r.to.more_unstable.is_some() || r.is_folder_scope())
}

/// The selecting side of a rule: `module` for dependents and required rules, else `from`.
fn selects(rule: &Rule, module: &Value) -> bool {
    let source = js::text(module, "source");
    let (path, path_not) = match &rule.module {
        Some(m) => (pattern(m.path.as_ref()), pattern(m.path_not.as_ref())),
        None => (
            pattern(rule.from.path.as_ref()),
            pattern(rule.from.path_not.as_ref()),
        ),
    };
    path.is_none_or(|p| patterns::test(&p, &source))
        && path_not.is_none_or(|p| !patterns::test(&p, &source))
}

fn has_placeholder(p: &str) -> bool {
    p.as_bytes()
        .windows(2)
        .any(|w| w[0] == b'$' && w[1].is_ascii_digit())
}

fn to_matches(rule: &Rule, family: Family, modules: &[Value]) -> usize {
    let Some(path) = pattern(rule.to.path.as_ref()).filter(|p| !has_placeholder(p)) else {
        return 0;
    };
    let path_not = pattern(rule.to.path_not.as_ref()).filter(|p| !has_placeholder(p));
    let ok = |text: &str| {
        patterns::test(&path, text) && path_not.as_ref().is_none_or(|p| !patterns::test(p, text))
    };
    if rule.to.reachable.is_some() || family == Family::Required {
        modules
            .iter()
            .filter(|m| ok(&js::text(m, "source")))
            .count()
    } else {
        modules
            .iter()
            .flat_map(|m| js::array(m, "dependencies"))
            .filter(|d| ok(&js::text(d, "resolved")))
            .count()
    }
}

/// A known violation, as dependency-cruiser's softening reads it, with its expiry applied.
struct Known {
    shape: Value,
    id: Option<String>,
}

fn known_entries(
    entries: &[KnownViolation],
    today: NaiveDate,
    expired: &mut Vec<Expired>,
) -> Vec<Known> {
    let mut out = Vec::new();
    for entry in entries {
        if let Some(expires) = entry.expires
            && today > expires
        {
            let name = entry.id.clone().unwrap_or_else(|| {
                format!(
                    "{} -> {}",
                    entry.from.as_deref().unwrap_or("?"),
                    entry.to.as_deref().unwrap_or("?")
                )
            });
            expired.push(Expired {
                name,
                expires,
                kind: "knownViolation".into(),
            });
            continue;
        }
        out.push(Known {
            shape: serde_json::to_value(entry).unwrap_or(Value::Null),
            id: entry.id.clone(),
        });
    }
    out
}

fn kind_of(value: &Value) -> Option<&str> {
    js::str_of(value, "type")
}

/// `softenKnownViolations`, plus the id-keyed entries Rulebearing writes.
fn soften(modules: &mut [Value], known: &[Known]) {
    if known.is_empty() {
        return;
    }
    let ignore = |rule: &mut Value| js::set(rule, "severity", json!("ignore"));
    for module in modules.iter_mut() {
        let source = js::text(module, "source").into_owned();
        if module.get("valid") == Some(&Value::Bool(false))
            && let Some(Value::Array(rules)) = module.get_mut("rules")
        {
            for rule in rules.iter_mut() {
                let name = js::text(rule, "name").into_owned();
                let id = violation_id(&name, &source, &source, "");
                let hit = known.iter().any(|k| {
                    k.id.as_deref() == Some(id.as_str())
                        || (matches!(kind_of(&k.shape), Some("module" | "reachability"))
                            && js::str_of(&k.shape, "from") == Some(source.as_str())
                            && k.shape.get("rule").and_then(|r| js::str_of(r, "name"))
                                == Some(name.as_str()))
                });
                if hit {
                    ignore(rule);
                }
            }
        }
        if let Some(Value::Array(dependencies)) = module.get_mut("dependencies") {
            for dependency in dependencies.iter_mut() {
                if dependency.get("valid") != Some(&Value::Bool(false)) {
                    continue;
                }
                let to = js::text(dependency, "resolved").into_owned();
                let kind = js::str_of(dependency, "dependencyKind")
                    .unwrap_or("")
                    .to_owned();
                let cycle = dependency.get("cycle").cloned();
                if let Some(Value::Array(rules)) = dependency.get_mut("rules") {
                    for rule in rules.iter_mut() {
                        let name = js::text(rule, "name").into_owned();
                        let id = violation_id(&name, &source, &to, &kind);
                        let mut key = json!({ "rule": rule.clone(), "from": source, "to": to });
                        if let Some(cycle) = &cycle {
                            js::set(&mut key, "cycle", cycle.clone());
                        }
                        let hit = known.iter().any(|k| {
                            k.id.as_deref() == Some(id.as_str())
                                || (matches!(
                                    kind_of(&k.shape),
                                    Some("dependency" | "cycle" | "instability")
                                ) && is_same_violation(&k.shape, &key))
                        });
                        if hit {
                            ignore(rule);
                        }
                    }
                }
            }
        }
    }
}

/// Adds the stable id, the `fix` and the decision token to each violation.
fn annotate(violations: &mut [Value], modules: &[Value], rules: &DependencyRules) {
    let mut kinds: HashMap<(String, String), String> = HashMap::new();
    for module in modules {
        for dependency in js::array(module, "dependencies") {
            if let Some(kind) = js::str_of(dependency, "dependencyKind") {
                kinds.insert(
                    (
                        js::text(module, "source").into_owned(),
                        js::text(dependency, "resolved").into_owned(),
                    ),
                    kind.to_owned(),
                );
            }
        }
    }
    for violation in violations.iter_mut() {
        let name = violation
            .get("rule")
            .map(|r| js::text(r, "name").into_owned())
            .unwrap_or_default();
        let from = js::text(violation, "from").into_owned();
        let to = js::text(violation, "to").into_owned();
        let edge = matches!(
            kind_of(violation),
            Some("dependency" | "cycle" | "instability")
        );
        let kind = if edge {
            kinds
                .get(&(from.clone(), to.clone()))
                .cloned()
                .unwrap_or_default()
        } else {
            String::new()
        };
        js::set(
            violation,
            "id",
            json!(violation_id(&name, &from, &to, &kind)),
        );
        let rule = rules
            .forbidden
            .iter()
            .chain(&rules.required)
            .find(|r| r.name() == name)
            .or_else(|| {
                (name == "not-in-allowed")
                    .then(|| rules.allowed.first())
                    .flatten()
            });
        if let Some(rule) = rule {
            if let Some(fix) = &rule.meta.fix {
                js::set(violation, "fix", json!(fix));
            }
            if let Some(token) = rule.meta.comment.as_deref().and_then(decision_token) {
                js::set(violation, "decision", json!(token));
            }
        }
    }
}

fn stats_and_liveness(
    rules: &DependencyRules,
    modules: &[Value],
    violations: &[Value],
    liveness: bool,
) -> (Vec<RuleStats>, Vec<VacuousRule>) {
    let mut stats = Vec::new();
    let mut vacuous = Vec::new();
    let lists: [(Family, &[Rule]); 3] = [
        (Family::Forbidden, &rules.forbidden),
        (Family::Allowed, &rules.allowed),
        (Family::Required, &rules.required),
    ];
    for (family, list) in lists {
        for (index, rule) in list.iter().enumerate() {
            let name = if family == Family::Allowed {
                format!("allowed[{index}]")
            } else {
                rule.name().to_owned()
            };
            let from_matches = modules.iter().filter(|m| selects(rule, m)).count();
            let count = violations
                .iter()
                .filter(|v| v.get("rule").and_then(|r| js::str_of(r, "name")) == Some(rule.name()))
                .count();
            if liveness && from_matches == 0 && !rule.meta.allow_empty {
                vacuous.push(VacuousRule::new(
                    name.clone(),
                    if rule.module.is_some() {
                        "module"
                    } else {
                        "from"
                    },
                ));
            }
            stats.push(RuleStats {
                name,
                family,
                from_matches,
                to_matches: to_matches(rule, family, modules),
                violations: count,
            });
        }
    }
    (stats, vacuous)
}

fn expired_rules(rules: &DependencyRules, today: NaiveDate) -> Vec<Expired> {
    rules
        .forbidden
        .iter()
        .chain(&rules.allowed)
        .chain(&rules.required)
        .filter_map(|r| {
            r.meta
                .expires
                .filter(|e| today > *e)
                .map(|expires| Expired {
                    name: r.name().to_owned(),
                    expires,
                    kind: "rule".into(),
                })
        })
        .collect()
}

/// The `focus` filter as `addFocus` reads it: present only when `focus.path` is set.
fn focus_filter(options: &rb_config::model::Options) -> Option<Filter> {
    options
        .focus
        .as_ref()
        .and_then(|f| f.path.clone())
        .map(|path| Filter {
            path: Some(path),
            depth: options.focus.as_ref().and_then(|f| f.depth),
        })
}

/// Merges each module's and each dependency's verdict into it; with `validate` off, every
/// verdict is `{ "valid": true }`.
fn add_validations(modules: &mut [Value], rules: &DependencyRules, validate: bool) {
    for module in modules {
        let verdict = if validate {
            validate_module(rules, module)
        } else {
            json!({ "valid": true })
        };
        let snapshot = module.clone();
        if let (Value::Object(target), Value::Object(verdict)) = (&mut *module, verdict) {
            target.extend(verdict);
        }
        if let Some(Value::Array(dependencies)) = module.get_mut("dependencies") {
            for dependency in dependencies.iter_mut() {
                let verdict = if validate {
                    validate_dependency(rules, &snapshot, dependency)
                } else {
                    json!({ "valid": true })
                };
                if let (Value::Object(target), Value::Object(verdict)) = (dependency, verdict) {
                    target.extend(verdict);
                }
            }
        }
    }
}

/// Runs the whole analysis.
///
/// # Errors
/// [`EngineError::Document`] if the annotated graph does not convert back into document types,
/// which would be a bug in the engine rather than in the input.
pub fn evaluate(
    document: GraphDocument,
    config: &Config,
    opts: &EvalOptions,
) -> Result<Evaluation, EngineError> {
    let rules = &config.rules.dependencies;
    let options = &config.options;
    let skip = options.skip_analysis_not_in_rules.unwrap_or(false);
    let metrics = opts.metrics || needs_metrics(rules);
    let focus = focus_filter(&config.options);
    let GraphDocument {
        modules,
        revision_data,
        code,
        ..
    } = document;
    let mut modules: Vec<Value> = modules
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<_, _>>()?;

    derive::cycles(&mut modules, "source", "resolved", skip, rules);
    derive::dependents(
        &mut modules,
        DependentsWhen {
            skip,
            metrics,
            reaches: options.reaches.is_some(),
            focus: focus.is_some(),
            force: options.force_derive_dependents.unwrap_or(false),
        },
        rules,
    );
    derive::orphans(&mut modules, skip, rules);
    derive::reachables(&mut modules, rules);
    if metrics {
        derive::module_metrics(&mut modules);
    }
    if let Some(focus) = &focus {
        modules = add_focus(modules, focus);
    }
    add_validations(&mut modules, rules, opts.validate);
    let mut expired = expired_rules(rules, opts.today);
    let known = known_entries(&config.known_violations, opts.today, &mut expired);
    soften(&mut modules, &known);

    let folder_values = if metrics {
        folders(&modules, skip, rules)
    } else {
        Vec::new()
    };
    let mut violations = summarize_modules(&modules, Some(rules));
    violations.extend(summarize_folders(&folder_values, Some(rules)));
    violations.sort_by(crate::compare::compare_violations);
    annotate(&mut violations, &modules, rules);
    let (rule_stats, vacuous) = stats_and_liveness(rules, &modules, &violations, opts.liveness);

    let stats = violation_stats(&violations);
    let count = |k: &str| stats.get(k).and_then(Value::as_u64).unwrap_or(0);
    let used = rule_set_used(rules);
    let summary = Summary {
        violations: violations
            .into_iter()
            .map(serde_json::from_value)
            .collect::<Result<_, _>>()?,
        error: count("error"),
        warn: count("warn"),
        info: count("info"),
        ignore: Some(count("ignore")),
        total_cruised: modules.len() as u64,
        total_dependencies_cruised: Some(
            modules
                .iter()
                .map(|m| js::array(m, "dependencies").len() as u64)
                .sum(),
        ),
        rule_set_used: (!used.is_empty()).then_some(used),
        options_used: options_used(&opts.options_used, &opts.args),
        vacuous_rules: (!vacuous.is_empty()).then(|| vacuous.clone()),
        expired: (!expired.is_empty()).then(|| expired.iter().map(Expired::entry).collect()),
        // `environment`, `inspected` and `ratchets` belong to the command line.
        ..Summary::default()
    };
    let modules: Vec<Module> = modules
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<_, _>>()?;
    let folders: Option<Vec<Folder>> = if metrics {
        Some(
            folder_values
                .into_iter()
                .map(serde_json::from_value)
                .collect::<Result<_, _>>()?,
        )
    } else {
        None
    };
    Ok(Evaluation {
        document: GraphDocument {
            modules,
            folders,
            summary,
            revision_data,
            code,
        },
        vacuous,
        rule_stats,
        expired,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_model::{Dependency, DependencyKind, DependencyType, ModuleSystem, Severity};

    fn config(value: Value) -> Config {
        let map = match value {
            Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        rb_config::load::from_canonical(map, rb_config::CompatMode::Native).unwrap_or_default()
    }

    fn edge(to: &str) -> Dependency {
        Dependency {
            dependency_types: vec![DependencyType::Local],
            followable: true,
            dependency_kind: Some(DependencyKind::Import),
            line: Some(1),
            column: Some(1),
            ..Dependency::new(format!("./{to}"), to, ModuleSystem::Es6)
        }
    }

    fn document() -> GraphDocument {
        let module = |source: &str, deps: &[&str]| Module {
            dependencies: deps.iter().map(|d| edge(d)).collect(),
            ..Module::new(source)
        };
        GraphDocument {
            modules: vec![
                module("apps/web/a.ts", &["apps/api/b.ts", "packages/x.ts"]),
                module("apps/api/b.ts", &["apps/web/a.ts"]),
                module("packages/x.ts", &[]),
                module("lonely.ts", &[]),
            ],
            ..GraphDocument::default()
        }
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 22).unwrap_or_default()
    }

    #[test]
    fn a_fence_with_captures_ids_fix_and_decision() -> Result<(), EngineError> {
        let cfg = config(json!({ "forbidden": [{
            "name": "no-cross-app", "severity": "error", "comment": "Apps share packages only. adr:0003",
            "fix": "Move the code into packages/*.",
            "from": { "path": "^apps/([^/]+)/" }, "to": { "path": "^apps/([^/]+)/", "pathNot": "^apps/$1/" }
        }] }));
        let result = evaluate(
            document(),
            &cfg,
            &EvalOptions {
                today: today(),
                ..EvalOptions::default()
            },
        )?;
        let v = result.violations();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].from, "apps/api/b.ts");
        assert_eq!(v[0].fix.as_deref(), Some("Move the code into packages/*."));
        assert_eq!(v[0].decision.as_deref(), Some("adr:0003"));
        assert_eq!(
            v[0].id.as_deref(),
            Some(violation_id("no-cross-app", "apps/api/b.ts", "apps/web/a.ts", "import").as_str())
        );
        assert_eq!(result.document.summary.error, 2);
        assert_eq!(result.error_count(), 2);
        assert!(result.vacuous.is_empty());
        assert_eq!(result.rule_stats[0].from_matches, 2);
        assert_eq!(result.rule_stats[0].violations, 2);
        let a = &result.document.modules[0];
        assert!(!a.dependencies[0].valid);
        assert!(a.dependencies[0].circular, "a and b form a cycle");
        assert_eq!(result.document.summary.total_cruised, 4);
        assert_eq!(result.document.summary.total_dependencies_cruised, Some(3));
        Ok(())
    }

    #[test]
    fn liveness_and_allow_empty() -> Result<(), EngineError> {
        let cfg = config(json!({ "forbidden": [
            { "name": "dead", "from": { "path": "^nowhere/" }, "to": {} },
            { "name": "opted-out", "allowEmpty": true, "from": { "path": "^nowhere/" }, "to": {} },
            { "name": "dependents", "from": {}, "module": { "path": "^nowhere", "numberOfDependentsLessThan": 1 } }
        ] }));
        let result = evaluate(document(), &cfg, &EvalOptions::default())?;
        let names: Vec<(&str, &str)> = result
            .vacuous
            .iter()
            .map(|v| (v.name.as_str(), v.side.as_str()))
            .collect();
        assert_eq!(names, [("dead", "from"), ("dependents", "module")]);
        assert_eq!(
            result.document.summary.vacuous_rules.as_ref().map(Vec::len),
            Some(2)
        );
        let off = evaluate(
            document(),
            &cfg,
            &EvalOptions {
                liveness: false,
                ..EvalOptions::default()
            },
        )?;
        assert!(off.vacuous.is_empty());
        Ok(())
    }

    #[test]
    fn orphans_and_known_violations() -> Result<(), EngineError> {
        let cfg = config(json!({
            "forbidden": [
                { "name": "no-orphans", "severity": "error", "from": { "orphan": true }, "to": {} },
                { "name": "no-circular", "severity": "error", "from": {}, "to": { "circular": true } }
            ],
            "options": { "knownViolations": [
                { "type": "module", "from": "lonely.ts", "to": "lonely.ts", "rule": { "name": "no-orphans", "severity": "error" } },
                { "id": violation_id("no-circular", "apps/web/a.ts", "apps/api/b.ts", "import") },
                { "id": "RB-00000000", "expires": "2020-01-01" }
            ] }
        }));
        let result = evaluate(
            document(),
            &cfg,
            &EvalOptions {
                today: today(),
                ..EvalOptions::default()
            },
        )?;
        let severities: Vec<(String, Severity)> = result
            .violations()
            .iter()
            .map(|v| (v.from.clone(), v.rule.severity))
            .collect();
        assert!(severities.contains(&("lonely.ts".into(), Severity::Ignore)));
        // The softened edge of the cycle sorts after the other edge, which is the same cycle, so
        // only the error survives deduplication, as in dependency-cruiser.
        assert!(severities.contains(&("apps/api/b.ts".into(), Severity::Error)));
        let softened = &result.document.modules[0].dependencies[0];
        assert_eq!(
            softened.rules.as_ref().map(|r| r[0].severity),
            Some(Severity::Ignore)
        );
        assert_eq!(result.expired.len(), 1);
        assert_eq!(result.expired[0].kind, "knownViolation");
        assert_eq!(result.error_count(), result.document.summary.error + 1);
        Ok(())
    }

    #[test]
    fn expired_rules_fail_the_day_after() -> Result<(), EngineError> {
        let cfg = config(
            json!({ "forbidden": [{ "name": "temp", "expires": "2026-09-21", "from": {}, "to": {} }] }),
        );
        let result = evaluate(
            document(),
            &cfg,
            &EvalOptions {
                today: today(),
                ..EvalOptions::default()
            },
        )?;
        assert_eq!(
            result.expired,
            [Expired {
                name: "temp".into(),
                expires: NaiveDate::from_ymd_opt(2026, 9, 21).unwrap_or_default(),
                kind: "rule".into()
            }]
        );
        assert_eq!(
            result.document.summary.expired,
            Some(vec![ExpiredEntry {
                name: "temp".into(),
                expires: "2026-09-21".into(),
                kind: "rule".into()
            }]),
            "the saved result carries it, so fmt --exit-code counts it (ADR-0031)"
        );
        let on_the_day = evaluate(
            document(),
            &cfg,
            &EvalOptions {
                today: NaiveDate::from_ymd_opt(2026, 9, 21).unwrap_or_default(),
                ..EvalOptions::default()
            },
        )?;
        assert!(on_the_day.expired.is_empty());
        assert_eq!(on_the_day.document.summary.expired, None);
        Ok(())
    }

    #[test]
    fn folder_rules_turn_metrics_on() -> Result<(), EngineError> {
        let cfg = config(
            json!({ "forbidden": [{ "name": "folder-cycles", "scope": "folder", "severity": "error", "from": {}, "to": { "circular": true } }] }),
        );
        assert!(needs_metrics(&cfg.rules.dependencies));
        let result = evaluate(document(), &cfg, &EvalOptions::default())?;
        assert!(result.document.folders.is_some());
        assert!(result.document.modules[0].instability.is_some());
        assert!(
            result
                .violations()
                .iter()
                .any(|v| v.rule.name == "folder-cycles")
        );
        Ok(())
    }

    #[test]
    fn without_validation_everything_is_valid() -> Result<(), EngineError> {
        let cfg = config(
            json!({ "forbidden": [{ "name": "all", "severity": "error", "from": {}, "to": {} }] }),
        );
        let result = evaluate(
            document(),
            &cfg,
            &EvalOptions {
                validate: false,
                liveness: false,
                ..EvalOptions::default()
            },
        )?;
        assert!(result.violations().is_empty());
        assert!(result.document.modules.iter().all(|m| m.valid));
        Ok(())
    }

    #[test]
    fn allowed_rules_and_required_rules() -> Result<(), EngineError> {
        let cfg = config(json!({
            "allowed": [{ "fix": "Only import packages.", "from": {}, "to": { "path": "^packages/" } }],
            "allowedSeverity": "error",
            "required": [{ "name": "needs-packages", "severity": "warn", "module": { "path": "^apps/api" }, "to": { "path": "^packages/" } }]
        }));
        let result = evaluate(document(), &cfg, &EvalOptions::default())?;
        let names: Vec<&str> = result
            .violations()
            .iter()
            .map(|v| v.rule.name.as_str())
            .collect();
        assert!(names.contains(&"not-in-allowed"));
        assert!(names.contains(&"needs-packages"));
        let allowed = result
            .violations()
            .iter()
            .find(|v| v.rule.name == "not-in-allowed");
        assert_eq!(
            allowed.and_then(|v| v.fix.as_deref()),
            Some("Only import packages.")
        );
        assert_eq!(result.rule_stats[0].name, "allowed[0]");
        assert_eq!(result.rule_stats[1].to_matches, 1);
        Ok(())
    }

    #[test]
    fn known_violations_soften_only_their_own_kind() -> Result<(), EngineError> {
        let cfg = config(json!({
            "forbidden": [
                { "name": "no-orphans", "severity": "error", "from": { "orphan": true }, "to": {} },
                { "name": "no-packages", "severity": "error", "from": {}, "to": { "path": "^packages/" } }
            ],
            "options": { "knownViolations": [
                { "type": "module", "from": "lonely.ts", "to": "lonely.ts", "rule": { "name": "other", "severity": "error" } },
                { "type": "module", "from": "elsewhere.ts", "to": "elsewhere.ts", "rule": { "name": "no-orphans", "severity": "error" } },
                { "type": "dependency", "from": "lonely.ts", "to": "lonely.ts", "rule": { "name": "no-orphans", "severity": "error" } },
                { "type": "module", "from": "apps/web/a.ts", "to": "packages/x.ts", "rule": { "name": "no-packages", "severity": "error" } },
                { "id": "RB-00000000", "expires": "2026-09-22" }
            ] }
        }));
        let result = evaluate(
            document(),
            &cfg,
            &EvalOptions {
                today: today(),
                ..EvalOptions::default()
            },
        )?;
        let found: Vec<(&str, &str, Severity)> = result
            .violations()
            .iter()
            .map(|v| (v.rule.name.as_str(), v.from.as_str(), v.rule.severity))
            .collect();
        assert_eq!(
            found,
            [
                ("no-orphans", "lonely.ts", Severity::Error),
                ("no-packages", "apps/web/a.ts", Severity::Error),
            ]
        );
        assert_eq!(result.document.summary.ignore, Some(0));
        assert!(
            result.expired.is_empty(),
            "an entry expiring today still applies today"
        );
        Ok(())
    }

    #[test]
    fn metrics_are_needed_only_for_instability_and_folders() {
        let plain = config(
            json!({ "forbidden": [{ "name": "p", "from": {}, "to": { "circular": true } }] }),
        );
        assert!(!needs_metrics(&plain.rules.dependencies));
        let unstable =
            config(json!({ "allowed": [{ "from": {}, "to": { "moreUnstable": true } }] }));
        assert!(needs_metrics(&unstable.rules.dependencies));
    }

    #[test]
    fn the_summary_counts_each_severity_and_records_the_options() -> Result<(), EngineError> {
        let cfg = config(json!({ "forbidden": [
            { "name": "w", "severity": "warn", "from": {}, "to": { "path": "^packages/" } },
            { "name": "i", "severity": "info", "from": {}, "to": { "path": "^apps/api" } }
        ] }));
        let result = evaluate(
            document(),
            &cfg,
            &EvalOptions {
                args: vec!["apps".into(), "packages".into()],
                options_used: json!({ "outputType": "json", "progress": "none" })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
                ..EvalOptions::default()
            },
        )?;
        let summary = &result.document.summary;
        assert_eq!(
            (summary.error, summary.warn, summary.info, summary.ignore),
            (0, 1, 1, Some(0))
        );
        assert_eq!(
            Value::Object(summary.options_used.clone()),
            json!({ "outputType": "json", "args": "apps packages" })
        );
        Ok(())
    }

    #[test]
    fn rule_set_used_only_when_there_are_rules() -> Result<(), EngineError> {
        let bare = evaluate(document(), &config(json!({})), &EvalOptions::default())?;
        assert_eq!(bare.document.summary.rule_set_used, None);
        assert_eq!(bare.document.folders, None, "no metrics, no folders");
        let ruled = evaluate(
            document(),
            &config(json!({ "forbidden": [{ "name": "p", "from": {}, "to": {} }] })),
            &EvalOptions::default(),
        )?;
        assert!(ruled.document.summary.rule_set_used.is_some());
        Ok(())
    }

    #[test]
    fn placeholders_are_a_dollar_and_a_digit() {
        assert!(has_placeholder("^apps/$1/"));
        assert!(!has_placeholder("^apps/v2/"));
        assert!(!has_placeholder("x$|^$a"));
        assert!(!has_placeholder("$"));
    }

    #[test]
    fn stats_count_selected_modules_and_matched_targets() {
        let modules: Vec<Value> = document()
            .modules
            .iter()
            .filter_map(|m| serde_json::to_value(m).ok())
            .collect();
        let rule = |value: Value| -> Rule { serde_json::from_value(value).unwrap_or_default() };
        let fence = rule(
            json!({ "from": {}, "to": { "path": "^(apps|packages)/", "pathNot": "^apps/api" } }),
        );
        assert_eq!(to_matches(&fence, Family::Forbidden, &modules), 2);
        let captured = rule(
            json!({ "from": {}, "to": { "path": "^(apps|packages)/", "pathNot": "^apps/$1" } }),
        );
        assert_eq!(
            to_matches(&captured, Family::Forbidden, &modules),
            3,
            "a pathNot with a placeholder cannot be counted without a from, so it is left out"
        );
        assert_eq!(
            to_matches(
                &rule(json!({ "from": {}, "to": { "path": "^$1" } })),
                Family::Forbidden,
                &modules
            ),
            0
        );
        // Dependencies for a dependency rule; modules for reachability and required rules.
        let apps_or_lonely = rule(json!({ "from": {}, "to": { "path": "^(apps|lonely)" } }));
        assert_eq!(to_matches(&apps_or_lonely, Family::Forbidden, &modules), 2);
        assert_eq!(to_matches(&apps_or_lonely, Family::Required, &modules), 3);
        let reachable =
            rule(json!({ "from": {}, "to": { "path": "^(apps|lonely)", "reachable": true } }));
        assert_eq!(to_matches(&reachable, Family::Forbidden, &modules), 3);

        let not_api =
            rule(json!({ "from": { "path": "^apps/", "pathNot": "^apps/api" }, "to": {} }));
        assert!(selects(&not_api, &modules[0]));
        assert!(!selects(&not_api, &modules[1]));
        assert!(!selects(&not_api, &modules[2]));
        let module_not = rule(json!({ "module": { "pathNot": "^apps/" }, "to": {} }));
        assert!(!selects(&module_not, &modules[0]));
        assert!(selects(&module_not, &modules[3]));
    }

    #[test]
    fn focus_narrows_the_graph() -> Result<(), EngineError> {
        let cfg = config(json!({ "options": { "focus": "^packages/" } }));
        let result = evaluate(document(), &cfg, &EvalOptions::default())?;
        let sources: Vec<&str> = result
            .document
            .modules
            .iter()
            .map(|m| m.source.as_str())
            .collect();
        assert_eq!(sources, ["apps/web/a.ts", "packages/x.ts"]);
        assert_eq!(result.document.modules[1].matches_focus, Some(true));
        Ok(())
    }
}
