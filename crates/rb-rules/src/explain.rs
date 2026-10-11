//! `explain <rule>`: a rule, its sentence, its fix, what it matches and the first ten edges it
//! flagged, as text or JSON. The command line and the playground print the same bytes because
//! both call this module.
//!
//! - Source: [design § Questions an agent can ask](../../../docs/artifacts/design.md#questions-an-agent-can-ask-before-it-writes-the-import),
//!   [design § The agentic engineering hat](../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two)
//! - Decision: [ADR-0021](../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Plans: [Wave 1, Step 14](../../../docs/plans/implemented/0001-wave-1-typescript-parity.md#step-14-rules---json-explain-explain---plain-test-can-import-1e)
//!   (`explain`, `explain --plain`), [Wave 4, Step 2](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables)
//!   (moved out of `rb-cli` for the playground's `explain(config, cruise_json, rule)`,
//!   [ADR-0073](../../../docs/adr/0073-extractors-read-files-through-rb-model.md))
//! - Requirements: [FR-CLI-01](../../../docs/prd.md#fr-cli-01), [FR-CLI-06](../../../docs/prd.md#fr-cli-06),
//!   [FR-REACH-01](../../../docs/prd.md#fr-reach-01)
//!
//! A rule is named as it is written, or an `allowed` entry as `allowed[n]`. The JSON form is the
//! rule as `rules --json` lists it ([`rule_json`]), its `sentence` and, when a graph was read,
//! the `firstEdges` the text lists.

use std::fmt::Write as _;

use rb_config::{Config, Family, Rule};
use serde_json::{Value, json};

use crate::plain;
use crate::{Evaluation, RuleStats};

/// How many flagged edges `explain` lists.
pub const FIRST_EDGES: usize = 10;

/// Every dependency rule with its list, in evaluation order.
pub fn listed(config: &Config) -> Vec<(Family, &Rule)> {
    config.rules.all_dependency_rules().collect()
}

/// A rule as `rules --json` prints it, with its statistics when a graph was evaluated.
pub fn rule_json(family: Family, rule: &Rule, stats: Option<&RuleStats>) -> Value {
    let severity = if family == Family::Allowed {
        Value::Null
    } else {
        json!(rule.severity())
    };
    json!({
        "name": stats.map_or_else(|| rule.name().to_owned(), |s| s.name.clone()),
        "family": family.as_str(),
        "severity": severity,
        "comment": rule.meta.comment,
        "fix": rule.meta.fix,
        "since": rule.meta.since,
        "deprecated": rule.meta.deprecated,
        "replacedBy": rule.meta.replaced_by,
        "from": serde_json::to_value(&rule.from).unwrap_or(Value::Null),
        "to": serde_json::to_value(&rule.to).unwrap_or(Value::Null),
        "module": rule.module.as_ref().and_then(|m| serde_json::to_value(m).ok()),
        "select": Value::Null,
        "fromMatches": stats.map(|s| s.from_matches),
        "toMatches": stats.map(|s| s.to_matches),
        "violations": stats.map(|s| s.violations),
    })
}

/// A rule `explain` was asked about that the configuration does not have (the command line's
/// exit 3).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("no rule `{name}`; the rules are {}", rules.join(", "))]
pub struct NoRule {
    /// The name asked for.
    pub name: String,
    /// Every rule's name, in evaluation order.
    pub rules: Vec<String>,
}

/// The rule `explain` describes, found in its configuration.
#[derive(Debug, Clone, Copy)]
pub struct Explained<'c> {
    index: usize,
    family: Family,
    rule: &'c Rule,
}

/// Finds the rule named `name` (or `allowed[n]`) in `config`.
///
/// # Errors
/// [`NoRule`] naming every rule there is.
pub fn find<'c>(config: &'c Config, name: &str) -> Result<Explained<'c>, NoRule> {
    let listed = listed(config);
    let allowed_offset = config.rules.dependencies.forbidden.len();
    listed
        .iter()
        .enumerate()
        .find(|(i, (family, rule))| {
            rule.name() == name
                || (*family == Family::Allowed
                    && i.checked_sub(allowed_offset)
                        .is_some_and(|n| name == format!("allowed[{n}]")))
        })
        .map(|(index, (family, rule))| Explained {
            index,
            family: *family,
            rule,
        })
        .ok_or_else(|| NoRule {
            name: name.to_owned(),
            rules: listed.iter().map(|(_, r)| r.name().to_owned()).collect(),
        })
}

impl Explained<'_> {
    /// The rule's place in evaluation order: its index into [`Evaluation::rule_stats`].
    pub fn index(&self) -> usize {
        self.index
    }

    /// The rule's one English sentence ([`plain::sentence`]).
    pub fn sentence(&self) -> String {
        plain::sentence(self.family, self.rule)
    }

    /// `explain --plain`: the sentence, or with `json` the rule and its sentence, no graph read.
    pub fn plain(&self, json: bool) -> String {
        let sentence = self.sentence();
        if json {
            json_text(rule_json(self.family, self.rule, None), &sentence, None)
        } else {
            format!("{sentence}\n")
        }
    }

    /// `explain` over an evaluated graph: the rule, its sentence, comment, fix and selectors,
    /// its statistics and the first [`FIRST_EDGES`] edges it flagged; with `json`, the same as
    /// JSON.
    pub fn with_graph(&self, evaluation: &Evaluation, json: bool) -> String {
        let (family, rule) = (self.family, self.rule);
        let sentence = self.sentence();
        let stats = evaluation.rule_stats.get(self.index);
        let first: Vec<(&str, &str)> = evaluation
            .document
            .summary
            .violations
            .iter()
            .filter(|v| v.rule.name == rule.name())
            .take(FIRST_EDGES)
            .map(|v| (v.from.as_str(), v.to.as_str()))
            .collect();
        if json {
            return json_text(rule_json(family, rule, stats), &sentence, Some(&first));
        }
        let mut out = String::new();
        let _ = writeln!(
            out,
            "{}  ({}, {})",
            rule.name(),
            family.as_str(),
            rule.severity()
        );
        let _ = writeln!(out, "  {sentence}");
        if let Some(comment) = &rule.meta.comment {
            let _ = writeln!(out, "  why: {comment}");
        }
        let _ = writeln!(out, "  fix: {}", rule.meta.fix.as_deref().unwrap_or("-"));
        let selectors = rule_json(family, rule, None);
        for side in ["from", "to", "module", "select"] {
            if let Some(value) = selectors.get(side).filter(|v| !v.is_null()) {
                let _ = writeln!(out, "  {side}: {value}");
            }
        }
        if let Some(stats) = stats {
            let _ = writeln!(
                out,
                "  matches: from {} modules, to {}; {} violations",
                stats.from_matches, stats.to_matches, stats.violations
            );
        }
        let edges: Vec<String> = first
            .iter()
            .map(|(from, to)| format!("    {from} -> {to}"))
            .collect();
        if !edges.is_empty() {
            let _ = writeln!(out, "  first edges:\n{}", edges.join("\n"));
        }
        out
    }
}

/// `explain --json`: the rule as [`rule_json`] gives it, its sentence and, when the graph was
/// read, the first edges it flagged; pretty-printed with a final newline.
pub fn json_text(mut value: Value, sentence: &str, first: Option<&[(&str, &str)]>) -> String {
    if let Some(object) = value.as_object_mut() {
        object.insert("sentence".into(), json!(sentence));
        if let Some(first) = first {
            let edges: Vec<Value> = first
                .iter()
                .map(|(from, to)| json!({ "from": from, "to": to }))
                .collect();
            object.insert("firstEdges".into(), Value::Array(edges));
        }
    }
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_model::{Dependency, GraphDocument, Module, ModuleSystem};

    type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    fn config() -> Result<Config> {
        let rules = json!({
            "forbidden": [
                { "name": "not-to-lib", "severity": "warn", "comment": "lib is private",
                  "fix": "import from src/api instead",
                  "from": { "path": "^src/" }, "to": { "path": "^lib/" } },
                { "name": "no-circular", "severity": "error", "from": {}, "to": { "circular": true } }
            ],
            "allowed": [
                { "from": { "path": "^src/" }, "to": { "path": "^src/" } },
                { "from": { "path": "^lib/" }, "to": { "path": "^lib/" } }
            ]
        });
        Ok(rb_config::load::from_canonical(
            rules.as_object().cloned().unwrap_or_default(),
            rb_config::CompatMode::Native,
        )?)
    }

    /// Twelve edges from `src/` to `lib/`, so the list stops at ten.
    fn evaluated(config: &Config) -> Result<Evaluation> {
        let targets: Vec<String> = (0..12).map(|i| format!("lib/m{i:02}.ts")).collect();
        let mut modules = vec![Module {
            dependencies: targets
                .iter()
                .map(|t| Dependency::new(t.clone(), t.clone(), ModuleSystem::Es6))
                .collect(),
            ..Module::new("src/a.ts")
        }];
        modules.extend(targets.iter().map(|t| Module::new(t.clone())));
        let document = GraphDocument {
            modules,
            ..GraphDocument::default()
        };
        Ok(crate::evaluate(
            document,
            config,
            &crate::EvalOptions {
                liveness: false,
                validate: true,
                metrics: false,
                today: chrono::NaiveDate::default(),
                args: Vec::new(),
                options_used: serde_json::Map::new(),
            },
        )?)
    }

    #[test]
    fn a_rule_is_found_by_name_or_allowed_position() -> Result {
        let config = config()?;
        let names: Vec<&str> = listed(&config).iter().map(|(_, r)| r.name()).collect();
        assert_eq!(names.len(), 4);
        assert_eq!(&names[..2], ["not-to-lib", "no-circular"]);
        assert_eq!(find(&config, "not-to-lib")?.index(), 0);
        assert_eq!(find(&config, "no-circular")?.index(), 1);
        assert_eq!(find(&config, "allowed[0]")?.index(), 2);
        assert_eq!(find(&config, "allowed[1]")?.index(), 3);
        let missing = find(&config, "allowed[2]").err();
        assert_eq!(
            missing.as_ref().map(|e| e.name.as_str()),
            Some("allowed[2]")
        );
        let none = find(&config, "nope").err().map(|e| e.to_string());
        assert_eq!(
            none,
            Some(format!(
                "no rule `nope`; the rules are {}",
                names.join(", ")
            ))
        );
        Ok(())
    }

    #[test]
    fn plain_is_the_sentence_alone() -> Result {
        let config = config()?;
        let rule = find(&config, "not-to-lib")?;
        let sentence = "Files matching `src/` may not import files matching `lib/`.";
        assert_eq!(rule.sentence(), sentence);
        assert_eq!(rule.plain(false), format!("{sentence}\n"));
        let as_json: Value = serde_json::from_str(&rule.plain(true))?;
        assert_eq!(as_json["sentence"], sentence);
        assert_eq!(as_json["name"], "not-to-lib");
        assert_eq!(as_json["severity"], "warn");
        assert!(as_json.get("firstEdges").is_none());
        assert!(as_json["fromMatches"].is_null());
        Ok(())
    }

    #[test]
    fn with_a_graph_the_statistics_and_the_first_ten_edges_are_listed() -> Result {
        let config = config()?;
        let evaluation = evaluated(&config)?;
        let text = find(&config, "not-to-lib")?.with_graph(&evaluation, false);
        let edges = (0..10).fold(String::new(), |mut all, i| {
            let _ = writeln!(all, "    src/a.ts -> lib/m{i:02}.ts");
            all
        });
        assert_eq!(
            text,
            format!(
                "not-to-lib  (forbidden, warn)\n  \
                 Files matching `src/` may not import files matching `lib/`.\n  \
                 why: lib is private\n  \
                 fix: import from src/api instead\n  \
                 from: {{\"path\":\"^src/\"}}\n  \
                 to: {{\"path\":\"^lib/\"}}\n  \
                 matches: from 1 modules, to 12; 12 violations\n  \
                 first edges:\n{edges}"
            )
        );
        let as_json: Value =
            serde_json::from_str(&find(&config, "not-to-lib")?.with_graph(&evaluation, true))?;
        assert_eq!(as_json["violations"], 12);
        assert_eq!(as_json["fromMatches"], 1);
        assert_eq!(as_json["toMatches"], 12);
        assert_eq!(
            as_json["firstEdges"].as_array().map(Vec::len),
            Some(FIRST_EDGES)
        );
        assert_eq!(as_json["firstEdges"][9]["to"], "lib/m09.ts");
        Ok(())
    }

    #[test]
    fn a_rule_without_a_comment_fix_or_edges_says_so() -> Result {
        let config = config()?;
        let evaluation = evaluated(&config)?;
        let text = find(&config, "no-circular")?.with_graph(&evaluation, false);
        assert!(
            text.starts_with("no-circular  (forbidden, error)\n"),
            "{text}"
        );
        assert!(!text.contains("why:"), "{text}");
        assert!(text.contains("\n  fix: -\n"), "{text}");
        assert!(
            text.contains("matches: from 13 modules, to 0; 0 violations"),
            "{text}"
        );
        assert!(!text.contains("first edges"), "{text}");
        let allowed = find(&config, "allowed[0]")?.plain(true);
        let allowed: Value = serde_json::from_str(&allowed)?;
        assert!(
            allowed["severity"].is_null(),
            "an allowed entry has no severity"
        );
        assert_eq!(allowed["family"], "allowed");
        Ok(())
    }

    #[test]
    fn rule_json_names_every_field() -> Result {
        let config = config()?;
        let (family, rule) = listed(&config)[0];
        let stats = RuleStats {
            name: "renamed".into(),
            family,
            from_matches: 3,
            to_matches: 4,
            violations: 5,
        };
        let value = rule_json(family, rule, Some(&stats));
        let keys: Vec<&str> = value
            .as_object()
            .into_iter()
            .flat_map(|o| o.keys().map(String::as_str))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            [
                "comment",
                "deprecated",
                "family",
                "fix",
                "from",
                "fromMatches",
                "module",
                "name",
                "replacedBy",
                "select",
                "severity",
                "since",
                "to",
                "toMatches",
                "violations"
            ]
        );
        assert_eq!(value["name"], "renamed");
        assert_eq!(value["fix"], "import from src/api instead");
        assert_eq!(
            (
                &value["fromMatches"],
                &value["toMatches"],
                &value["violations"]
            ),
            (&json!(3), &json!(4), &json!(5))
        );
        assert!(value["module"].is_null() && value["select"].is_null());
        Ok(())
    }

    #[test]
    fn json_text_adds_the_sentence_and_edges_and_ends_with_a_newline() -> Result {
        // Key order follows serde_json's map, which the binary builds insertion-ordered; the
        // value and the pretty layout are what this crate decides.
        let text = json_text(json!({ "name": "r" }), "S.", Some(&[("a", "b")]));
        let value: Value = serde_json::from_str(&text)?;
        assert_eq!(
            value,
            json!({ "name": "r", "sentence": "S.", "firstEdges": [{ "from": "a", "to": "b" }] })
        );
        assert!(
            text.ends_with("\n}\n") && text.contains("\n  \"name\": \"r\""),
            "{text}"
        );
        let without: Value = serde_json::from_str(&json_text(json!({ "name": "r" }), "S.", None))?;
        assert_eq!(without, json!({ "name": "r", "sentence": "S." }));
        assert_eq!(json_text(json!("x"), "S.", None), "\"x\"\n");
        Ok(())
    }
}
