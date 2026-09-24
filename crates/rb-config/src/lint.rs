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
