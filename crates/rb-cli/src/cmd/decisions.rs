//! `rulebearing decisions [--json]` and `decisions new --title T --rules r1,r2`: the links from
//! rules to decision records, checked, and a new record scaffolded.
//!
//! - Source: [design § The agentic engineering hat](../../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two)
//!   ("lists rule to decision-record links, fails a dangling one, and `decisions new` scaffolds a
//!   record with the enforcement ids filled in")
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//! - Decisions: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md) decision 2,
//!   [ADR-0001](../../../../docs/adr/0001-record-architecture-decisions.md) (the record template),
//!   [ADR-0008](../../../../docs/adr/0008-exit-code-contract.md)
//! - Requirement: [FR-CLI-02](../../../../docs/prd.md#fr-cli-02)
//!
//! Tokens are read from every rule's `comment` (`adr:NNNN`, `plan:<slug>`). An `adr:NNNN` resolves
//! to the file under the ADR directory (`--adr-dir`, default `docs/adr`) whose name starts with
//! that number; one that resolves to nothing is dangling and the command exits 1. A `plan:<slug>`
//! resolves to a Markdown file under `docs/plans/` whose name, with or without its leading
//! number, starts with the slug, and is listed
//! either way, since a plan may be named before it is written. `decisions new` takes the next
//! free number and writes the ADR-0001 template with the rules that enforce it named.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde_json::{Value, json};

use crate::cli::ConfigArgs;
use crate::cmd::catalogue;
use crate::context::Context;
use crate::{Outcome, RunExit, configure};

/// Where decision records live unless `--adr-dir` says otherwise.
pub const ADR_DIR: &str = "docs/adr";

/// `decisions`.
#[derive(Debug, Clone, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct DecisionsArgs {
    /// Scaffold a new record
    #[command(subcommand)]
    pub command: Option<DecisionsCommand>,
    /// Print JSON
    #[arg(long)]
    pub json: bool,
    /// The folder holding the decision records
    #[arg(long, value_name = "DIR", default_value = ADR_DIR)]
    pub adr_dir: String,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
}

/// `decisions` subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum DecisionsCommand {
    /// Write docs/adr/NNNN-<slug>.md from the ADR template, naming the rules that enforce it
    New(NewArgs),
}

/// `decisions new`.
#[derive(Debug, Clone, Args)]
pub struct NewArgs {
    /// The decision's title
    #[arg(long)]
    pub title: String,
    /// The rules that enforce it, comma separated
    #[arg(long, value_name = "RULES", value_delimiter = ',', required = true)]
    pub rules: Vec<String>,
    /// The folder holding the decision records
    #[arg(long, value_name = "DIR", default_value = ADR_DIR)]
    pub adr_dir: String,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
}

/// The number in a file name such as `0010-crate-layout.md`.
fn leading_number(name: &str) -> Option<u32> {
    let digits: String = name.chars().take_while(char::is_ascii_digit).collect();
    let markdown = Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("md"));
    if digits.is_empty() || !markdown {
        return None;
    }
    digits.parse().ok()
}

/// The record files in `dir` with their numbers, sorted by name.
fn records(dir: &Path) -> Vec<(u32, PathBuf)> {
    let mut found: Vec<(u32, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            leading_number(&name).map(|n| (n, e.path()))
        })
        .collect();
    found.sort_by(|a, b| a.1.cmp(&b.1));
    found
}

/// Markdown files under `dir`, recursively, sorted.
fn markdown_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            markdown_under(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

/// The file a decision token names, when it exists: `adr:NNNN` under `adr_dir`, `plan:<slug>`
/// under `docs/plans/`, both relative to `base`.
pub fn resolve(base: &Path, adr_dir: &str, token: &str) -> Option<PathBuf> {
    if let Some(number) = token.strip_prefix("adr:") {
        let number: u32 = number.parse().ok()?;
        return records(&base.join(adr_dir))
            .into_iter()
            .find(|(n, _)| *n == number)
            .map(|(_, p)| p);
    }
    let slug = token.strip_prefix("plan:")?;
    let mut plans = Vec::new();
    markdown_under(&base.join("docs/plans"), &mut plans);
    plans.into_iter().find(|p| {
        p.file_stem().is_some_and(|s| {
            let stem = s.to_string_lossy();
            let unnumbered = stem.trim_start_matches(|c: char| c.is_ascii_digit());
            stem.starts_with(slug) || unnumbered.trim_start_matches('-').starts_with(slug)
        })
    })
}

/// `path` relative to `base`, `/`-separated.
pub fn shown(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// One rule-to-decision link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The rule.
    pub rule: String,
    /// Its family.
    pub family: &'static str,
    /// The token, or `None` for a rule that cites no decision.
    pub token: Option<String>,
    /// The record, relative to the working directory, when it exists.
    pub file: Option<String>,
}

impl Link {
    /// An `adr:` token whose record is absent.
    pub fn dangling(&self) -> bool {
        self.file.is_none() && self.token.as_deref().is_some_and(|t| t.starts_with("adr:"))
    }
}

/// Every rule's links, in rule order; a rule with no token is one link with `token: None`.
pub fn links(base: &Path, adr_dir: &str, config: &rb_config::Config) -> Vec<Link> {
    let mut out = Vec::new();
    for entry in catalogue::entries(config) {
        let tokens = entry.tokens();
        if tokens.is_empty() {
            out.push(Link {
                rule: entry.name.clone(),
                family: entry.family,
                token: None,
                file: None,
            });
        }
        for token in tokens {
            out.push(Link {
                rule: entry.name.clone(),
                family: entry.family,
                file: resolve(base, adr_dir, &token).map(|p| shown(base, &p)),
                token: Some(token),
            });
        }
    }
    out
}

/// Runs `decisions`.
pub fn run(ctx: &mut Context<'_>, args: &DecisionsArgs) -> Outcome {
    if let Some(DecisionsCommand::New(new)) = &args.command {
        return scaffold(ctx, new);
    }
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let found = links(&ctx.cwd, &args.adr_dir, &config);
    let dangling: Vec<&Link> = found.iter().filter(|l| l.dangling()).collect();
    let stdout = if args.json {
        let rows: Vec<Value> = found
            .iter()
            .map(|l| {
                json!({
                    "rule": l.rule, "family": l.family, "token": l.token,
                    "file": l.file, "dangling": l.dangling(),
                })
            })
            .collect();
        let mut text = serde_json::to_string_pretty(&json!({
            "adrDir": args.adr_dir,
            "decisions": rows,
            "dangling": dangling.len(),
        }))
        .unwrap_or_default();
        text.push('\n');
        text
    } else {
        table(&found, &args.adr_dir)
    };
    if dangling.is_empty() {
        return Outcome::printed(stdout);
    }
    let mut stderr = String::new();
    for link in &dangling {
        let _ = writeln!(
            stderr,
            "rulebearing decisions: rule `{}` cites {} but {} holds no record with that number; write it (`rulebearing decisions new`) or correct the token",
            link.rule,
            link.token.as_deref().unwrap_or_default(),
            args.adr_dir
        );
    }
    Outcome {
        stdout,
        stderr,
        code: RunExit::Violations(1).code(),
    }
}

fn table(found: &[Link], adr_dir: &str) -> String {
    let rows: Vec<[String; 4]> = found
        .iter()
        .map(|l| {
            let record = match (&l.file, &l.token) {
                (Some(file), _) => file.clone(),
                (None, Some(t)) if t.starts_with("adr:") => format!("MISSING under {adr_dir}"),
                (None, Some(_)) => "not written".to_owned(),
                (None, None) => "-".to_owned(),
            };
            [
                l.rule.clone(),
                l.family.to_owned(),
                l.token.clone().unwrap_or_else(|| "-".to_owned()),
                record,
            ]
        })
        .collect();
    let header = [
        "rule".to_owned(),
        "family".to_owned(),
        "decision".to_owned(),
        "record".to_owned(),
    ];
    let widths: Vec<usize> = (0..3)
        .map(|i| {
            rows.iter()
                .chain(std::iter::once(&header))
                .map(|r| r[i].len())
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut out = String::new();
    for row in std::iter::once(&header).chain(rows.iter()) {
        let _ = writeln!(
            out,
            "{:<w0$}  {:<w1$}  {:<w2$}  {}",
            row[0],
            row[1],
            row[2],
            row[3],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2]
        );
    }
    out
}

/// A title as a file-name slug: lowercase ASCII letters and digits, runs of anything else one `-`.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_owned()
}

/// The ADR-0001 template for record `number`.
pub fn template(number: u32, title: &str, date: &str, rules: &[String], config: &str) -> String {
    let enforced: Vec<String> = rules.iter().map(|r| format!("`{r}`")).collect();
    format!(
        "# ADR-{number:04}: {title}

- **Status:** Proposed
- **Date:** {date}
- **Derives from:** the design section, requirement or incident this decision answers
- **Constrains:** the architecture section or code this decision shapes
- **Enforced by:** {} in `{config}`, each citing `adr:{number:04}` in its `comment`

## Context

What forces the decision: the problem, the constraints, what was measured.

## Decision

What is decided, as numbered, testable statements.

## Consequences

What becomes easier, what becomes harder, and what the rules above now fail.

## Alternatives considered

Each option not taken, and why.
",
        enforced.join(", ")
    )
}

/// Runs `decisions new`.
fn scaffold(ctx: &mut Context<'_>, args: &NewArgs) -> Outcome {
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let known: Vec<String> = catalogue::entries(&config)
        .into_iter()
        .map(|e| e.name)
        .collect();
    let unknown: Vec<&String> = args.rules.iter().filter(|r| !known.contains(r)).collect();
    if !unknown.is_empty() {
        return Outcome::failed(
            RunExit::InvalidConfig,
            format!(
                "rulebearing decisions new: no rule {}; the rules are {}\n",
                unknown
                    .iter()
                    .map(|r| format!("`{r}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
                known.join(", ")
            ),
        );
    }
    let name = slug(&args.title);
    if name.is_empty() {
        return Outcome::failed(
            RunExit::InvalidConfig,
            "rulebearing decisions new: the title has no letters or digits to name the file by\n",
        );
    }
    let dir = ctx.resolve(&args.adr_dir);
    let number = records(&dir).iter().map(|(n, _)| *n).max().unwrap_or(0) + 1;
    let path = dir.join(format!("{number:04}-{name}.md"));
    let config_name = config
        .origin
        .as_deref()
        .map_or_else(|| "the configuration".to_owned(), |p| shown(&ctx.cwd, p));
    let text = template(
        number,
        &args.title,
        &ctx.today.format("%Y-%m-%d").to_string(),
        &args.rules,
        &config_name,
    );
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, text.as_bytes()))
    }) {
        return Outcome::failed(
            RunExit::Untrustworthy,
            format!(
                "rulebearing decisions new: cannot write {}: {e}\n",
                path.display()
            ),
        );
    }
    let mut out = format!("wrote {}\n", shown(&ctx.cwd, &path));
    let _ = writeln!(
        out,
        "add adr:{number:04} to the comment of {}",
        args.rules.join(", ")
    );
    if dir.join("README.md").is_file() {
        let _ = writeln!(
            out,
            "add its row to {}",
            shown(&ctx.cwd, &dir.join("README.md"))
        );
    }
    Outcome::printed(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_and_numbers() {
        assert_eq!(
            slug("Stop Domain reaching Web!"),
            "stop-domain-reaching-web"
        );
        assert_eq!(slug("  --C# & .NET  "), "c-net");
        assert_eq!(slug("***"), "");
        assert_eq!(leading_number("0010-x.md"), Some(10));
        assert_eq!(leading_number("README.md"), None);
        assert_eq!(leading_number("0010-x.txt"), None);
    }

    #[test]
    fn the_template_follows_adr_0001() {
        let text = template(36, "T", "2026-09-24", &["a".into(), "b".into()], "r.yaml");
        assert!(text.starts_with("# ADR-0036: T\n"));
        for heading in [
            "- **Status:** Proposed",
            "- **Date:** 2026-09-24",
            "- **Derives from:**",
            "- **Constrains:**",
            "`a`, `b` in `r.yaml`, each citing `adr:0036`",
            "## Context",
            "## Decision",
            "## Consequences",
            "## Alternatives considered",
        ] {
            assert!(text.contains(heading), "{heading}");
        }
    }

    #[test]
    fn dangling_is_an_adr_without_a_record() {
        let link = |token: Option<&str>, file: Option<&str>| Link {
            rule: "r".into(),
            family: "forbidden",
            token: token.map(str::to_owned),
            file: file.map(str::to_owned),
        };
        assert!(link(Some("adr:0099"), None).dangling());
        assert!(!link(Some("adr:0001"), Some("docs/adr/0001-x.md")).dangling());
        assert!(!link(Some("plan:x"), None).dangling());
        assert!(!link(None, None).dangling());
    }
}
