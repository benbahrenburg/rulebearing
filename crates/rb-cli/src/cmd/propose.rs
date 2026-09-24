//! `rulebearing propose`: a first draft of a rule, with what it would match today.
//!
//! - Source: [design § Rules an agent writes, held to the same bar](../../../../docs/artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar)
//!   ("the starting point an agent should be given instead of a blank regex")
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//!   (the three forms; read from the worktree-aware cache of Step 13)
//! - Decisions: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md) decision 2,
//!   [ADR-0016](../../../../docs/adr/0016-linear-time-regex-and-strict-compat.md) (patterns the
//!   engine compiles)
//! - Requirement: [FR-CLI-02](../../../../docs/prd.md#fr-cli-02)
//!
//! | Form | Drafts |
//! | --- | --- |
//! | `--from GLOB --to GLOB` | a `forbidden` rule, with the modules each side matches and the edges it would flag |
//! | `--select KIND [--where PREDICATE] [--should CONDITION]` | an element rule, with the selection's size and a sample, and what fails `should` when it is given |
//! | `--from-example "a -> b"` | the narrowest `forbidden` rule covering that edge: each side's directory as a prefix, the `from` side widened one segment at a time until it matches more modules than the example's, never up to the directory both files share |
//!
//! A glob becomes an anchored pattern (`**/` any folders, `*` one segment, `?` one character,
//! `{a,b}` either); a glob without wildcards is a directory prefix when the graph has files below
//! it, else one file; a value starting with `^` is taken as a pattern already. Predicates and
//! conditions are YAML, parsed by the same code as the configuration, so a draft that prints is
//! a rule that loads. The output is YAML to paste under the rules, with the counts as comments.

use std::fmt::Write as _;

use clap::{ArgGroup, Args};
use rb_config::elements::{ElementRule, parse_elements};
use rb_config::{Config, Rule};
use rb_model::GraphDocument;
use rb_rules::elements::{Architecture, Evaluator};
use rb_rules::{EvalOptions, Evaluation, evaluate};
use serde_json::{Value, json};

use crate::cli::ConfigArgs;
use crate::cmd::decisions::slug;
use crate::cmd::test_rules::parse_example;
use crate::context::Context;
use crate::{Outcome, RunExit, cache, configure};

/// How many edges or objects a draft lists.
pub const SAMPLE: usize = 10;

/// `propose`.
#[derive(Debug, Clone, Default, Args)]
#[command(group(ArgGroup::new("form").required(true).args(["from", "select", "from_example"])))]
pub struct ProposeArgs {
    /// Draft a forbidden rule from files matching GLOB (or a pattern starting with ^)
    #[arg(long, value_name = "GLOB", requires = "to")]
    pub from: Option<String>,
    /// ... to files matching GLOB (or a pattern starting with ^)
    #[arg(long, value_name = "GLOB", requires = "from")]
    pub to: Option<String>,
    /// Draft an element rule over objects of KIND: type, class, interface, attribute, member,
    /// field, method, property, function or module
    #[arg(long, value_name = "KIND")]
    pub select: Option<String>,
    /// The element rule's filter, in YAML, such as '{ haveNameEndingWith: Service }'
    #[arg(long = "where", value_name = "PREDICATE", requires = "select")]
    pub where_: Option<String>,
    /// What every selected object must satisfy, in YAML, such as '{ beSealed: true }'
    #[arg(long, value_name = "CONDITION", requires = "select")]
    pub should: Option<String>,
    /// Generalise one forbidden edge, "from -> to", to the narrowest rule that covers it
    #[arg(long, value_name = "EDGE")]
    pub from_example: Option<String>,
    /// The drafted rule's name (default: made from its sides)
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
    /// A graph document to answer from instead of the cache
    #[arg(long, value_name = "FILE")]
    pub graph: Option<String>,
    /// Extract afresh, neither reading nor writing the cache
    #[arg(long)]
    pub no_cache: bool,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
}

/// Escapes what a JavaScript regular expression treats specially, leaving `-` and `/` readable.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A glob as an anchored pattern over module sources.
pub fn glob_to_pattern(glob: &str, sources: &[&str]) -> String {
    if glob.starts_with('^') {
        return glob.to_owned();
    }
    let glob = glob.trim_start_matches("./");
    if !glob.contains(['*', '?', '{']) {
        let literal = glob.trim_end_matches('/');
        let below = format!("{literal}/");
        return if glob.ends_with('/') || sources.iter().any(|s| s.starts_with(&below)) {
            format!("^{}/", escape(literal))
        } else {
            format!("^{}$", escape(literal))
        };
    }
    let mut out = String::from("^");
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                if chars.get(i + 2) == Some(&'/') {
                    out.push_str("(?:.*/)?");
                    i += 3;
                } else {
                    out.push_str(".*");
                    i += 2;
                }
                continue;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '{' => out.push_str("(?:"),
            '}' => out.push(')'),
            ',' if out.matches("(?:").count() > out.matches(')').count() => out.push('|'),
            c => out.push_str(&escape(&c.to_string())),
        }
        i += 1;
    }
    match out.strip_suffix("/.*") {
        Some(prefix) => format!("{prefix}/"),
        None => format!("{out}$"),
    }
}

/// A pattern's readable part, for a rule name; `any` when it has none.
fn words(pattern: &str) -> String {
    let text = slug(&pattern.replace("[^/]", "").replace(".*", ""));
    if text.is_empty() {
        "any".to_owned()
    } else {
        text
    }
}

/// Evaluates `rule` alone over `document`, liveness off.
///
/// # Errors
/// The engine's message.
pub fn evaluate_alone(
    document: GraphDocument,
    rule: Rule,
    today: chrono::NaiveDate,
) -> Result<Evaluation, String> {
    let mut single = Config::default();
    single.rules.dependencies.forbidden.push(rule);
    let options = EvalOptions {
        liveness: false,
        today,
        ..EvalOptions::default()
    };
    evaluate(document, &single, &options).map_err(|e| e.to_string())
}

fn yaml_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// The YAML and the counts for a drafted `forbidden` rule.
fn forbidden_draft(
    ctx: &Context<'_>,
    document: GraphDocument,
    name: &str,
    from: &str,
    to: &str,
    note: Option<String>,
) -> Result<String, Outcome> {
    let rule: Rule = serde_json::from_value(json!({
        "name": name, "severity": "error", "from": { "path": from }, "to": { "path": to },
    }))
    .map_err(|e| {
        Outcome::failed(
            RunExit::InvalidConfig,
            format!("rulebearing propose: {e}\n"),
        )
    })?;
    let evaluation = evaluate_alone(document, rule, ctx.today).map_err(|e| {
        Outcome::failed(
            RunExit::InvalidConfig,
            format!("rulebearing propose: {e}\n"),
        )
    })?;
    let stats = evaluation.rule_stats.first();
    let (from_matches, to_matches) = stats.map_or((0, 0), |s| (s.from_matches, s.to_matches));
    let mut edges: Vec<(String, String)> = evaluation
        .violations()
        .iter()
        .map(|v| (v.from.clone(), v.to.clone()))
        .collect();
    edges.sort();
    edges.dedup();
    let mut out = format!(
        "# rulebearing propose: `from` matches {}, `to` matches {}; {} would be flagged today\n",
        plural(from_matches, "module"),
        plural(to_matches, "dependency"),
        plural(edges.len(), "edge")
    );
    if let Some(note) = note {
        let _ = writeln!(out, "# {note}");
    }
    let _ = writeln!(
        out,
        "forbidden:\n  - name: {name}\n    severity: error\n    from: {{ path: {} }}\n    to: {{ path: {} }}",
        yaml_string(from),
        yaml_string(to)
    );
    if !edges.is_empty() {
        let _ = writeln!(out, "# flagged today:");
        for (a, b) in edges.iter().take(SAMPLE) {
            let _ = writeln!(out, "#   {a} -> {b}");
        }
        if edges.len() > SAMPLE {
            let _ = writeln!(out, "#   and {} more", edges.len() - SAMPLE);
        }
    }
    let _ = writeln!(
        out,
        "# next: add a comment citing its decision (adr:NNNN) and a fix, then `rulebearing test --generate {name}`"
    );
    Ok(out)
}

/// The directory of a path, without the file.
fn directory(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(d, _)| d)
}

/// The `--from-example` sides: `from` widened until it matches more than the example's module,
/// never to the folder both files share; `to` the target's folder unless that holds the source.
pub fn generalise(from: &str, to: &str, sources: &[&str]) -> (String, String) {
    let from_parts: Vec<&str> = from.split('/').collect();
    let to_parts: Vec<&str> = to.split('/').collect();
    let shared = from_parts
        .iter()
        .zip(&to_parts)
        .take(
            from_parts
                .len()
                .saturating_sub(1)
                .min(to_parts.len().saturating_sub(1)),
        )
        .take_while(|(a, b)| a == b)
        .count();
    // Folders below the shared one on the source's side, narrowest first.
    let candidates: Vec<String> = (shared + 1..from_parts.len())
        .rev()
        .map(|n| from_parts[..n].join("/"))
        .collect();
    let matching = |folder: &str| {
        let below = format!("{folder}/");
        sources.iter().filter(|s| s.starts_with(&below)).count()
    };
    let from_pattern = candidates
        .iter()
        .find(|c| matching(c) > 1)
        .or_else(|| candidates.last())
        .map_or_else(
            || format!("^{}$", escape(from)),
            |c| format!("^{}/", escape(c)),
        );
    let to_pattern = match directory(to) {
        Some(folder) if !from.starts_with(&format!("{folder}/")) => {
            format!("^{}/", escape(folder))
        }
        _ => format!("^{}$", escape(to)),
    };
    (from_pattern, to_pattern)
}

fn normalise(path: &str) -> String {
    let mut text = path.replace('\\', "/");
    while let Some(rest) = text.strip_prefix("./") {
        text = rest.to_owned();
    }
    text
}

fn yaml_value(text: &str, flag: &str) -> Result<Value, Outcome> {
    serde_yaml::from_str::<Value>(text).map_err(|e| {
        Outcome::failed(
            RunExit::InvalidConfig,
            format!("rulebearing propose: {flag} is not YAML: {e}\n"),
        )
    })
}

fn element_draft(
    document: &GraphDocument,
    args: &ProposeArgs,
    kind: &str,
) -> Result<String, Outcome> {
    let invalid = |m: String| {
        Outcome::failed(
            RunExit::InvalidConfig,
            format!("rulebearing propose: {m}\n"),
        )
    };
    let filter = args
        .where_
        .as_deref()
        .map(|w| yaml_value(w, "--where"))
        .transpose()?;
    let condition = args
        .should
        .as_deref()
        .map(|s| yaml_value(s, "--should"))
        .transpose()?;
    let name = args.name.clone().unwrap_or_else(|| {
        let described = filter.as_ref().map(Value::to_string).unwrap_or_default();
        let mut text = slug(&format!("{kind} {described}"));
        text.truncate(60);
        text.trim_end_matches('-').to_owned()
    });
    let mut select = json!({ "kind": kind });
    if let Some(filter) = &filter {
        select["where"] = filter.clone();
    }
    // Without `--should` only the selection is read; `exist` stands in for the condition, since
    // the loader refuses an empty `should` and every language answers `exist`.
    let value = json!([{
        "name": name,
        "select": select,
        "should": condition.clone().unwrap_or_else(|| json!({ "exist": true })),
    }]);
    let rule: ElementRule = parse_elements(&value)
        .map_err(|e| invalid(e.to_string()))?
        .into_iter()
        .next()
        .ok_or_else(|| invalid("nothing to draft".to_owned()))?;
    let architecture = Architecture::new(document);
    rb_rules::elements::capability::validate(&architecture, &rule)
        .map_err(|e| invalid(e.to_string()))?;
    let evaluator = Evaluator::new(&architecture, &rule.name);
    let mut selected: Vec<String> = evaluator
        .select(&rule.select)
        .map_err(|e| invalid(e.to_string()))?
        .iter()
        .map(|o| o.key().to_owned())
        .collect();
    selected.sort();
    selected.dedup();
    let sample: Vec<&str> = selected.iter().take(SAMPLE).map(String::as_str).collect();
    let mut out = format!(
        "# rulebearing propose: {} selected today",
        plural(selected.len(), &format!("{kind} object"))
    );
    if !sample.is_empty() {
        let _ = write!(out, "; sample: {}", sample.join(", "));
    }
    out.push('\n');
    let _ = writeln!(
        out,
        "elements:\n  - name: {name}\n    severity: error\n    select: {select}"
    );
    match &condition {
        Some(condition) => {
            let _ = writeln!(out, "    should: {condition}");
            let outcome = rb_rules::elements::evaluate(&architecture, &rule)
                .map_err(|e| invalid(e.to_string()))?;
            let failing: Vec<&str> = outcome.failures().map(|r| r.object.as_str()).collect();
            let _ = write!(
                out,
                "# {} of {} fail `should` today",
                failing.len(),
                selected.len()
            );
            if failing.is_empty() {
                out.push('\n');
            } else {
                let shown: Vec<&str> = failing.iter().take(SAMPLE).copied().collect();
                let _ = writeln!(out, ": {}", shown.join(", "));
            }
        }
        None => {
            let _ = writeln!(
                out,
                "# add `should`: what every selected object must satisfy, such as {{ beSealed: true }}; `--should` shows what would fail today"
            );
        }
    }
    let _ = writeln!(
        out,
        "# next: add a comment citing its decision (adr:NNNN) and a fix"
    );
    Ok(out)
}

/// Runs `propose`.
pub fn run(ctx: &mut Context<'_>, args: &ProposeArgs) -> Outcome {
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let document = match cache::document(ctx, &config, args.graph.as_deref(), args.no_cache) {
        Ok(d) => d,
        Err(m) => {
            return Outcome::failed(
                RunExit::Untrustworthy,
                format!("rulebearing propose: {m}\n"),
            );
        }
    };
    let result = if let Some(kind) = &args.select {
        element_draft(&document, args, kind)
    } else {
        let sources: Vec<String> = document.modules.iter().map(|m| m.source.clone()).collect();
        let sources: Vec<&str> = sources.iter().map(String::as_str).collect();
        let (from, to, note) = if let Some(example) = &args.from_example {
            let Some((a, b)) = parse_example(example) else {
                return Outcome::failed(
                    RunExit::InvalidConfig,
                    format!(
                        "rulebearing propose: --from-example `{example}` is not \"from -> to\"\n"
                    ),
                );
            };
            let (a, b) = (normalise(&a), normalise(&b));
            let (from, to) = generalise(&a, &b, &sources);
            let present = document
                .modules
                .iter()
                .any(|m| m.source == a && m.dependencies.iter().any(|d| d.resolved == b));
            let note = format!(
                "generalised from {a} -> {b}, which {} in the graph today",
                if present { "is" } else { "is not" }
            );
            (from, to, Some(note))
        } else {
            let from = glob_to_pattern(args.from.as_deref().unwrap_or_default(), &sources);
            let to = glob_to_pattern(args.to.as_deref().unwrap_or_default(), &sources);
            (from, to, None)
        };
        let name = args
            .name
            .clone()
            .unwrap_or_else(|| format!("no-{}-to-{}", words(&from), words(&to)));
        forbidden_draft(ctx, document, &name, &from, &to, note)
    };
    match result {
        Ok(text) => Outcome::printed(text),
        Err(outcome) => outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_become_anchored_patterns() {
        let sources = ["src/domain/a.ts", "src/web/b.ts", "src/main.ts"];
        for (glob, pattern) in [
            ("src/domain/**", "^src/domain/"),
            ("./src/domain/", "^src/domain/"),
            ("src/domain", "^src/domain/"),
            ("src/main.ts", "^src/main\\.ts$"),
            ("src/*.ts", "^src/[^/]*\\.ts$"),
            ("src/**/*.spec.ts", "^src/(?:.*/)?[^/]*\\.spec\\.ts$"),
            ("src/{domain,web}/**", "^src/(?:domain|web)/"),
            ("src/?.ts", "^src/[^/]\\.ts$"),
            ("a,b/*", "^a,b/[^/]*$"),
            ("^already$", "^already$"),
        ] {
            assert_eq!(glob_to_pattern(glob, &sources), pattern, "{glob}");
        }
        for (glob, source, expected) in [
            ("src/**/*.spec.ts", "src/x.spec.ts", true),
            ("src/**/*.spec.ts", "src/a/b/x.spec.ts", true),
            ("src/*.ts", "src/a/x.ts", false),
        ] {
            let pattern = glob_to_pattern(glob, &sources);
            assert_eq!(
                rb_rules::patterns::test(&pattern, source),
                expected,
                "{glob} {source}"
            );
        }
        assert_eq!(escape("a.b-c/(d)"), "a\\.b-c/\\(d\\)");
    }

    #[test]
    fn an_example_widens_on_the_from_side_only_until_it_matches_more() {
        let sources = [
            "src/features/cart/ui/button.ts",
            "src/features/cart/model.ts",
            "src/features/billing/api.ts",
            "src/lib/log.ts",
        ];
        // `ui/` holds only the example, so the rule widens to `cart/`, which holds two files.
        assert_eq!(
            generalise(
                "src/features/cart/ui/button.ts",
                "src/features/billing/api.ts",
                &sources
            ),
            (
                "^src/features/cart/".to_owned(),
                "^src/features/billing/".to_owned()
            )
        );
        // Never widened to the folder both share: `features/` would include the target.
        assert_eq!(
            generalise(
                "src/features/billing/api.ts",
                "src/features/cart/model.ts",
                &sources
            ),
            (
                "^src/features/billing/".to_owned(),
                "^src/features/cart/".to_owned()
            )
        );
        // A file in the shared folder, or a target whose folder holds the source, stays exact.
        assert_eq!(
            generalise("src/a.ts", "src/lib/log.ts", &sources),
            ("^src/a\\.ts$".to_owned(), "^src/lib/".to_owned())
        );
        assert_eq!(
            generalise("src/lib/x/y.ts", "src/lib/log.ts", &sources).1,
            "^src/lib/log\\.ts$"
        );
        assert_eq!(
            generalise("a.ts", "b.ts", &sources),
            ("^a\\.ts$".into(), "^b\\.ts$".into())
        );
    }

    #[test]
    fn names_and_plurals() {
        assert_eq!(words("^src/(?:.*/)?[^/]*\\.spec\\.ts$"), "src-spec-ts");
        assert_eq!(plural(1, "edge"), "1 edge");
        assert_eq!(plural(2, "edge"), "2 edges");
        assert_eq!(normalise("./a\\b"), "a/b");
        assert_eq!(directory("a/b/c.ts"), Some("a/b"));
        assert_eq!(directory("c.ts"), None);
    }
}
