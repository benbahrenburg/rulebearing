//! `rulebearing cruise`: extract, evaluate and report; dependency-cruiser's `depcruise`.
//!
//! - Source: [design § The subcommands a guard reaches for](../../../../docs/artifacts/design.md#the-subcommands-a-guard-reaches-for)
//! - Coverage: [coverage § Command line](../../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line)
//! - Decisions: [ADR-0008](../../../../docs/adr/0008-exit-code-contract.md),
//!   [ADR-0007](../../../../docs/adr/0007-vacuous-rules-fail-by-default.md),
//!   [ADR-0017](../../../../docs/adr/0017-coffeescript-livescript-sidecar.md)
//! - Plan: [Wave 1, Step 13](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-13-rb-cli-cruise-fmt-exit-codes-flags-1d)
//! - Requirements: [FR-CORE-02](../../../../docs/prd.md#fr-core-02), [FR-CORE-06](../../../../docs/prd.md#fr-core-06),
//!   [FR-CLI-08](../../../../docs/prd.md#fr-cli-08)
//!
//! A gating reporter exits with the error count ([ADR-0030](../../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md)),
//! every reporter exits 2 when the run cannot be trusted (an empty cruise, an unsupported file,
//! a vacuous rule) and 3 for an invalid configuration. The report is still written for a vacuous
//! run, so the reader sees which rules matched nothing.
//!
//! `--from-hook` answers as a Claude Code `Stop` hook ([docs/agents.md](../../../../docs/agents.md#the-hooks)):
//! with error findings it prints `{"decision": "block", "reason": <the agent report>}`, which
//! hands the findings to the agent before the turn ends; otherwise nothing. It exits 0 always, and
//! does nothing when the hook input says a `Stop` hook already kept the turn going
//! (`stop_hook_active`), so the agent is never held in a loop.

use std::fmt::Write as _;

use rb_config::Config;
use rb_report::ReportOptions;

use crate::cli::{ColorChoice, CruiseArgs, Liveness};
use crate::context::Context;
use crate::exit::RunExit;
use crate::pipeline::{self, RunError, RunOptions};
use crate::progress::Progress;
use crate::ratchets::{self, Ratchets};
use crate::{Outcome, configure, write_output};

/// The languages and extensions this build reads (`--info`).
pub fn info() -> String {
    let mut out = String::from("rulebearing supports:\n\n  extension  how\n");
    #[cfg(feature = "extract-ts")]
    {
        for ext in rb_extract_ts::EXTENSIONS {
            let _ = writeln!(out, "  .{ext:<9} native (oxc)");
        }
        for ext in rb_extract_ts::SIDECAR_EXTENSIONS {
            let _ = writeln!(
                out,
                "  .{ext:<9} the Node sidecar (wave 3); without it the run exits 2 (ADR-0017)"
            );
        }
    }
    #[cfg(not(feature = "extract-ts"))]
    out.push_str("  (this build has no TypeScript extractor)\n");
    out.push_str(
        "\nparsers: acorn, swc and tsc are accepted and recorded; oxc reads every one of them\n",
    );
    out
}

/// Whether to colour.
pub fn color(choice: ColorChoice, terminal: bool) -> bool {
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => terminal && std::env::var_os("NO_COLOR").is_none(),
    }
}

fn failed(error: &RunError, stderr: &str) -> Outcome {
    let code = match error {
        RunError::Config(_) | RunError::Engine(rb_rules::EngineError::Element(_)) => {
            RunExit::InvalidConfig
        }
        RunError::Extract(_) | RunError::Engine(_) => RunExit::Untrustworthy,
    };
    Outcome {
        stdout: String::new(),
        stderr: format!("{stderr}rulebearing cruise: {error}\n"),
        code: code.code(),
    }
}

/// Runs `cruise`.
pub fn run(ctx: &mut Context<'_>, args: &CruiseArgs) -> Outcome {
    if !args.from_hook {
        return cruise(ctx, args);
    }
    let input = ctx.read_stdin().unwrap_or_default();
    let active = serde_json::from_str::<serde_json::Value>(&input)
        .ok()
        .and_then(|v| {
            v.get("stop_hook_active")
                .and_then(serde_json::Value::as_bool)
        })
        .unwrap_or(false);
    if active {
        return Outcome::printed(String::new());
    }
    let outcome = cruise(ctx, args);
    Outcome { code: 0, ..outcome }
}

fn cruise(ctx: &mut Context<'_>, args: &CruiseArgs) -> Outcome {
    if args.info {
        return Outcome {
            stdout: info(),
            stderr: String::new(),
            code: 0,
        };
    }
    let mut progress = Progress::new(if args.no_progress {
        None
    } else {
        args.progress
    });
    let mut config = match configure::load(ctx, &args.config) {
        Ok(config) => config,
        Err(e) => return failed(&RunError::Config(e), ""),
    };
    let has_config = config.is_some();
    let liveness = Liveness::of(args.liveness, args.no_liveness, config.as_ref());
    let mut effective = config.take().unwrap_or_default();
    if let Err(e) = configure::apply_flags(&mut effective, args, ctx) {
        return failed(&RunError::Config(e), "");
    }
    progress.stage("configuration");
    let mut stderr = String::new();
    for warning in &effective.warnings {
        let rule = warning
            .rule
            .as_deref()
            .map(|r| format!("rule `{r}`: "))
            .unwrap_or_default();
        let _ = writeln!(stderr, "warning: {rule}{}", warning.message);
    }
    let (output_type, output_to) = if args.from_hook {
        ("agent".to_owned(), "-".to_owned())
    } else {
        (
            args.output_type
                .clone()
                .or_else(|| effective.options.output_type.clone())
                .unwrap_or_else(|| "err".into()),
            args.output_to
                .clone()
                .or_else(|| effective.options.output_to.clone())
                .unwrap_or_else(|| "-".into()),
        )
    };
    effective.options.metrics = Some(configure::wants_metrics(
        &effective,
        args.metrics && !args.no_metrics,
        &output_type,
    ));
    let options = RunOptions {
        liveness: liveness != Liveness::Off,
        options_used: configure::options_used(
            has_config.then_some(&effective),
            ctx,
            &output_type,
            &output_to,
        ),
        paths: args.paths.clone(),
    };
    let result = match &args.graph {
        Some(file) => match pipeline::load_graph(ctx, file) {
            Ok(mut document) => {
                pipeline::reset(&mut document);
                progress.stage("read graph");
                pipeline::evaluate_document(ctx, &effective, document, &options, &mut progress)
            }
            Err(message) => {
                return Outcome::failed(
                    RunExit::Untrustworthy,
                    format!("{stderr}rulebearing cruise: {message}\n"),
                );
            }
        },
        None => pipeline::run(ctx, &effective, &options, &mut progress),
    };
    let mut run = match result {
        Ok(run) => run,
        Err(e) => return failed(&e, &stderr),
    };
    for warning in &run.warnings {
        let _ = writeln!(stderr, "warning: {warning}");
    }
    let ratchets = ratchets::evaluate(ctx, &effective, &run.evaluation.document, options.liveness);
    summarise(&mut run.document.summary, &ratchets, liveness);
    report(
        ctx,
        &effective,
        &run,
        &ratchets,
        (args, liveness),
        &output_type,
        &output_to,
        progress,
        stderr,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the report step reads every part of the run once"
)]
fn report(
    ctx: &Context<'_>,
    config: &Config,
    run: &pipeline::Run,
    ratchets: &Ratchets,
    (args, liveness): (&CruiseArgs, Liveness),
    output_type: &str,
    output_to: &str,
    mut progress: Progress,
    mut stderr: String,
) -> Outcome {
    let value = match serde_json::to_value(&run.document) {
        Ok(v) => v,
        Err(e) => return failed(&RunError::Engine(e.into()), &stderr),
    };
    let options = ReportOptions {
        color: color(args.color, ctx.color_terminal) && output_to == "-",
        strict_schema: args.strict_schema,
        max_findings: args.max_findings,
        timestamp: ctx.timestamp.clone(),
        path_prefix: if output_type == "github-annotations" {
            ctx.repository_prefix()
        } else {
            String::new()
        },
        // The cruise applies no `collapse` to its modules (pipeline.rs), so none reaches the
        // reporter either.
        collapse_pattern: None,
    };
    let rendered = match rb_report::render(output_type, &value, &options) {
        Ok(r) => r,
        Err(e) => {
            return failed(
                &RunError::Config(rb_config::ConfigError::Invalid(e.to_string())),
                &stderr,
            );
        }
    };
    progress.stage("report");
    let mut stdout = String::new();
    if let Err(message) = write_output(ctx, output_to, &rendered.output, &mut stdout) {
        let _ = writeln!(stderr, "rulebearing cruise: {message}");
        return Outcome {
            stdout,
            stderr,
            code: RunExit::Untrustworthy.code(),
        };
    }
    stderr.insert_str(0, &progress.finish());
    for expired in &run.evaluation.expired {
        let _ = writeln!(
            stderr,
            "error: {} `{}` expired on {}; it no longer applies and the run fails",
            expired.kind, expired.name, expired.expires
        );
    }
    let strict = liveness == Liveness::Strict;
    stderr.push_str(&ratchets::messages(config, ratchets, strict));
    for v in &run.evaluation.vacuous {
        let _ = writeln!(stderr, "{}", vacuous_message(&v.name, &v.side, strict));
    }
    let vacuous = !run.evaluation.vacuous.is_empty() || !ratchets.vacuous.is_empty();
    // 2 whatever the reporter; the error count only for a reporter that gates (ADR-0030). A rule
    // that matches nothing counts under strict liveness only (ADR-0032).
    let code = if (strict && vacuous) || ratchets.no_budget() {
        RunExit::Untrustworthy
    } else if rb_report::gates(output_type) {
        RunExit::Violations(run.evaluation.error_count() + ratchets.exceeded())
    } else {
        RunExit::Violations(0)
    };
    if args.from_hook {
        return stop_hook(code, &stdout, stderr);
    }
    Outcome {
        stdout,
        stderr,
        code: code.code(),
    }
}

/// Adds the ratchets and their vacuous entries to the summary. Under `warn` every vacuous entry
/// stays in the result, marked, so `fmt --exit-code` reads the same verdict from the saved file
/// (ADR-0029, ADR-0031, ADR-0032).
fn summarise(summary: &mut rb_model::Summary, ratchets: &Ratchets, liveness: Liveness) {
    if !ratchets.results.is_empty() {
        summary.ratchets = Some(ratchets.results.clone());
    }
    if !ratchets.vacuous.is_empty() {
        summary
            .vacuous_rules
            .get_or_insert_with(Vec::new)
            .extend(ratchets.vacuous.iter().cloned());
    }
    if liveness == Liveness::Warn {
        for entry in summary.vacuous_rules.iter_mut().flatten() {
            entry.severity = Some("warn".into());
        }
    }
}

/// The line for a rule that matches nothing: an error under strict liveness, else a warning that
/// says how to make it one.
pub fn vacuous_message(name: &str, side: &str, strict: bool) -> String {
    if strict {
        format!(
            "error: rule `{name}` is vacuous: its {side} side matched no module, so it checks nothing. Fix the pattern, delete the rule, or excuse it with allowEmpty (ADR-0007, ADR-0032)"
        )
    } else {
        format!(
            "warning: rule `{name}` is vacuous: its {side} side matched no module, so it checks nothing. dependency-cruiser does not check this and the run goes on; --liveness strict fails it (ADR-0032)"
        )
    }
}

/// The `Stop` hook's answer: block, with the agent report and the run's errors as the reason,
/// only when a trustworthy run found errors.
fn stop_hook(code: RunExit, report: &str, stderr: String) -> Outcome {
    let count = match code {
        RunExit::Violations(count) if count > 0 => count,
        _ => {
            return Outcome {
                stdout: String::new(),
                stderr,
                code: 0,
            };
        }
    };
    let mut errors = String::new();
    for line in stderr.lines().filter(|l| l.starts_with("error:")) {
        let _ = writeln!(errors, "{line}");
    }
    let reason = format!(
        "rulebearing found {count} error(s) against the architecture rules; fix them before ending the turn. Each rule's `fix` says how.\n{errors}{report}"
    );
    let mut text = serde_json::json!({ "decision": "block", "reason": reason }).to_string();
    text.push('\n');
    Outcome {
        stdout: text,
        stderr,
        code: 0,
    }
}
