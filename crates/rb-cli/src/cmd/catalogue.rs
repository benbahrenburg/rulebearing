//! Every rule of every family as one list: its name, its sentence, its `fix`, its decision
//! tokens and the tree it fences. `docs` renders it and `decisions` checks it.
//!
//! - Source: [design § Docs derived from the rules](../../../../docs/artifacts/design.md#docs-derived-from-the-rules-never-written-beside-them)
//!   ("one line per rule with its fence in words, its `fix`, and its decision link, grouped by
//!   the `from` tree")
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Requirement: [FR-CLI-02](../../../../docs/prd.md#fr-cli-02)
//!
//! Dependency rules take their sentence from `explain --plain` ([`crate::cmd::plain`]). Element,
//! slice and diagram rules, which `explain --plain` does not cover, are written from their
//! parsed expressions with the keys as the configuration spells them, so the sentence names the
//! same `ArchUnitNET` methods the rule does. Ratchets say what they count and that the count only
//! falls. Everything is in configuration order inside a group, and groups are sorted, so the
//! output is the same on every run.

use rb_config::elements::{ElementRule, Expr, Objects, Operand, Selector, Side, SliceCondition};
use rb_config::{Config, Family, Rule};
use rb_model::Severity;
use rb_model::options::Patterns;

use crate::cmd::plain;

/// The part of the rule set a rule fences, which is how `docs` groups rules.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// Files under a path, as `explain --plain` writes it (`crates/rb-extract-<x>/`).
    Tree(String),
    /// Every file: a rule whose selecting side has no path.
    Everything,
    /// Element rules over one kind.
    Elements(String),
    /// Slice rules.
    Slices,
    /// Diagram rules.
    Diagrams,
}

impl Group {
    /// The group's heading.
    pub fn heading(&self) -> String {
        match self {
            Self::Tree(path) => format!("`{path}`"),
            Self::Everything => "Every file".to_owned(),
            Self::Elements(kind) => format!("Element rules over `{kind}`"),
            Self::Slices => "Slice rules".to_owned(),
            Self::Diagrams => "Diagram rules".to_owned(),
        }
    }
}

/// One rule, whatever its family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The rule's name; `allowed` entries are `not-in-allowed (allowed[n])`.
    pub name: String,
    /// The family: `forbidden`, `allowed`, `required`, `elements`, `slices`, `diagrams` or
    /// `ratchets`.
    pub family: &'static str,
    /// The severity, when the family has one.
    pub severity: Option<Severity>,
    /// The fence in words.
    pub sentence: String,
    /// The imperative to follow when the rule fires.
    pub fix: Option<String>,
    /// Why the rule exists.
    pub comment: Option<String>,
    /// What it fences.
    pub group: Group,
}

impl Entry {
    /// The decision tokens in the comment.
    pub fn tokens(&self) -> Vec<String> {
        self.comment.as_deref().map(tokens).unwrap_or_default()
    }
}

/// Every decision token in `text`, in order of appearance, each once: `adr:NNNN` and
/// `plan:<slug>`, with the grammar [`rb_config::decision_token`] reads the first by.
pub fn tokens(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut found: Vec<(usize, String)> = Vec::new();
    for prefix in ["adr:", "plan:"] {
        for (start, _) in text.match_indices(prefix) {
            if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
                continue;
            }
            let rest = &text[start + prefix.len()..];
            let body: String = if prefix == "adr:" {
                rest.chars().take_while(char::is_ascii_digit).collect()
            } else {
                rest.chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                    .collect()
            };
            if !body.is_empty() {
                found.push((start, format!("{prefix}{body}")));
            }
        }
    }
    found.sort();
    let mut out: Vec<String> = Vec::new();
    for (_, token) in found {
        if !out.contains(&token) {
            out.push(token);
        }
    }
    out
}

fn joined(patterns: Option<&Patterns>) -> Option<String> {
    patterns.map(Patterns::joined).filter(|p| !p.is_empty())
}

/// A pattern as a path, without the backticks `explain --plain` puts round it.
fn tree(pattern: &str) -> String {
    let text = plain::humanize(pattern, "x");
    text.trim_matches('`').to_owned()
}

fn group_of(path: Option<String>) -> Group {
    path.map_or(Group::Everything, |p| Group::Tree(tree(&p)))
}

fn dependency_entry(family: Family, index: usize, rule: &Rule, config: &Config) -> Entry {
    let selecting = rule
        .module
        .as_ref()
        .and_then(|m| joined(m.path.as_ref()))
        .or_else(|| joined(rule.from.path.as_ref()));
    let (name, severity) = if family == Family::Allowed {
        (
            format!("not-in-allowed (allowed[{index}])"),
            config.rules.dependencies.allowed_severity,
        )
    } else {
        (rule.name().to_owned(), Some(rule.severity()))
    };
    Entry {
        name,
        family: family.as_str(),
        severity,
        sentence: plain::sentence(family, rule),
        fix: rule.meta.fix.clone(),
        comment: rule.meta.comment.clone(),
        group: group_of(selecting),
    }
}

fn quoted(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("\"{n}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

fn objects(objects: &Objects) -> String {
    match objects {
        Objects::Names(names) => quoted(names),
        Objects::Selector(selector) => selection(selector),
    }
}

/// A selector in words: `class`, or `class where haveNameEndingWith "Service"`.
pub fn selection(selector: &Selector) -> String {
    let mut text = selector.kind.as_str().to_owned();
    if !selector.languages.is_empty() {
        let languages: Vec<&str> = selector.languages.iter().map(|l| l.as_str()).collect();
        text = format!("{} {text}", languages.join(" or "));
    }
    if let Some(filter) = &selector.where_ {
        text = format!("{text} where {}", expression(filter, Side::Where));
    }
    text
}

/// An expression with its keys as the configuration spells them.
pub fn expression(expr: &Expr, side: Side) -> String {
    let nested = |items: &[Expr], word: &str| {
        items
            .iter()
            .map(|e| match e {
                Expr::All(_) | Expr::Any(_) => format!("({})", expression(e, side)),
                _ => expression(e, side),
            })
            .collect::<Vec<_>>()
            .join(word)
    };
    match expr {
        Expr::All(items) if items.is_empty() => "nothing".to_owned(),
        Expr::All(items) => nested(items, " and "),
        Expr::Any(items) => nested(items, " or "),
        Expr::Not(inner) => format!("not ({})", expression(inner, side)),
        Expr::Test(test) => {
            let key = &test.key;
            match &test.operand {
                Operand::Flag => {
                    let (_, spelled_negated, _) = rb_config::elements::split_key(key, side);
                    if spelled_negated == test.negated {
                        key.clone()
                    } else {
                        format!("{key}: false")
                    }
                }
                Operand::Names(names) => format!("{key} {}", quoted(names)),
                Operand::Pattern(pattern) => format!("{key} /{pattern}/"),
                Operand::Objects(o) => format!("{key} {}", objects(o)),
                Operand::Attribute {
                    attribute,
                    positional,
                    named,
                } => {
                    let mut arguments: Vec<String> = positional.clone();
                    arguments.extend(named.iter().map(|(k, v)| format!("{k} = {v}")));
                    let name = attribute.as_ref().map(objects).unwrap_or_default();
                    format!("{key} {name}({})", arguments.join(", "))
                }
                Operand::Diagram(path) => format!("{key} {path}"),
            }
        }
    }
}

/// An element rule in words.
pub fn element_sentence(rule: &ElementRule) -> String {
    format!(
        "Every {} must satisfy `{}`.",
        wrap_selection(&rule.select),
        expression(&rule.should, Side::Should)
    )
}

fn wrap_selection(selector: &Selector) -> String {
    let words = selection(selector);
    match words.split_once(" where ") {
        Some((kind, filter)) => format!("`{kind}` where `{filter}`"),
        None => format!("`{words}`"),
    }
}

/// Every rule in the configuration, in family order, each in configuration order.
pub fn entries(config: &Config) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut allowed_index = 0usize;
    for (family, rule) in config.rules.all_dependency_rules() {
        let index = if family == Family::Allowed {
            allowed_index += 1;
            allowed_index - 1
        } else {
            0
        };
        out.push(dependency_entry(family, index, rule, config));
    }
    for ratchet in &config.rules.ratchets {
        let side = |p: Option<&Patterns>| {
            joined(p).map_or_else(|| "any file".to_owned(), |p| plain::humanize(&p, "x"))
        };
        out.push(Entry {
            name: ratchet.name.clone(),
            family: "ratchets",
            severity: None,
            sentence: format!(
                "Imports from {} to {} may not outnumber the ceiling in `{}`, which only falls.",
                side(ratchet.from.path.as_ref()),
                side(ratchet.to.path.as_ref()),
                ratchet.budget
            ),
            fix: ratchet.fix.clone(),
            comment: ratchet.comment.clone(),
            group: group_of(joined(ratchet.from.path.as_ref())),
        });
    }
    for rule in &config.rules.elements {
        out.push(Entry {
            name: rule.name.clone(),
            family: "elements",
            severity: Some(rule.severity),
            sentence: element_sentence(rule),
            fix: rule.fix.clone(),
            comment: rule.comment.clone(),
            group: Group::Elements(rule.select.kind.as_str().to_owned()),
        });
    }
    for rule in &config.rules.slices {
        let conditions: Vec<&str> = rule
            .should
            .iter()
            .map(|c| match c {
                SliceCondition::NotDependOnEachOther => "not depend on each other",
                SliceCondition::BeFreeOfCycles => "be free of cycles",
            })
            .collect();
        out.push(Entry {
            name: rule.name.clone(),
            family: "slices",
            severity: Some(rule.severity),
            sentence: format!(
                "The slices of `{}` must {}.",
                rule.matching,
                conditions.join(" and ")
            ),
            fix: rule.fix.clone(),
            comment: rule.comment.clone(),
            group: Group::Slices,
        });
    }
    for rule in &config.rules.diagrams {
        out.push(Entry {
            name: rule.name.clone(),
            family: "diagrams",
            severity: Some(rule.severity),
            sentence: format!(
                "Every {} must follow the dependencies drawn in `{}`.",
                wrap_selection(&rule.select),
                rule.adhere_to
            ),
            fix: rule.fix.clone(),
            comment: rule.comment.clone(),
            group: Group::Diagrams,
        });
    }
    out
}

/// The entries grouped, groups sorted, entries in configuration order inside each.
pub fn grouped(entries: &[Entry]) -> Vec<(Group, Vec<&Entry>)> {
    let mut groups: std::collections::BTreeMap<Group, Vec<&Entry>> =
        std::collections::BTreeMap::new();
    for entry in entries {
        groups.entry(entry.group.clone()).or_default().push(entry);
    }
    groups.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(value: &serde_json::Value) -> Config {
        rb_config::load_text(
            &value.to_string(),
            rb_config::read::Syntax::Yaml,
            std::path::Path::new("."),
            &rb_config::LoadOptions::default(),
        )
        .unwrap_or_default()
    }

    #[test]
    fn tokens_are_every_decision_in_order_once() {
        assert_eq!(
            tokens("see adr:0010 and plan:wave-2, again adr:0010; adr:0014"),
            ["adr:0010", "plan:wave-2", "adr:0014"]
        );
        assert!(tokens("xadr:0010 adr: plan:").is_empty());
        assert_eq!(tokens("adr:7"), ["adr:7"]);
    }

    #[test]
    fn every_family_has_a_sentence_and_a_group() {
        let c = config(&json!({
            "forbidden": [{ "name": "f", "severity": "error", "comment": "adr:0001",
                "from": { "path": "^src/a/" }, "to": { "path": "^src/b/" } }],
            "allowed": [{ "from": {}, "to": { "path": "^src/" } }],
            "allowedSeverity": "warn",
            "required": [{ "name": "r", "module": { "path": "^src/pages/" },
                "to": { "path": "^src/auth" } }],
            "rules": {
                "ratchets": [{ "name": "q", "from": { "path": "^src/a/" }, "to": {}, "budget": "b.json" }],
                "elements": [{ "name": "e", "select": { "kind": "class", "language": "dotnet",
                    "where": { "haveNameEndingWith": "Service", "areNotPublic": true } },
                    "should": { "any": [{ "beSealed": false }, { "notDependOnAny": ["X"] }] } }],
                "slices": [{ "name": "s", "matching": "App.(*)", "should": ["beFreeOfCycles", "notDependOnEachOther"] }],
                "diagrams": [{ "name": "d", "select": { "kind": "type" }, "adhereTo": "a.puml" }]
            }
        }));
        let all = entries(&c);
        let names: Vec<&str> = all.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            ["f", "not-in-allowed (allowed[0])", "r", "q", "e", "s", "d"]
        );
        assert_eq!(all[0].group, Group::Tree("src/a/".into()));
        assert_eq!(all[1].group, Group::Everything);
        assert_eq!(all[1].severity, Some(Severity::Warn));
        assert_eq!(all[2].group, Group::Tree("src/pages/".into()));
        assert!(all[3].sentence.contains("`b.json`"), "{}", all[3].sentence);
        assert_eq!(
            all[4].sentence,
            "Every `dotnet class` where `haveNameEndingWith \"Service\" and areNotPublic` must satisfy `(beSealed: false) or (notDependOnAny \"X\")`."
                .replace("(beSealed: false)", "beSealed: false")
                .replace("(notDependOnAny \"X\")", "notDependOnAny \"X\"")
        );
        assert_eq!(
            all[5].sentence,
            "The slices of `App.(*)` must be free of cycles and not depend on each other."
        );
        assert!(all[6].sentence.contains("`a.puml`"));
        let groups = grouped(&all);
        let headings: Vec<String> = groups.iter().map(|(g, _)| g.heading()).collect();
        assert_eq!(
            headings,
            [
                "`src/a/`",
                "`src/pages/`",
                "Every file",
                "Element rules over `class`",
                "Slice rules",
                "Diagram rules"
            ]
        );
        assert_eq!(all[0].tokens(), ["adr:0001"]);
    }
}
