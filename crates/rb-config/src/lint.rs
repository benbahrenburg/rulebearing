//! `config lint`: the mistakes people and agents make when they write rules by hand.
//!
//! - Source: [design § The native format](../../../docs/artifacts/design.md#the-native-format),
//!   [design § Rules an agent writes](../../../docs/artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar)
//! - Plan: [Wave 1, Step 4](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-4-config-convert-config-expand-config-lint-shorthands-1a);
//!   [Wave 2, Step 8](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#28-step-8-cross-language-rule-additions-per-language-dependencytypes-license-moreunstable-2d)
//!   (`type-only-on-dotnet`)
//! - Requirement: [FR-CFG-05](../../../docs/prd.md#fr-cfg-05), [FR-RULE-02](../../../docs/prd.md#fr-rule-02)
//!
//! | Code | Finding | Needs a graph |
//! | --- | --- | --- |
//! | `never-matches` | a rule's `from`, `module` or `to.path` matches nothing in the graph | yes |
//! | `shadowed` | a rule repeats an earlier rule's restrictions, so it can never add a finding | no |
//! | `overlapping-allowed` | two `allowed` entries admit the same edges | no |
//! | `allowed-admits-everything` | an `allowed` entry with empty `from` and `to` | no |
//! | `severity-below-error` | a `warn` or `info` rule with zero current violations, which could be `error` for free | yes |
//! | `no-fix` | a rule without `fix` | no |
//! | `fix-restates-name` | a `fix` that says nothing the name does not | no |
//! | `missing-decision-token` | a comment without `adr:NNNN` or `plan:<slug>`, under `--require-comment-token` | no |
//! | `type-only-on-dotnet` | a rule limited to .NET by `language` that names `type-only`, which no .NET edge carries ([design § Dependency rules](../../../docs/artifacts/design.md#dependency-rules-the-whole-of-dependency-cruiser-1820)) | no |
//! | `replaced-by-unknown` | a `replacedBy` that names no rule, `layers` or `independence` entry, or ratchet of the configuration | no |
//! | `replaced-by-self` | a `replacedBy` that names the rule itself | no |
//! | `since-after-deprecated` | a `since` later than the rule's `deprecated`, both semver ([`crate::version`]) | no |
//!
//! The lifecycle checks ([Wave 3, Step 12](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#23-steps-for-sub-wave-3c-presets-lifecycle-fields-snapshot-and-changelog))
//! cover every family: dependency, element, slice and diagram rules, the `layers` and
//! `independence` shorthands (a `layers` entry once, not once per rule it expands to) and
//! ratchets. A `deprecated` without a
//! `replacedBy` is not a finding, since a rule may be retired with nothing in its place; nor is a
//! version that is not semver, which is compared with nothing.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use rb_model::{GraphDocument, Severity};

use crate::model::{Config, Family, FromRestriction, Rule, ToRestriction};
use crate::pattern;

/// One finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The rule concerned.
    pub rule: String,
    /// The finding class, from the table above.
    pub code: &'static str,
    /// What is wrong and what to do.
    pub message: String,
}

/// What to check.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LintOptions {
    /// Report comments without a decision token.
    pub require_comment_token: bool,
}

/// Words a `fix` may add to a rule name and still say nothing.
const FILLER: &[&str] = &[
    "do", "not", "dont", "don", "t", "no", "the", "a", "an", "to", "from", "never", "avoid",
];

fn words(text: &str) -> BTreeSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether `fix` only restates `name`.
pub fn restates(name: &str, fix: &str) -> bool {
    let name_words = words(name);
    let fix_words = words(fix);
    !fix_words.is_empty()
        && fix_words
            .iter()
            .all(|w| name_words.contains(w) || FILLER.contains(&w.as_str()))
}

fn same_restrictions(a: &Rule, b: &Rule) -> bool {
    a.scope == b.scope && a.from == b.from && a.to == b.to && a.module == b.module
}

fn matches_any(pattern: Option<&rb_model::options::Patterns>, texts: &[&str]) -> Option<bool> {
    let text = pattern?.joined();
    // A pattern with a capture placeholder depends on the `from` match; skip it.
    if (0..10).any(|i| text.contains(&format!("${i}"))) {
        return None;
    }
    let matcher = pattern::matcher(&text).ok()?;
    Some(texts.iter().any(|t| matcher.is_match(t)))
}

fn graph_findings(rule: &Rule, family: Family, graph: &GraphDocument, out: &mut Vec<Finding>) {
    let sources: Vec<&str> = graph.modules.iter().map(|m| m.source.as_str()).collect();
    let resolved: Vec<&str> = graph
        .modules
        .iter()
        .flat_map(|m| m.dependencies.iter().map(|d| d.resolved.as_str()))
        .collect();
    let mut never = |side: &str| {
        out.push(Finding {
            rule: rule.name().to_owned(),
            code: "never-matches",
            message: format!(
                "{side} matches nothing in the graph, so the rule can never fire; fix the pattern or delete the rule"
            ),
        });
    };
    if matches_any(rule.from.path.as_ref(), &sources) == Some(false) {
        never("from.path");
    }
    if let Some(module) = &rule.module
        && matches_any(module.path.as_ref(), &sources) == Some(false)
    {
        never("module.path");
    }
    let targets = if rule.to.reachable.is_some() || family == Family::Required {
        &sources
    } else {
        &resolved
    };
    if matches_any(rule.to.path.as_ref(), targets) == Some(false) {
        never("to.path");
    }
    let severity = if family == Family::Allowed {
        None
    } else {
        Some(rule.severity())
    };
    if matches!(severity, Some(Severity::Warn | Severity::Info))
        && !graph
            .summary
            .violations
            .iter()
            .any(|v| v.rule.name == rule.name())
    {
        out.push(Finding {
            rule: rule.name().to_owned(),
            code: "severity-below-error",
            message: format!(
                "severity is {} but the rule has no current violations; raise it to error so it cannot regress",
                rule.severity()
            ),
        });
    }
}

/// Runs every check.
pub fn lint(config: &Config, graph: Option<&GraphDocument>, options: LintOptions) -> Vec<Finding> {
    let mut out = Vec::new();
    let rules: Vec<(Family, &Rule)> = config.rules.all_dependency_rules().collect();
    for (index, (family, rule)) in rules.iter().enumerate() {
        let name = if *family == Family::Allowed {
            format!(
                "allowed[{}]",
                index - config.rules.dependencies.forbidden.len()
            )
        } else {
            rule.name().to_owned()
        };
        let finding = |code: &'static str, message: String| Finding {
            rule: name.clone(),
            code,
            message,
        };
        match &rule.meta.fix {
            None => out.push(finding(
                "no-fix",
                "no `fix`: add the imperative an agent should follow when the rule fires".into(),
            )),
            Some(fix) if restates(rule.name(), fix) => out.push(finding(
                "fix-restates-name",
                format!("`fix` \"{fix}\" only restates the rule name; say what to do instead"),
            )),
            Some(_) => {}
        }
        if options.require_comment_token
            && rule
                .meta
                .comment
                .as_deref()
                .and_then(crate::decision_token)
                .is_none()
        {
            out.push(finding(
                "missing-decision-token",
                "the comment has no decision token; add `adr:NNNN` or `plan:<slug>`".into(),
            ));
        }
        if let Some(message) = crate::normalize::type_only_on_dotnet(rule) {
            out.push(finding("type-only-on-dotnet", message));
        }
        let earlier = &rules[..index];
        if *family == Family::Allowed {
            if rule.from == FromRestriction::default() && rule.to == ToRestriction::default() {
                out.push(finding(
                    "allowed-admits-everything",
                    "this allowed entry has empty `from` and `to`, so every edge is allowed and the list checks nothing".into(),
                ));
            }
            if let Some((_, other)) = earlier
                .iter()
                .find(|(f, other)| *f == Family::Allowed && same_restrictions(other, rule))
            {
                let _ = other;
                out.push(finding(
                    "overlapping-allowed",
                    "this allowed entry admits exactly the edges an earlier entry admits; delete one".into(),
                ));
            }
        } else if let Some((_, other)) = earlier
            .iter()
            .find(|(f, other)| f == family && same_restrictions(other, rule))
        {
            out.push(finding(
                "shadowed",
                format!(
                    "the restrictions repeat those of `{}`, so this rule can never add a finding; merge the two",
                    other.name()
                ),
            ));
        }
        if let Some(graph) = graph {
            let before = out.len();
            graph_findings(rule, *family, graph, &mut out);
            for finding in &mut out[before..] {
                finding.rule.clone_from(&name);
            }
        }
    }
    out.extend(lifecycle_findings(config));
    out
}

/// The lifecycle fields of one rule, shorthand entry or ratchet
/// ([Wave 3, Step 12](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#23-steps-for-sub-wave-3c-presets-lifecycle-fields-snapshot-and-changelog)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleEntry<'a> {
    /// The name a finding reports: the rule's, or `allowed[n]` for an `allowed` entry.
    pub label: String,
    /// The name `replacedBy` would use for it.
    pub name: &'a str,
    /// `since`.
    pub since: Option<&'a str>,
    /// `deprecated`.
    pub deprecated: Option<&'a str>,
    /// `replacedBy`.
    pub replaced_by: Option<&'a str>,
}

impl<'a> LifecycleEntry<'a> {
    fn new(name: &'a str, fields: [&'a Option<String>; 3]) -> Self {
        let [since, deprecated, replaced_by] = fields.map(Option::as_deref);
        Self {
            label: name.to_owned(),
            name,
            since,
            deprecated,
            replaced_by,
        }
    }

    fn of(name: &'a str, lifecycle: &'a crate::elements::Lifecycle) -> Self {
        Self::new(
            name,
            [
                &lifecycle.since,
                &lifecycle.deprecated,
                &lifecycle.replaced_by,
            ],
        )
    }
}

/// The names of the rules the `layers` entries expanded to: each entry speaks for its rules.
fn expanded_layer_rules(config: &Config) -> BTreeSet<String> {
    config
        .rules
        .layers
        .iter()
        .flat_map(crate::shorthands::layer_rule_names)
        .collect()
}

/// The lifecycle fields of everything that carries them, once each: dependency rules in
/// evaluation order (less those a `layers` entry expanded to), then the `layers` entries, then
/// element, slice and diagram rules, then ratchets. An `independence` entry expands to one rule
/// of its own name, which stands for it.
pub fn lifecycle_entries(config: &Config) -> Vec<LifecycleEntry<'_>> {
    let rules = &config.rules;
    let forbidden = rules.dependencies.forbidden.len();
    let layered = expanded_layer_rules(config);
    let mut out: Vec<LifecycleEntry<'_>> = rules
        .all_dependency_rules()
        .enumerate()
        .filter(|(_, (_, rule))| !layered.contains(rule.name()))
        .map(|(index, (family, rule))| LifecycleEntry {
            label: if family == Family::Allowed {
                format!("allowed[{}]", index - forbidden)
            } else {
                rule.name().to_owned()
            },
            name: rule.name(),
            since: rule.meta.since.as_deref(),
            deprecated: rule.meta.deprecated.as_deref(),
            replaced_by: rule.meta.replaced_by.as_deref(),
        })
        .collect();
    out.extend(
        rules
            .layers
            .iter()
            .map(|l| LifecycleEntry::new(&l.name, [&l.since, &l.deprecated, &l.replaced_by])),
    );
    out.extend(
        rules
            .elements
            .iter()
            .map(|r| LifecycleEntry::of(&r.name, &r.lifecycle)),
    );
    out.extend(
        rules
            .slices
            .iter()
            .map(|r| LifecycleEntry::of(&r.name, &r.lifecycle)),
    );
    out.extend(
        rules
            .diagrams
            .iter()
            .map(|r| LifecycleEntry::of(&r.name, &r.lifecycle)),
    );
    out.extend(
        rules
            .ratchets
            .iter()
            .map(|r| LifecycleEntry::new(&r.name, [&r.since, &r.deprecated, &r.replaced_by])),
    );
    out
}

/// `replaced-by-unknown`, `replaced-by-self` and `since-after-deprecated` over every family.
pub fn lifecycle_findings(config: &Config) -> Vec<Finding> {
    let all = lifecycle_entries(config);
    let layered = expanded_layer_rules(config);
    let known: BTreeSet<&str> = all
        .iter()
        .map(|l| l.name)
        .chain(layered.iter().map(String::as_str))
        .chain(config.rules.independence.iter().map(|i| i.name.as_str()))
        .collect();
    let mut out = Vec::new();
    for rule in &all {
        let finding = |code: &'static str, message: String| Finding {
            rule: rule.label.clone(),
            code,
            message,
        };
        match rule.replaced_by {
            Some(next) if next == rule.name => out.push(finding(
                "replaced-by-self",
                "`replacedBy` names the rule itself; name the rule that takes over, or remove the key".into(),
            )),
            Some(next) if !known.contains(next) => out.push(finding(
                "replaced-by-unknown",
                format!(
                    "`replacedBy` names `{next}`, which is no rule, shorthand or ratchet of this configuration; add that rule or correct the name"
                ),
            )),
            _ => {}
        }
        if let (Some(since), Some(deprecated)) = (rule.since, rule.deprecated)
            && let (Some(a), Some(b)) = (
                crate::version::parse(since),
                crate::version::parse(deprecated),
            )
            && a > b
        {
            out.push(finding(
                "since-after-deprecated",
                format!(
                    "`since` {since} is later than `deprecated` {deprecated}; a rule is deprecated after it arrives, so correct one of the two"
                ),
            ));
        }
    }
    out
}

/// Renders findings for the terminal: `<code> <rule>: <message>`.
pub fn render(findings: &[Finding]) -> String {
    if findings.is_empty() {
        return "config lint: no findings\n".to_owned();
    }
    let mut out = String::new();
    for f in findings {
        let _ = writeln!(out, "{} {}: {}", f.code, f.rule, f.message);
    }
    let _ = writeln!(out, "config lint: {} finding(s)", findings.len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restating_is_detected() {
        assert!(restates("no-circular", "No circular."));
        assert!(restates(
            "no-cross-app-imports",
            "Do not do cross app imports"
        ));
        assert!(!restates(
            "no-circular",
            "Move the shared type into a third module."
        ));
        assert!(!restates("x", ""));
    }

    fn dependency(name: &str, lifecycle: [Option<&str>; 3]) -> Rule {
        let [since, deprecated, replaced_by] = lifecycle.map(|v| v.map(str::to_owned));
        Rule {
            meta: crate::RuleMeta {
                name: Some(name.into()),
                fix: Some("Move the import behind the gateway.".into()),
                since,
                deprecated,
                replaced_by,
                ..crate::RuleMeta::default()
            },
            ..Rule::default()
        }
    }

    fn codes(findings: &[Finding]) -> Vec<(String, &'static str)> {
        findings.iter().map(|f| (f.rule.clone(), f.code)).collect()
    }

    #[test]
    fn lifecycle_fields_are_linted_on_every_family() -> Result<(), crate::ConfigError> {
        let mut config = Config::default();
        let forbidden = &mut config.rules.dependencies.forbidden;
        forbidden.push(dependency(
            "old",
            [Some("1.2.0"), Some("2.0.0"), Some("new")],
        ));
        forbidden.push(dependency("new", [Some("2.0.0"), None, None]));
        forbidden.push(dependency("dangling", [None, Some("2.0.0"), Some("gone")]));
        forbidden.push(dependency("self", [None, None, Some("self")]));
        forbidden.push(dependency(
            "backwards",
            [Some("3.0.0"), Some("v2.9.9"), None],
        ));
        forbidden.push(dependency("retired", [None, Some("2.0.0"), None]));
        forbidden.push(dependency("calver", [Some("2027.1"), Some("2026.9"), None]));
        forbidden.push(dependency(
            "to-ratchet",
            [None, Some("2.0.0"), Some("budget")],
        ));
        forbidden.push(dependency("to-slice", [None, None, Some("slices-acyclic")]));
        config
            .rules
            .dependencies
            .allowed
            .push(dependency("not-in-allowed", [None, None, Some("nope")]));
        config.rules.ratchets.push(crate::model::Ratchet {
            name: "budget".into(),
            ..crate::model::Ratchet::default()
        });
        config.rules.slices = crate::elements::parse_slices(&serde_json::json!([
            { "name": "slices-acyclic", "matching": "A.(*)", "should": "beFreeOfCycles", "replacedBy": "missing" }
        ]))?;
        config.rules.elements = crate::elements::parse_elements(&serde_json::json!([
            { "name": "sealed", "select": { "kind": "class" }, "should": { "beSealed": true }, "since": "2.0.0", "deprecated": "1.0.0" }
        ]))?;
        config.rules.diagrams = crate::elements::parse_diagrams(&serde_json::json!([
            { "name": "diagram", "select": { "kind": "type" }, "adhereTo": "d.puml", "replacedBy": "old" }
        ]))?;
        let findings = lifecycle_findings(&config);
        assert_eq!(
            codes(&findings),
            [
                ("dangling".to_owned(), "replaced-by-unknown"),
                ("self".to_owned(), "replaced-by-self"),
                ("backwards".to_owned(), "since-after-deprecated"),
                ("allowed[0]".to_owned(), "replaced-by-unknown"),
                ("sealed".to_owned(), "since-after-deprecated"),
                ("slices-acyclic".to_owned(), "replaced-by-unknown"),
            ]
        );
        assert!(
            findings[0].message.contains("`gone`"),
            "{}",
            findings[0].message
        );
        assert!(findings[2].message.contains("3.0.0") && findings[2].message.contains("v2.9.9"));
        let all = lint(&config, None, LintOptions::default());
        assert!(
            codes(&all).ends_with(&codes(&findings)),
            "lint reports the lifecycle findings last"
        );
        Ok(())
    }

    #[test]
    fn shorthands_and_ratchets_are_linted_once_each() -> Result<(), crate::ConfigError> {
        let text = r#"
rules:
  layers:
    - { name: app-layers, layers: ["^ui/", "^domain/", "^db/"], since: "2.0.0", deprecated: "1.0.0", replacedBy: gone }
  independence:
    - { name: apart, pattern: "^src/([^/]+)/", replacedBy: app-layers }
  ratchets:
    - { name: budget, from: {}, to: {}, budget: b.json, replacedBy: "app-layers:2-to-1" }
    - { name: old-budget, from: {}, to: {}, budget: b.json, replacedBy: missing }
  dependencies:
    forbidden:
      - { name: r, fix: "Move it.", from: {}, to: {}, replacedBy: apart }
"#;
        let config = crate::load_text(
            text,
            crate::read::Syntax::Yaml,
            &std::env::temp_dir(),
            &crate::LoadOptions::default(),
        )?;
        let entries: Vec<(&str, Option<&str>)> = lifecycle_entries(&config)
            .iter()
            .map(|e| (e.name, e.deprecated))
            .collect();
        assert_eq!(
            entries,
            [
                ("r", None),
                ("apart", None),
                ("app-layers", Some("1.0.0")),
                ("budget", None),
                ("old-budget", None),
            ]
        );
        assert_eq!(
            codes(&lifecycle_findings(&config)),
            [
                ("app-layers".to_owned(), "replaced-by-unknown"),
                ("app-layers".to_owned(), "since-after-deprecated"),
                ("old-budget".to_owned(), "replaced-by-unknown"),
            ]
        );
        Ok(())
    }

    #[test]
    fn equal_versions_are_not_backwards() {
        let mut config = Config::default();
        config
            .rules
            .dependencies
            .forbidden
            .push(dependency("same", [Some("2.0.0"), Some("2.0.0"), None]));
        assert!(lifecycle_findings(&config).is_empty());
    }

    #[test]
    fn render_lists_findings() {
        let findings = [Finding {
            rule: "r".into(),
            code: "no-fix",
            message: "m".into(),
        }];
        assert_eq!(
            render(&findings),
            "no-fix r: m\nconfig lint: 1 finding(s)\n"
        );
        assert_eq!(render(&[]), "config lint: no findings\n");
    }
}
