//! `rulebearing baseline`: write the current violations as known violations; dependency-cruiser's
//! `depcruise-baseline` (`depcruise -c -T baseline -f .dependency-cruiser-known-violations.json`),
//! with three modes.
//!
//! - Coverage: [dc coverage § Command line](../../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line)
//!   (`depcruise-baseline`, `--baseline-mode`)
//! - Source: [design § import-linter contracts](../../../../docs/artifacts/design.md#import-linter-contracts-for-the-python-teams-who-know-them)
//!   (`shrink-only` replaces import-linter's unmatched-ignore alerting)
//! - Plan: [Wave 2, Step 10](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Decisions: [ADR-0015](../../../../docs/adr/0015-stable-violation-id.md) (entries are keyed by
//!   the stable id), [ADR-0008](../../../../docs/adr/0008-exit-code-contract.md)
//! - Requirements: [FR-RULE-09](../../../../docs/prd.md#fr-rule-09), [FR-CLI-01](../../../../docs/prd.md#fr-cli-01)
//!
//! dependency-cruiser 18.2.0's `depcruise-baseline` has one behaviour and no modes; it is `full`.
//!
//! | Mode | Reads | Writes | Exits |
//! | --- | --- | --- | --- |
//! | `full` (default) | the tree and the configuration | every current violation, the `baseline` reporter's output; an entry whose `id` was already in the file keeps its `expires`, `owner` and `reason` | 0 |
//! | `shrink-only` | the tree, and the baseline file (or, without one, `options.knownViolations`) as the known violations | the file less the entries no violation matches any more; never adds one | the number of entries that no longer occur, each printed; 0 when every entry still occurs |
//! | `format` | the baseline file only | the same entries, validated, sorted by rule, `from`, `to` and `id`, with sorted keys | 0 |
//!
//! `--expires`, `--owner` and `--reason` fill those fields on every entry written that lacks them.
//! A malformed flag, file or configuration exits 3.

use std::fmt::Write as _;
use std::path::Path;

use chrono::NaiveDate;
use clap::{Args, ValueEnum};
use rb_config::model::KnownViolation;
use rb_report::baseline::{LIFECYCLE_KEYS, Lifecycle};
use serde_json::Value;

use crate::cli::{ConfigArgs, CruiseArgs};
use crate::context::Context;
use crate::exit::RunExit;
use crate::pipeline::{self, RunError, RunOptions};
use crate::progress::Progress;
use crate::{Outcome, configure, write_output};

/// How `baseline` treats the file it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum BaselineMode {
    /// Every current violation, as `depcruise-baseline` writes.
    #[default]
    Full,
    /// Only remove entries that no longer occur, and fail naming each one.
    ShrinkOnly,
    /// Rewrite the file in canonical form without cruising.
    Format,
}

/// `baseline`.
#[derive(Debug, Clone, Default, Args)]
pub struct BaselineArgs {
    /// Files, directories and globs to cruise
    #[arg(value_name = "FILES-OR-DIRECTORIES")]
    pub paths: Vec<String>,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
    /// Evaluate the rules over this graph document instead of extracting
    #[arg(long, value_name = "FILE")]
    pub graph: Option<String>,
    /// The baseline file; - for stdout
    #[arg(short = 'f', long, value_name = "FILE",
          default_value = rb_config::load::DEFAULT_KNOWN_VIOLATIONS_FILE)]
    pub output_to: String,
    /// full writes every current violation; shrink-only removes entries that no longer occur and
    /// fails naming them; format rewrites the file in canonical form
    #[arg(long, value_enum, value_name = "MODE", default_value_t = BaselineMode::Full)]
    pub baseline_mode: BaselineMode,
    /// The last day the entries apply (YYYY-MM-DD), on each entry that has none
    #[arg(long, value_name = "DATE")]
    pub expires: Option<String>,
    /// Who answers for the entries, on each entry that has none
    #[arg(long, value_name = "NAME")]
    pub owner: Option<String>,
    /// Why the entries stand, on each entry that has none
    #[arg(long, value_name = "TEXT")]
    pub reason: Option<String>,
}

fn failed(code: RunExit, message: &str) -> Outcome {
    Outcome::failed(code, format!("rulebearing baseline: {message}\n"))
}

fn run_failed(error: &RunError) -> Outcome {
    let code = match error {
        RunError::Config(_) | RunError::Engine(rb_rules::EngineError::Element(_)) => {
            RunExit::InvalidConfig
        }
        RunError::Extract(_) | RunError::Engine(_) => RunExit::Untrustworthy,
    };
    failed(code, &error.to_string())
}

/// The lifecycle the flags give, with `--expires` checked.
///
/// # Errors
/// An [`Outcome`] (exit 3) when `--expires` is not a date.
pub fn lifecycle(args: &BaselineArgs) -> Result<Lifecycle, Outcome> {
    if let Some(date) = &args.expires
        && NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err()
    {
        return Err(failed(
            RunExit::InvalidConfig,
            &format!("--expires `{date}`: give a date as YYYY-MM-DD"),
        ));
    }
    Ok(Lifecycle {
        expires: args.expires.clone(),
        owner: args.owner.clone(),
        reason: args.reason.clone(),
    })
}

/// Fills each lifecycle field an entry lacks.
fn fill(entry: &mut Value, lifecycle: &Lifecycle) {
    if let Value::Object(map) = entry {
        for (key, value) in
            LIFECYCLE_KEYS
                .iter()
                .zip([&lifecycle.expires, &lifecycle.owner, &lifecycle.reason])
        {
            if let Some(value) = value
                && map.get(*key).is_none_or(Value::is_null)
            {
                map.insert((*key).to_owned(), Value::String(value.clone()));
            }
        }
    }
}

/// A baseline file: its entries as written, and as the engine reads them.
struct File {
    raw: Vec<Value>,
    entries: Vec<KnownViolation>,
}

/// Reads the baseline file, or `None` when it does not exist.
fn read(path: &Path) -> Result<Option<File>, Outcome> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).map_err(|e| {
        failed(
            RunExit::InvalidConfig,
            &format!("cannot read {}: {e}", path.display()),
        )
    })?;
    let entries = rb_config::load::known_violations_text(path, &text)
        .map_err(|e| failed(RunExit::InvalidConfig, &e.to_string()))?;
    let raw: Vec<Value> =
        serde_json::from_str(&text).map_err(|e| failed(RunExit::InvalidConfig, &e.to_string()))?;
    Ok(Some(File { raw, entries }))
}

/// How an entry is named when it is reported: its id, then its rule and edge.
pub fn describe(entry: &Value) -> String {
    let text = |key: &str| entry.get(key).and_then(Value::as_str).unwrap_or("?");
    let rule = entry
        .get("rule")
        .and_then(|r| r.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("?");
    let edge = format!("rule `{rule}`: {} -> {}", text("from"), text("to"));
    match entry.get("id").and_then(Value::as_str) {
        Some(id) => format!("{id} ({edge})"),
        None => edge,
    }
}

/// The canonical order of `format`: rule name, `from`, `to`, `id`.
fn canonical_key(entry: &Value) -> (String, String, String, String) {
    let text = |v: Option<&Value>| v.and_then(Value::as_str).unwrap_or_default().to_owned();
    (
        text(entry.get("rule").and_then(|r| r.get("name"))),
        text(entry.get("from")),
        text(entry.get("to")),
        text(entry.get("id")),
    )
}

/// Runs `baseline`.
pub fn run(ctx: &mut Context<'_>, args: &BaselineArgs) -> Outcome {
    let lifecycle = match lifecycle(args) {
        Ok(l) => l,
        Err(outcome) => return outcome,
    };
    match args.baseline_mode {
        BaselineMode::Format => format(ctx, args, &lifecycle),
        BaselineMode::Full | BaselineMode::ShrinkOnly => cruise(ctx, args, &lifecycle),
    }
}

fn format(ctx: &Context<'_>, args: &BaselineArgs, lifecycle: &Lifecycle) -> Outcome {
    if args.output_to == "-" {
        return failed(
            RunExit::InvalidConfig,
            "--baseline-mode format rewrites a file; name it with -f",
        );
    }
    let path = ctx.resolve(&args.output_to);
    let file = match read(&path) {
        Ok(Some(file)) => file,
        Ok(None) => {
            return failed(
                RunExit::InvalidConfig,
                &format!(
                    "{} does not exist; write it with --baseline-mode full",
                    path.display()
                ),
            );
        }
        Err(outcome) => return outcome,
    };
    // Filled on the entries as the engine reads them, so each field lands in its place and a
    // second `format` writes the same bytes.
    let expires = lifecycle
        .expires
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
    let mut entries: Vec<Value> = file
        .entries
        .into_iter()
        .map(|mut e| {
            e.expires = e.expires.or(expires);
            e.owner = e.owner.or_else(|| lifecycle.owner.clone());
            e.reason = e.reason.or_else(|| lifecycle.reason.clone());
            e
        })
        .filter_map(|e| serde_json::to_value(e).ok())
        .collect();
    entries.sort_by_key(canonical_key);
    let mut stdout = String::new();
    match write_output(
        ctx,
        &args.output_to,
        &rb_report::baseline::text(&entries),
        &mut stdout,
    ) {
        Ok(()) => Outcome::printed(stdout),
        Err(message) => failed(RunExit::Untrustworthy, &message),
    }
}

/// Where `shrink-only` found the known violations it checks.
enum Source {
    File(File),
    Config,
}

fn cruise(ctx: &mut Context<'_>, args: &BaselineArgs, lifecycle: &Lifecycle) -> Outcome {
    let config = match configure::load(ctx, &args.config) {
        Ok(config) => config,
        Err(e) => return failed(RunExit::InvalidConfig, &e.to_string()),
    };
    let has_config = config.is_some();
    let mut effective = config.unwrap_or_default();
    let flags = CruiseArgs {
        paths: args.paths.clone(),
        ..CruiseArgs::default()
    };
    if let Err(e) = configure::apply_flags(&mut effective, &flags, ctx) {
        return failed(RunExit::InvalidConfig, &e.to_string());
    }
    let path = ctx.resolve(&args.output_to);
    let file = if args.output_to == "-" {
        None
    } else {
        match read(&path) {
            Ok(file) => file,
            Err(outcome) => return outcome,
        }
    };
    let shrink = args.baseline_mode == BaselineMode::ShrinkOnly;
    let source = match file {
        Some(file) => {
            if shrink {
                effective.known_violations.clone_from(&file.entries);
            }
            Some(Source::File(file))
        }
        None if effective.known_violations.is_empty() => None,
        None => Some(Source::Config),
    };
    if shrink {
        match &source {
            None => {
                return failed(
                    RunExit::InvalidConfig,
                    &format!(
                        "--baseline-mode shrink-only: {} does not exist and the configuration has no options.knownViolations; write a baseline with --baseline-mode full first",
                        path.display()
                    ),
                );
            }
            Some(Source::Config) if *lifecycle != Lifecycle::default() => {
                return failed(
                    RunExit::InvalidConfig,
                    "--expires, --owner and --reason under shrink-only edit a baseline file; the configuration's options.knownViolations are edited in the configuration",
                );
            }
            Some(_) => {}
        }
    }
    let options = RunOptions {
        liveness: false,
        options_used: configure::options_used(
            has_config.then_some(&effective),
            ctx,
            "baseline",
            &args.output_to,
        ),
        paths: args.paths.clone(),
    };
    let mut progress = Progress::new(None);
    let result = match &args.graph {
        Some(graph) => match pipeline::load_graph(ctx, graph) {
            Ok(mut document) => {
                pipeline::reset(&mut document);
                pipeline::evaluate_document(ctx, &effective, document, &options, &mut progress)
            }
            Err(message) => return failed(RunExit::Untrustworthy, &message),
        },
        None => pipeline::run(ctx, &effective, &options, &mut progress),
    };
    let run = match result {
        Ok(run) => run,
        Err(e) => return run_failed(&e),
    };
    if shrink {
        shrink_only(ctx, args, &run, source, &effective, lifecycle)
    } else {
        full(ctx, args, &run, source.as_ref(), &effective, lifecycle)
    }
}

fn full(
    ctx: &Context<'_>,
    args: &BaselineArgs,
    run: &pipeline::Run,
    source: Option<&Source>,
    config: &rb_config::Config,
    lifecycle: &Lifecycle,
) -> Outcome {
    let previous: Vec<Value> = match source {
        Some(Source::File(file)) => file.raw.clone(),
        Some(Source::Config) => config
            .known_violations
            .iter()
            .filter_map(|e| serde_json::to_value(e).ok())
            .collect(),
        None => Vec::new(),
    };
    let value = match serde_json::to_value(&run.document) {
        Ok(v) => v,
        Err(e) => return failed(RunExit::Untrustworthy, &e.to_string()),
    };
    let entries = rb_report::baseline::entries(&value, lifecycle, &previous);
    let mut stdout = String::new();
    match write_output(
        ctx,
        &args.output_to,
        &rb_report::baseline::text(&entries),
        &mut stdout,
    ) {
        Ok(()) => Outcome::printed(stdout),
        Err(message) => failed(RunExit::Untrustworthy, &message),
    }
}

fn shrink_only(
    ctx: &Context<'_>,
    args: &BaselineArgs,
    run: &pipeline::Run,
    source: Option<Source>,
    config: &rb_config::Config,
    lifecycle: &Lifecycle,
) -> Outcome {
    let stale = &run.evaluation.unmatched_known;
    let mut stderr = String::new();
    let mut stdout = String::new();
    match source {
        Some(Source::File(file)) => {
            let path = ctx.resolve(&args.output_to);
            for index in stale {
                if let Some(entry) = file.raw.get(*index) {
                    let _ = writeln!(
                        stderr,
                        "error: known violation {} no longer occurs; shrink-only removed it from {}",
                        describe(entry),
                        path.display()
                    );
                }
            }
            let mut kept: Vec<Value> = file
                .raw
                .iter()
                .enumerate()
                .filter(|(i, _)| !stale.contains(i))
                .map(|(_, e)| e.clone())
                .collect();
            for entry in &mut kept {
                fill(entry, lifecycle);
            }
            if kept != file.raw
                && let Err(message) = write_output(
                    ctx,
                    &args.output_to,
                    &rb_report::baseline::text(&kept),
                    &mut stdout,
                )
            {
                return failed(RunExit::Untrustworthy, &message);
            }
        }
        Some(Source::Config) | None => {
            let file = config.origin.as_ref().map_or_else(
                || "the configuration".to_owned(),
                |p| p.display().to_string(),
            );
            for index in stale {
                if let Some(entry) = config
                    .known_violations
                    .get(*index)
                    .and_then(|e| serde_json::to_value(e).ok())
                {
                    let _ = writeln!(
                        stderr,
                        "error: known violation {} no longer occurs; remove it from options.knownViolations in {file}",
                        describe(&entry)
                    );
                }
            }
        }
    }
    let unknown = run
        .document
        .summary
        .violations
        .iter()
        .filter(|v| v.rule.severity != rb_model::Severity::Ignore)
        .count();
    if unknown > 0 {
        let _ = writeln!(
            stderr,
            "warning: {unknown} finding(s) are not in the baseline; shrink-only does not add them (--baseline-mode full does)"
        );
    }
    Outcome {
        stdout,
        stderr,
        code: RunExit::Violations(stale.len() as u64).code(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn entries_are_described_by_id_then_rule_and_edge() {
        assert_eq!(
            describe(&json!({ "id": "RB-1", "from": "a", "to": "b", "rule": { "name": "r" } })),
            "RB-1 (rule `r`: a -> b)"
        );
        assert_eq!(describe(&json!({})), "rule `?`: ? -> ?");
    }

    #[test]
    fn flags_fill_only_missing_fields() {
        let lifecycle = Lifecycle {
            expires: Some("2026-12-31".into()),
            owner: Some("@me".into()),
            reason: None,
        };
        let mut entry = json!({ "id": "RB-1", "owner": "@them", "expires": null });
        fill(&mut entry, &lifecycle);
        assert_eq!(
            entry,
            json!({ "id": "RB-1", "owner": "@them", "expires": "2026-12-31" })
        );
        let bad = BaselineArgs {
            expires: Some("31/12/2026".into()),
            ..BaselineArgs::default()
        };
        assert_eq!(lifecycle_code(&bad), Some(3));
        let good = BaselineArgs {
            expires: Some("2026-12-31".into()),
            ..BaselineArgs::default()
        };
        assert_eq!(lifecycle_code(&good), None);
    }

    fn lifecycle_code(args: &BaselineArgs) -> Option<u8> {
        lifecycle(args).err().map(|o| o.code)
    }

    #[test]
    fn format_sorts_by_rule_from_to_and_id() {
        let mut entries = [
            json!({ "rule": { "name": "b" }, "from": "a" }),
            json!({ "rule": { "name": "a" }, "from": "z", "id": "RB-2" }),
            json!({ "rule": { "name": "a" }, "from": "z", "id": "RB-1" }),
            json!({ "rule": { "name": "a" }, "from": "c", "to": "d" }),
        ];
        entries.sort_by_key(canonical_key);
        let order: Vec<String> = entries.iter().map(describe).collect();
        assert_eq!(
            order,
            [
                "rule `a`: c -> d",
                "RB-1 (rule `a`: z -> ?)",
                "RB-2 (rule `a`: z -> ?)",
                "rule `b`: a -> ?"
            ]
        );
    }
}
