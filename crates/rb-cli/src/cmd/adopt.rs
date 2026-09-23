//! `rulebearing adopt`: a brownfield repository to a green gate in one pull request.
//!
//! - Source: [design § The developer relations hat](../../../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo)
//!   ("writes the baseline, the CI step, the hook, and a `docs/architecture/rulebearing.md`, in
//!   one pull request that is green on day one")
//! - Plan: [Wave 1, Step 17](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-17-adopt-1f)
//! - Requirement: [FR-CLI-03](../../../../docs/prd.md#fr-cli-03)
//!
//! The repository's dependency-cruiser configuration is kept as it is: `adopt` writes a
//! `rulebearing.yaml` that `extends` it and adds `options.knownViolations`, one entry per current
//! finding keyed by its stable id, each with `expires` (90 days unless `--expires`) and `owner`
//! (the git user unless `--owner`). It cruises again with that file and stops unless the exit code
//! is 0, and only then writes the CI step, the pre-commit hook and the architecture page. Last, it
//! commits the files on a branch and opens a pull request with `gh`, or prints the branch when
//! `gh` or a remote is missing (`--no-pr` stops before the branch).

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use chrono::{Days, NaiveDate};
use clap::{Args, ValueEnum};
use rb_config::load::{self, LoadOptions};
use rb_config::read::Syntax;
use rb_config::{Config, ConfigFormat};
use rb_model::{Severity, Violation};
use serde_json::{Value, json};

use crate::cli::ConfigArgs;
use crate::cmd::{init, plain, rules};
use crate::configure::{self, Source};
use crate::context::Context;
use crate::pipeline::{self, RunOptions};
use crate::progress::Progress;
use crate::{Outcome, RunExit};

/// How baseline entries are written.
#[derive(Debug, Clone, Default, Args)]
pub struct BaselineArgs {
    /// When baseline entries expire: a date (YYYY-MM-DD) or a number of days
    #[arg(long, value_name = "DATE|DAYS")]
    pub expires: Option<String>,
    /// Who answers for baseline entries (default: the git user)
    #[arg(long)]
    pub owner: Option<String>,
}

/// The CI system the step is written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Ci {
    /// A GitHub Actions workflow using the Rulebearing Action.
    #[default]
    Github,
    /// An Azure Pipelines file with `--output-type azure-devops`.
    Azure,
}

/// `adopt`.
#[derive(Debug, Clone, Default, Args)]
pub struct AdoptArgs {
    /// Files and folders to cruise (default: apps, packages and libs, else src, else .)
    #[arg(value_name = "FILES-OR-DIRECTORIES")]
    pub paths: Vec<String>,
    /// Baseline entries
    #[command(flatten)]
    pub baseline: BaselineArgs,
    /// The CI system to write a step for
    #[arg(long, value_enum, default_value_t = Ci::Github)]
    pub ci: Ci,
    /// Write the files but do not create a branch or open a pull request
    #[arg(long)]
    pub no_pr: bool,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
}

/// The default life of a baseline entry.
pub const DEFAULT_DAYS: u64 = 90;
/// The branch `adopt` commits to.
pub const BRANCH: &str = "rulebearing/adopt";

fn git(ctx: &Context<'_>, args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .current_dir(&ctx.cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
}

/// The expiry date for `--expires`.
///
/// # Errors
/// A message when the value is neither a date nor a number of days.
pub fn expiry(today: NaiveDate, expires: Option<&str>) -> Result<NaiveDate, String> {
    match expires {
        None => today
            .checked_add_days(Days::new(DEFAULT_DAYS))
            .ok_or_else(|| "the date overflows".into()),
        Some(text) => text
            .parse::<NaiveDate>()
            .or_else(|_| {
                text.parse::<u64>()
                    .map_err(|_| ())
                    .and_then(|d| today.checked_add_days(Days::new(d)).ok_or(()))
            })
            .map_err(|()| {
                format!("--expires `{text}`: give a date (YYYY-MM-DD) or a number of days")
            }),
    }
}

/// One `knownViolations` entry per finding that is not already ignored.
///
/// # Errors
/// An [`Outcome`] (exit 3) when `--expires` is malformed.
pub fn baseline(
    ctx: &Context<'_>,
    violations: &[Violation],
    args: &BaselineArgs,
) -> Result<Vec<Value>, Outcome> {
    let expires = expiry(ctx.today, args.expires.as_deref())
        .map_err(|m| Outcome::failed(RunExit::InvalidConfig, format!("rulebearing: {m}\n")))?;
    let owner = args
        .owner
        .clone()
        .or_else(|| git(ctx, &["config", "user.name"]))
        .unwrap_or_else(|| "unowned".into());
    Ok(violations
        .iter()
        .filter(|v| v.rule.severity != Severity::Ignore)
        .map(|v| {
            // The id matches the edge; `type` with `cycle` or `via` matches the violation as
            // dependency-cruiser's own baseline does, so a cycle stays known whichever of its
            // edges reports it.
            let mut entry = json!({
                "id": v.id,
                "type": v.violation_type,
                "from": v.from,
                "to": v.to,
                "rule": { "name": v.rule.name, "severity": v.rule.severity },
            });
            if let Some(cycle) = &v.cycle {
                entry["cycle"] = json!(cycle);
            }
            if let Some(via) = &v.via {
                entry["via"] = json!(via);
            }
            entry["expires"] = json!(expires.to_string());
            entry["owner"] = json!(owner);
            entry
        })
        .collect())
}

/// The `rulebearing.yaml` that extends the repository's configuration and holds the baseline.
pub fn adopted_config(extends: &str, entries: &[Value], today: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# rulebearing.yaml, written by `rulebearing adopt` on {today}."
    );
    let _ = writeln!(
        out,
        "# The rules are {extends}'s, unchanged; this file adds the findings present today"
    );
    let _ = writeln!(
        out,
        "# as known violations, each with an owner and an expiry. Fix them before they expire."
    );
    let _ = writeln!(
        out,
        "extends: {}",
        serde_json::to_string(extends).unwrap_or_default()
    );
    if !entries.is_empty() {
        out.push_str("options:\n  knownViolations:\n");
        for entry in entries {
            let _ = writeln!(
                out,
                "    - {}",
                serde_json::to_string(entry).unwrap_or_default()
            );
        }
    }
    out
}

/// The CI step's file name and text.
pub fn ci_step(ci: Ci, paths: &[String]) -> (&'static str, String) {
    let paths = paths.join(" ");
    match ci {
        Ci::Github => (
            ".github/workflows/rulebearing.yml",
            format!(
                "# The architecture gate, written by `rulebearing adopt`.\nname: rulebearing\n\non:\n  pull_request:\n  push:\n    branches: [main]\n\npermissions:\n  contents: read\n\njobs:\n  rulebearing:\n    runs-on: ubuntu-latest\n    timeout-minutes: 10\n    steps:\n      - uses: actions/checkout@v4\n      - uses: benbahrenburg/rulebearing@v{}\n        with:\n          args: --config rulebearing.yaml {paths}\n",
                env!("CARGO_PKG_VERSION")
            ),
        ),
        Ci::Azure => (
            "azure-pipelines.rulebearing.yml",
            format!(
                "# The architecture gate, written by `rulebearing adopt`.\ntrigger:\n  branches:\n    include: [main]\npr:\n  branches:\n    include: ['*']\n\npool:\n  vmImage: ubuntu-latest\n\nsteps:\n  - checkout: self\n  - script: npx --yes rulebearing@{} cruise --config rulebearing.yaml --output-type azure-devops {paths}\n    displayName: rulebearing\n",
                env!("CARGO_PKG_VERSION")
            ),
        ),
    }
}

/// The pre-commit hook: Husky's folder when the repository uses Husky, else `.githooks/`. An
/// existing hook keeps every line it has and gains the gate as its last line; one that already
/// runs the gate is left as it is.
pub fn hook(root: &Path, paths: &[String]) -> (String, String) {
    let line = format!(
        "npx --no-install rulebearing cruise --config rulebearing.yaml --output-type err {}\n",
        paths.join(" ")
    );
    let path = if root.join(".husky").is_dir() {
        ".husky/pre-commit"
    } else {
        ".githooks/pre-commit"
    };
    match std::fs::read_to_string(root.join(path)) {
        Ok(existing) if existing.contains("rulebearing cruise") => (path.into(), existing),
        Ok(existing) if !existing.is_empty() => {
            let separator = if existing.ends_with('\n') { "" } else { "\n" };
            (path.into(), format!("{existing}{separator}{line}"))
        }
        _ if path == ".husky/pre-commit" => (path.into(), line),
        _ => (
            path.into(),
            format!(
                "#!/bin/sh\n# The architecture gate before each commit, written by `rulebearing adopt`.\n# Enable it once per clone: git config core.hooksPath .githooks\n{line}"
            ),
        ),
    }
}

/// `docs/architecture/rulebearing.md`: every rule in a sentence, with its reason, its fix and
/// its baseline.
pub fn architecture_page(config: &Config, entries: &[Value], extends: &str) -> String {
    let mut out = String::from("# Architecture rules\n\n");
    let _ = writeln!(
        out,
        "These rules are checked on every pull request by `rulebearing cruise --config rulebearing.yaml`. They come from `{extends}`; this page is generated by `rulebearing adopt` from `rulebearing explain --plain`.\n"
    );
    out.push_str(
        "| Rule | What it says | Why | What to do | Baselined |\n| --- | --- | --- | --- | --- |\n",
    );
    let cell = |t: &str| t.replace('|', "\\|").replace('\n', " ");
    for (family, rule) in rules::listed(config) {
        let count = entries
            .iter()
            .filter(|e| e["rule"]["name"] == rule.name())
            .count();
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {count} |",
            rule.name(),
            cell(&plain::sentence(family, rule)),
            cell(rule.meta.comment.as_deref().unwrap_or("")),
            cell(rule.meta.fix.as_deref().unwrap_or("")),
        );
    }
    if !entries.is_empty() {
        let expires = entries
            .first()
            .and_then(|e| e["expires"].as_str())
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "\n{} findings were present when the gate was adopted. They are listed under `options.knownViolations` in `rulebearing.yaml` and expire on {expires}; the gate fails for each one still present after that day.",
            entries.len()
        );
    }
    out
}

/// The pull request's body.
pub fn pr_body(extends: &str, entries: &[Value], files: &[String], version: &str) -> String {
    let mut by_rule: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for e in entries {
        *by_rule
            .entry(e["rule"]["name"].as_str().unwrap_or_default().to_owned())
            .or_default() += 1;
    }
    let mut out = format!(
        "This adds an architecture gate that runs the rules in `{extends}` with Rulebearing {version}. The rules are unchanged, and the gate is green today.\n\n"
    );
    if entries.is_empty() {
        out.push_str("The repository has no findings, so there is no baseline.\n\n");
    } else {
        let _ = writeln!(
            out,
            "{} current findings are baselined in `rulebearing.yaml` (`options.knownViolations`). Each entry names an owner and expires; the gate fails for any still present after that.\n",
            entries.len()
        );
        out.push_str("| Rule | Baselined |\n| --- | --- |\n");
        for (rule, count) in &by_rule {
            let _ = writeln!(out, "| `{rule}` | {count} |");
        }
        out.push('\n');
    }
    out.push_str("Files:\n\n");
    for f in files {
        let _ = writeln!(out, "- `{f}`");
    }
    out.push_str(
        "\nTo see why a rule exists and what to do when it fires: `rulebearing explain <rule>`.\n",
    );
    out
}

fn write(ctx: &Context<'_>, file: &str, text: &str) -> Result<(), Outcome> {
    let mut ignored = String::new();
    crate::write_output(ctx, file, text, &mut ignored)
        .map_err(|m| Outcome::failed(RunExit::Untrustworthy, format!("rulebearing adopt: {m}\n")))
}

/// Marks the hook executable; Git runs it only then. Windows has no mode bit to set.
fn executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn cruise(ctx: &Context<'_>, config: &Config, paths: &[String]) -> Result<pipeline::Run, Outcome> {
    let options = RunOptions {
        liveness: true,
        options_used: serde_json::Map::new(),
        paths: paths.to_vec(),
    };
    pipeline::run(ctx, config, &options, &mut Progress::new(None))
        .map_err(|e| Outcome::failed(RunExit::Untrustworthy, format!("rulebearing adopt: {e}\n")))
}

/// Commits `files` on [`BRANCH`] and opens the pull request, or says what to do instead.
fn open_pull_request(ctx: &Context<'_>, files: &[String], body: &str) -> String {
    if git(ctx, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Some("true") {
        return "not a git repository: commit the files above yourself\n".into();
    }
    // `-b`, not `-B`: an adopt branch from an earlier run is never reset.
    if git(ctx, &["checkout", "-b", BRANCH]).is_none() {
        return format!(
            "could not create the branch {BRANCH} (does it exist already?); commit the files above yourself\n"
        );
    }
    let mut add = vec!["add", "--"];
    add.extend(files.iter().map(String::as_str));
    // Only the files adopt wrote: anything the user had staged stays staged, out of this commit.
    let mut commit = vec![
        "commit",
        "-m",
        "Adopt the rulebearing architecture gate",
        "--",
    ];
    commit.extend(files.iter().map(String::as_str));
    if git(ctx, &add).is_none() || git(ctx, &commit).is_none() {
        return format!("could not commit on {BRANCH}; commit the files above yourself\n");
    }
    let has_remote = git(ctx, &["remote"]).is_some_and(|r| !r.is_empty());
    let has_gh = Command::new("gh")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !has_remote || !has_gh {
        return format!(
            "committed on {BRANCH}; push it and open a pull request (no {} found)\n",
            if has_remote { "gh" } else { "remote" }
        );
    }
    if git(ctx, &["push", "-u", "origin", BRANCH]).is_none() {
        return format!(
            "committed on {BRANCH}, but the push failed; push it and open a pull request\n"
        );
    }
    match Command::new("gh")
        .args([
            "pr",
            "create",
            "--head",
            BRANCH,
            "--title",
            "Adopt the rulebearing architecture gate",
            "--body",
            body,
        ])
        .current_dir(&ctx.cwd)
        .output()
    {
        Ok(o) if o.status.success() => format!("opened {}", String::from_utf8_lossy(&o.stdout)),
        _ => format!("pushed {BRANCH}; open the pull request by hand (gh pr create failed)\n"),
    }
}

/// The dependency-cruiser configuration `adopt` wraps, as `extends` names it.
fn wrapped(ctx: &Context<'_>, args: &AdoptArgs) -> Result<String, Outcome> {
    let Source::File(existing) = configure::source(ctx, &args.config) else {
        return Err(Outcome::failed(
            RunExit::InvalidConfig,
            "rulebearing adopt: no configuration found. adopt keeps an existing dependency-cruiser configuration; for a repository with none, run `rulebearing init`\n",
        ));
    };
    let name = existing
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if ConfigFormat::detect(&name) == Some(ConfigFormat::Native) {
        return Err(Outcome::failed(
            RunExit::InvalidConfig,
            format!(
                "rulebearing adopt: {name} is already a Rulebearing configuration. adopt writes rulebearing.yaml around a dependency-cruiser one; add `options.knownViolations` to {name} instead\n"
            ),
        ));
    }
    let relative = existing.strip_prefix(&ctx.cwd).unwrap_or(&existing);
    Ok(format!(
        "./{}",
        relative.to_string_lossy().replace('\\', "/")
    ))
}

/// Cruises with the repository's configuration, baselines what it finds, and cruises again with
/// the written configuration: the text, the loaded configuration and the entries, or why not.
fn baselined(
    ctx: &mut Context<'_>,
    args: &AdoptArgs,
    extends: &str,
    paths: &[String],
) -> Result<(String, Config, Vec<Value>), Outcome> {
    let original = configure::required(ctx, &args.config)?;
    let first = cruise(ctx, &original, paths)?;
    if !first.evaluation.vacuous.is_empty() {
        let names: Vec<&str> = first
            .evaluation
            .vacuous
            .iter()
            .map(|v| v.name.as_str())
            .collect();
        return Err(Outcome::failed(
            RunExit::Untrustworthy,
            format!(
                "rulebearing adopt: rules {} match no module in {}, so they check nothing. Fix their paths or pass the right folders, then run adopt again (ADR-0007)\n",
                names.join(", "),
                paths.join(" ")
            ),
        ));
    }
    let entries = baseline(
        ctx,
        &first.evaluation.document.summary.violations,
        &args.baseline,
    )?;
    let text = adopted_config(extends, &entries, &ctx.today.to_string());
    let options = LoadOptions {
        via_node: args.config.config_via_node,
        ..LoadOptions::default()
    };
    let adopted = load::load_text(&text, Syntax::Yaml, &ctx.cwd, &options).map_err(|e| {
        Outcome::failed(
            RunExit::InvalidConfig,
            format!("rulebearing adopt: the written configuration does not load: {e}\n"),
        )
    })?;
    let second = cruise(ctx, &adopted, paths)?;
    let remaining = second.evaluation.error_count();
    if remaining != 0 || !second.evaluation.vacuous.is_empty() {
        return Err(Outcome::failed(
            RunExit::Untrustworthy,
            format!(
                "rulebearing adopt: with the baseline the gate still has {remaining} errors; nothing was written. Please report this with the output of `rulebearing cruise -T json`\n"
            ),
        ));
    }
    Ok((text, adopted, entries))
}

/// Runs `adopt`.
pub fn run(ctx: &mut Context<'_>, args: &AdoptArgs) -> Outcome {
    let extends = match wrapped(ctx, args) {
        Ok(e) => e,
        Err(o) => return o,
    };
    let paths = if args.paths.is_empty() {
        init::discover(&ctx.cwd).roots
    } else {
        args.paths.clone()
    };
    let (text, adopted, entries) = match baselined(ctx, args, &extends, &paths) {
        Ok(v) => v,
        Err(o) => return o,
    };
    let (ci_file, ci_text) = ci_step(args.ci, &paths);
    let (hook_file, hook_text) = hook(&ctx.cwd, &paths);
    let hook_path = ctx.resolve(&hook_file);
    let mut files = vec![
        ("rulebearing.yaml".to_owned(), text),
        (hook_file, hook_text),
    ];
    if !ctx.resolve(ci_file).exists() {
        files.push((ci_file.to_owned(), ci_text));
    }
    files.push((
        "docs/architecture/rulebearing.md".into(),
        architecture_page(&adopted, &entries, &extends),
    ));
    for (file, text) in &files {
        if let Err(o) = write(ctx, file, text) {
            return o;
        }
    }
    executable(&hook_path);
    let names: Vec<String> = files.into_iter().map(|(f, _)| f).collect();
    let mut report = format!(
        "baselined {} findings; a cruise with rulebearing.yaml exits 0\nwrote:\n",
        entries.len()
    );
    for n in &names {
        let _ = writeln!(report, "  {n}");
    }
    if !args.no_pr {
        let body = pr_body(&extends, &entries, &names, env!("CARGO_PKG_VERSION"));
        report.push_str(&open_pull_request(ctx, &names, &body));
    }
    Outcome::printed(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_takes_a_date_or_days() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 23).unwrap_or_default();
        assert_eq!(
            expiry(today, None).map(|d| d.to_string()),
            Ok("2026-12-22".into())
        );
        assert_eq!(
            expiry(today, Some("10")).map(|d| d.to_string()),
            Ok("2026-10-03".into())
        );
        assert_eq!(
            expiry(today, Some("2027-01-31")).map(|d| d.to_string()),
            Ok("2027-01-31".into())
        );
        assert!(expiry(today, Some("soon")).is_err());
    }

    #[test]
    fn ci_steps_and_hooks() {
        let paths = vec!["src".to_owned()];
        let (file, text) = ci_step(Ci::Github, &paths);
        assert_eq!(file, ".github/workflows/rulebearing.yml");
        assert!(
            text.contains("args: --config rulebearing.yaml src") && text.contains("contents: read")
        );
        let (file, text) = ci_step(Ci::Azure, &paths);
        assert_eq!(file, "azure-pipelines.rulebearing.yml");
        assert!(text.contains("--output-type azure-devops src"));
        let dir = std::env::temp_dir().join(format!("rb-adopt-hook-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(dir.join(".husky"));
        let (file, text) = hook(&dir, &paths);
        assert_eq!(file, ".husky/pre-commit");
        assert!(text.starts_with("npx --no-install rulebearing cruise"));
        let _ = std::fs::write(dir.join(".husky/pre-commit"), &text);
        assert_eq!(hook(&dir, &paths).1, text, "a second run adds nothing");
        // An existing Husky hook without a final newline keeps its last command whole.
        let _ = std::fs::write(dir.join(".husky/pre-commit"), "npm test");
        assert_eq!(
            hook(&dir, &paths).1,
            format!("npm test\n{text}"),
            "appended on a line of its own"
        );
        let _ = std::fs::remove_dir_all(dir.join(".husky"));
        assert_eq!(hook(&dir, &paths).0, ".githooks/pre-commit");
        assert!(hook(&dir, &paths).1.starts_with("#!/bin/sh\n"));
        // An existing .githooks/pre-commit is extended, never replaced.
        let _ = std::fs::create_dir_all(dir.join(".githooks"));
        let _ = std::fs::write(
            dir.join(".githooks/pre-commit"),
            "#!/bin/sh\ncargo fmt --check\n",
        );
        let (_, merged) = hook(&dir, &paths);
        assert!(merged.starts_with("#!/bin/sh\ncargo fmt --check\nnpx --no-install rulebearing"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
