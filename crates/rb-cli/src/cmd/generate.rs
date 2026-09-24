//! `rulebearing test --generate [RULE] [--force]`: a rule's `examples`, written from the edges it
//! matches and does not match today.
//!
//! - Source: [design § The agentic engineering hat](../../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two)
//!   ("an agent that proposes a rule gets its fixtures for free and the rule ships with proof")
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//!   (a bounded sample of each; refuses to overwrite non-empty examples without `--force`)
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md) decision 2
//! - Requirement: [FR-CLI-01](../../../../docs/prd.md#fr-cli-01) (`test --generate`)
//!
//! For each rule, `forbidden` examples come from the rule's violations over the graph and
//! `allowed` ones from edges it does not flag, those leaving the rule's own `from` tree first.
//! Every candidate is proven the way `rulebearing test` will prove it, against a graph holding
//! that edge alone, and one that does not behave there is not written: a cycle or reachability
//! violation needs more than one edge, so such a rule gets only the examples a single edge can
//! show, and says so. At most [`SAMPLE`] of each are written.
//!
//! The configuration is edited in place, as text, so its comments and layout survive: the rule's
//! `examples:` block is replaced, or added at the end of the rule's mapping. That needs a YAML
//! configuration with the rule written as a block mapping; any other file is left alone and the
//! block is printed to paste, with exit 3. The file is loaded again after the edit and restored
//! if the load fails or reads different examples back.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use rb_config::{Config, Family, Rule};
use rb_model::GraphDocument;
use rb_rules::matchers::pattern;
use rb_rules::{EvalOptions, evaluate};

use crate::cmd::test_rules::{TestArgs, flags, only};
use crate::context::Context;
use crate::{Outcome, RunExit, cache, configure};

/// How many examples of each kind are written per rule.
pub const SAMPLE: usize = 3;
/// How many edges are tried for the `allowed` sample before giving up.
const ATTEMPTS: usize = 200;

/// The examples generated for one rule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Generated {
    /// Edges the rule must allow.
    pub allowed: Vec<String>,
    /// Edges the rule must flag.
    pub forbidden: Vec<String>,
}

fn every_edge(document: &GraphDocument) -> Vec<(String, String)> {
    let mut edges: Vec<(String, String)> = document
        .modules
        .iter()
        .flat_map(|m| {
            m.dependencies
                .iter()
                .map(|d| (m.source.clone(), d.resolved.clone()))
        })
        .collect();
    edges.sort();
    edges.dedup();
    edges
}

/// The examples for `rule` from `document`.
///
/// # Errors
/// The engine's message.
pub fn examples(
    rule: &Rule,
    family: Family,
    config: &Config,
    document: &GraphDocument,
    today: chrono::NaiveDate,
) -> Result<Generated, String> {
    let single = only(rule, family, config);
    let options = EvalOptions {
        liveness: false,
        today,
        ..EvalOptions::default()
    };
    let evaluation = evaluate(document.clone(), &single, &options).map_err(|e| e.to_string())?;
    let flagged: BTreeSet<(String, String)> = evaluation
        .violations()
        .iter()
        .map(|v| (v.from.clone(), v.to.clone()))
        .collect();
    let forbidden: Vec<String> = flagged
        .iter()
        .filter(|(from, to)| from != to)
        .filter(|edge| {
            flags(
                rule,
                family,
                config,
                &(edge.0.clone(), edge.1.clone()),
                today,
            )
        })
        .take(SAMPLE)
        .map(|(from, to)| format!("{from} -> {to}"))
        .collect();
    let selecting = rule
        .module
        .as_ref()
        .and_then(|m| pattern(m.path.as_ref()))
        .or_else(|| pattern(rule.from.path.as_ref()));
    let mut candidates: Vec<(String, String)> = every_edge(document)
        .into_iter()
        .filter(|e| !flagged.contains(e) && e.0 != e.1)
        .collect();
    // Edges out of the rule's own tree first: they show what the rule lets through.
    candidates.sort_by_key(|(from, _)| {
        !selecting
            .as_deref()
            .is_some_and(|p| rb_rules::patterns::test(p, from))
    });
    let allowed: Vec<String> = candidates
        .into_iter()
        .take(ATTEMPTS)
        .filter(|edge| !flags(rule, family, config, edge, today))
        .take(SAMPLE)
        .map(|(from, to)| format!("{from} -> {to}"))
        .collect();
    Ok(Generated { allowed, forbidden })
}

/// Whether a line is `name: <rule>` (after an optional `- `), quoted or not, with or without a
/// trailing comment.
fn names(line: &str, rule: &str) -> bool {
    let content = line.trim_start();
    let content = content.strip_prefix('-').map_or(content, str::trim_start);
    let Some(value) = content.strip_prefix("name:") else {
        return false;
    };
    let value = value.trim();
    let unquoted = if let Some(inner) = value.strip_prefix('"') {
        inner.split('"').next().unwrap_or_default()
    } else if let Some(inner) = value.strip_prefix('\'') {
        inner.split('\'').next().unwrap_or_default()
    } else {
        value.split(" #").next().unwrap_or_default().trim()
    };
    unquoted == rule
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The column a mapping key starts at on `line`, past a leading `- `.
fn key_column(line: &str) -> usize {
    let lead = indent(line);
    let rest = &line[lead..];
    match rest.strip_prefix('-') {
        Some(after) => lead + 1 + (after.len() - after.trim_start().len()),
        None => lead,
    }
}

/// The key column of the item a `- ` opens on line `i`: on the same line, or, for a `-` alone on
/// its line, the next line's.
fn item_column(lines: &[&str], i: usize) -> usize {
    let after_dash = lines[i].trim_start().strip_prefix('-');
    if after_dash.is_some_and(|rest| rest.trim().is_empty()) {
        return lines
            .iter()
            .skip(i + 1)
            .find(|l| significant(l))
            .map_or(indent(lines[i]) + 2, |l| indent(l));
    }
    key_column(lines[i])
}

fn flow(list: &[String]) -> String {
    let items: Vec<String> = list
        .iter()
        .map(|e| serde_json::to_string(e).unwrap_or_default())
        .collect();
    format!("[{}]", items.join(", "))
}

/// The `examples:` block at `column`.
pub fn block(column: usize, generated: &Generated) -> Vec<String> {
    let pad = " ".repeat(column);
    let mut lines = vec![format!("{pad}examples:")];
    if !generated.forbidden.is_empty() {
        lines.push(format!("{pad}  forbidden: {}", flow(&generated.forbidden)));
    }
    if !generated.allowed.is_empty() {
        lines.push(format!("{pad}  allowed: {}", flow(&generated.allowed)));
    }
    lines
}

/// Which rule of the text to edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locator<'a> {
    /// The rule with this `name:`.
    Name(&'a str),
    /// The n-th item of the `allowed:` list, which has no name of its own.
    Allowed(usize),
}

fn significant(line: &str) -> bool {
    !line.trim().is_empty() && !line.trim_start().starts_with('#')
}

/// The rule's first line, the line to measure its keys from, and its key column.
fn locate(lines: &[&str], locator: Locator<'_>) -> Result<(usize, usize), String> {
    match locator {
        Locator::Name(rule) => {
            let at = lines
                .iter()
                .position(|l| names(l, rule))
                .ok_or_else(|| format!("no rule written as `name: {rule}` in a block mapping"))?;
            let column = key_column(lines[at]);
            // The item starts at the `- ` whose key column is the name's.
            let start = (0..=at)
                .rev()
                .find(|&i| {
                    lines[i].trim_start().starts_with('-') && item_column(lines, i) == column
                })
                .unwrap_or(at);
            Ok((start, column))
        }
        Locator::Allowed(n) => {
            let key = lines
                .iter()
                .position(|l| {
                    let t = l.trim();
                    t == "allowed:" || t.starts_with("allowed: #")
                })
                .ok_or("no `allowed:` list written in block form")?;
            let mut items = Vec::new();
            let mut dash: Option<usize> = None;
            for (i, line) in lines.iter().enumerate().skip(key + 1) {
                if !significant(line) {
                    continue;
                }
                let is_dash = line.trim_start().starts_with('-');
                match dash {
                    None if is_dash && indent(line) >= indent(lines[key]) => {
                        dash = Some(indent(line));
                        items.push(i);
                    }
                    Some(d) if is_dash && indent(line) == d => items.push(i),
                    Some(d) if indent(line) > d => {}
                    _ => break,
                }
            }
            let start = *items
                .get(n)
                .ok_or_else(|| format!("`allowed:` has no item {n} in block form"))?;
            if lines[start]
                .trim_start()
                .trim_start_matches('-')
                .trim_start()
                .starts_with('{')
            {
                return Err(format!("allowed[{n}] is written in flow form"));
            }
            Ok((start, item_column(lines, start)))
        }
    }
}

/// `text` with the rule's `examples:` replaced or added.
///
/// # Errors
/// A message when the rule is not in the text as a block mapping.
pub fn set_examples(
    text: &str,
    locator: Locator<'_>,
    generated: &Generated,
) -> Result<String, String> {
    let lines: Vec<&str> = text.lines().collect();
    let (start, column) = locate(&lines, locator)?;
    let at = start;
    let end = (at + 1..lines.len())
        .find(|&i| significant(lines[i]) && indent(lines[i]) < column)
        .unwrap_or(lines.len());
    let existing = (start..end).find(|&i| {
        key_column(lines[i]) == column
            && lines[i][column..].starts_with("examples:")
            && (i == start || indent(lines[i]) == column)
    });
    let new_block = block(column, generated);
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + new_block.len());
    let (cut_from, cut_to) = if let Some(e) = existing {
        let stop = (e + 1..end)
            .find(|&i| significant(lines[i]) && indent(lines[i]) <= column)
            .unwrap_or(end);
        (e, stop)
    } else {
        let last = (start..end)
            .rev()
            .find(|&i| significant(lines[i]))
            .unwrap_or(start);
        (last + 1, last + 1)
    };
    if existing == Some(start) {
        return Err("the rule opens with `- examples:`; move another key first".to_owned());
    }
    out.extend(lines[..cut_from].iter().map(|l| (*l).to_owned()));
    out.extend(new_block);
    out.extend(lines[cut_to..].iter().map(|l| (*l).to_owned()));
    let mut joined = out.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    Ok(joined)
}

fn is_yaml(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("yaml") || e.eq_ignore_ascii_case("yml"))
}

/// The rules to generate for: the one named, or every dependency rule.
fn targets<'c>(
    config: &'c Config,
    wanted: Option<&str>,
) -> Result<Vec<(Family, &'c Rule, String)>, Outcome> {
    let mut allowed = 0usize;
    let all: Vec<(Family, &Rule, String)> = config
        .rules
        .all_dependency_rules()
        .map(|(family, rule)| {
            let label = if family == Family::Allowed {
                allowed += 1;
                format!("allowed[{}]", allowed - 1)
            } else {
                rule.name().to_owned()
            };
            (family, rule, label)
        })
        .collect();
    let Some(wanted) = wanted else {
        return Ok(all);
    };
    let found: Vec<(Family, &Rule, String)> = all
        .iter()
        .filter(|(_, _, l)| l == wanted)
        .cloned()
        .collect();
    if found.is_empty() {
        let labels: Vec<&str> = all.iter().map(|(_, _, l)| l.as_str()).collect();
        return Err(Outcome::failed(
            RunExit::InvalidConfig,
            format!(
                "rulebearing test --generate: no rule `{wanted}`; the rules are {}\n",
                labels.join(", ")
            ),
        ));
    }
    Ok(found)
}

fn has_examples(rule: &Rule) -> bool {
    rule.meta
        .examples
        .as_ref()
        .is_some_and(|e| !e.allowed.is_empty() || !e.forbidden.is_empty())
}

/// Runs `test --generate`.
#[expect(
    clippy::too_many_lines,
    reason = "choose, sample, edit, reload and report: one pass over the rules, read top to bottom"
)]
pub fn run(ctx: &mut Context<'_>, args: &TestArgs) -> Outcome {
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let chosen = match targets(&config, args.rule.as_deref()) {
        Ok(t) => t,
        Err(o) => return o,
    };
    if let Some((_, rule, label)) = chosen.first()
        && args.rule.is_some()
        && has_examples(rule)
        && !args.force
    {
        return Outcome::failed(
            RunExit::InvalidConfig,
            format!(
                "rulebearing test --generate: `{label}` already has examples; pass --force to replace them\n"
            ),
        );
    }
    let document = match cache::document(ctx, &config, args.graph.as_deref(), args.no_cache) {
        Ok(d) => d,
        Err(m) => {
            return Outcome::failed(
                RunExit::Untrustworthy,
                format!("rulebearing test --generate: {m}\n"),
            );
        }
    };
    let mut report = String::new();
    let mut planned: Vec<(String, String, Generated)> = Vec::new();
    for (family, rule, label) in &chosen {
        if has_examples(rule) && !args.force {
            let _ = writeln!(report, "{label}: kept its examples (--force replaces them)");
            continue;
        }
        let generated = match examples(rule, *family, &config, &document, ctx.today) {
            Ok(g) => g,
            Err(e) => {
                return Outcome::failed(
                    RunExit::Untrustworthy,
                    format!("rulebearing test --generate: {e}\n"),
                );
            }
        };
        if generated.allowed.is_empty() && generated.forbidden.is_empty() {
            let _ = writeln!(
                report,
                "{label}: no edge of the graph makes an example that a single-edge graph proves; write them by hand"
            );
            continue;
        }
        let _ = writeln!(
            report,
            "{label}: {} forbidden, {} allowed",
            generated.forbidden.len(),
            generated.allowed.len()
        );
        if generated.forbidden.is_empty() {
            let _ = writeln!(
                report,
                "  no forbidden example: the rule flags nothing today, or only what one edge alone cannot show (a cycle, a path)"
            );
        }
        planned.push((label.clone(), rule.name().to_owned(), generated));
    }
    if planned.is_empty() {
        return Outcome::printed(report);
    }
    let Some(file) = config.origin.clone() else {
        return Outcome::failed(
            RunExit::InvalidConfig,
            "rulebearing test --generate: the configuration came from standard input, so there is no file to write the examples to\n",
        );
    };
    if !is_yaml(&file) {
        let mut blocks = report;
        for (label, _, generated) in &planned {
            let _ = writeln!(blocks, "\n# {label}\n{}", block(0, generated).join("\n"));
        }
        return Outcome {
            stdout: blocks,
            stderr: format!(
                "rulebearing test --generate: {} is not YAML, so it is not edited; paste the examples above into it\n",
                file.display()
            ),
            code: RunExit::InvalidConfig.code(),
        };
    }
    let original = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) => {
            return Outcome::failed(
                RunExit::Untrustworthy,
                format!(
                    "rulebearing test --generate: cannot read {}: {e}\n",
                    file.display()
                ),
            );
        }
    };
    let mut text = original.clone();
    for (label, name, generated) in &planned {
        let locator = label
            .strip_prefix("allowed[")
            .and_then(|r| r.strip_suffix(']'))
            .and_then(|n| n.parse().ok())
            .map_or(Locator::Name(name), Locator::Allowed);
        match set_examples(&text, locator, generated) {
            Ok(t) => text = t,
            Err(m) => {
                return Outcome::failed(
                    RunExit::InvalidConfig,
                    format!(
                        "rulebearing test --generate: {}: {m}; nothing was written\n",
                        file.display()
                    ),
                );
            }
        }
    }
    let restore = |why: String| {
        let _ = std::fs::write(&file, &original);
        Outcome::failed(
            RunExit::Untrustworthy,
            format!(
                "rulebearing test --generate: {why}; {} is unchanged\n",
                file.display()
            ),
        )
    };
    if let Err(e) = std::fs::write(&file, &text) {
        return restore(format!("cannot write {}: {e}", file.display()));
    }
    let reloaded = match configure::load(ctx, &args.config) {
        Ok(Some(c)) => c,
        Ok(None) => return restore("the configuration vanished".to_owned()),
        Err(e) => return restore(format!("the edited file does not load ({e})")),
    };
    for (label, _, generated) in &planned {
        let back = targets(&reloaded, Some(label))
            .ok()
            .and_then(|t| t.first().and_then(|(_, r, _)| r.meta.examples.clone()))
            .unwrap_or_default();
        if back.allowed != generated.allowed || back.forbidden != generated.forbidden {
            return restore(format!(
                "`{label}` reads different examples back after the edit"
            ));
        }
    }
    let base = ctx.cwd.canonicalize().unwrap_or_else(|_| ctx.cwd.clone());
    let shown = file.canonicalize().unwrap_or_else(|_| file.clone());
    let _ = writeln!(
        report,
        "wrote {}; run `rulebearing test` to prove them",
        crate::cmd::decisions::shown(&base, &shown)
    );
    Outcome::printed(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAML: &str = "# keep me
forbidden:
  - name: a
    severity: error # inline comment
    from: { path: \"^src/a/\" }
    to: { path: \"^src/b/\" }

  - name: \"b\"
    from: { path: \"^x\" }
    examples:
      forbidden: [\"old -> one\"]
      allowed:
        - \"old -> two\"
    to: { path: \"^y\" }
rules:
  ratchets: []
";

    fn generated() -> Generated {
        Generated {
            allowed: vec!["p -> q".into()],
            forbidden: vec!["r -> s".into(), "t -> u".into()],
        }
    }

    #[test]
    fn examples_are_added_at_the_end_of_the_rule() -> Result<(), String> {
        let out = set_examples(YAML, Locator::Name("a"), &generated())?;
        assert!(out.contains(
            "    to: { path: \"^src/b/\" }\n    examples:\n      forbidden: [\"r -> s\", \"t -> u\"]\n      allowed: [\"p -> q\"]\n\n  - name: \"b\""
        ), "{out}");
        assert!(out.starts_with("# keep me\n") && out.contains("# inline comment"));
        Ok(())
    }

    #[test]
    fn existing_examples_are_replaced_in_place() -> Result<(), String> {
        let out = set_examples(YAML, Locator::Name("b"), &generated())?;
        assert!(!out.contains("old -> "), "{out}");
        assert!(out.contains(
            "    from: { path: \"^x\" }\n    examples:\n      forbidden: [\"r -> s\", \"t -> u\"]\n      allowed: [\"p -> q\"]\n    to: { path: \"^y\" }\n"
        ), "{out}");
        let value: serde_yaml::Value = serde_yaml::from_str(&out).map_err(|e| e.to_string())?;
        assert_eq!(value["forbidden"][1]["examples"]["allowed"][0], "p -> q");
        Ok(())
    }

    #[test]
    fn a_rule_not_in_block_form_is_refused() {
        assert!(
            set_examples(
                "forbidden:\n  - { name: a }\n",
                Locator::Name("a"),
                &generated()
            )
            .is_err()
        );
        assert!(set_examples(YAML, Locator::Name("zzz"), &generated()).is_err());
        assert!(
            set_examples(
                "forbidden:\n  - examples: {}\n    name: a\n",
                Locator::Name("a"),
                &generated()
            )
            .is_err()
        );
    }

    #[test]
    fn allowed_items_are_found_by_position() -> Result<(), String> {
        let text = "allowed:\n  - from: { path: \"^a\" }\n    to: {}\n  -\n    from: { path: \"^b\" }\n    to: {}\nforbidden: []\n";
        let out = set_examples(text, Locator::Allowed(1), &generated())?;
        let value: serde_yaml::Value = serde_yaml::from_str(&out).map_err(|e| e.to_string())?;
        assert_eq!(value["allowed"][1]["examples"]["forbidden"][0], "r -> s");
        assert!(value["allowed"][0].get("examples").is_none());
        assert!(set_examples(text, Locator::Allowed(2), &generated()).is_err());
        assert!(set_examples("forbidden: []\n", Locator::Allowed(0), &generated()).is_err());
        assert!(
            set_examples(
                "allowed:\n  - { from: {} }\n",
                Locator::Allowed(0),
                &generated()
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn name_lines_and_columns() {
        assert!(names("  - name: a", "a"));
        assert!(names("    name: 'a' # c", "a"));
        assert!(names("-   name: \"a\"", "a"));
        assert!(!names("  - name: ab", "a"));
        assert!(!names("  fix: name: a", "a"));
        assert_eq!(key_column("  - name: a"), 4);
        assert_eq!(key_column("  -   name: a"), 6);
        assert_eq!(key_column("    from: x"), 4);
        assert_eq!(block(2, &Generated::default()), ["  examples:"]);
        assert!(is_yaml(std::path::Path::new("r.YML")));
        assert!(!is_yaml(std::path::Path::new("r.json")));
    }
}
