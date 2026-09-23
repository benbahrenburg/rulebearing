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
//! The exit code is the error count whatever the reporter, 2 when the run cannot be trusted (an
//! empty cruise, an unsupported file, a vacuous rule) and 3 for an invalid configuration. The
//! report is still written for a vacuous run, so the reader sees which rules matched nothing.

use std::fmt::Write as _;

use rb_config::Config;
use rb_report::ReportOptions;

use crate::cli::{ColorChoice, CruiseArgs};
use crate::context::Context;
use crate::exit::RunExit;
use crate::pipeline::{self, RunError, RunOptions};
use crate::progress::Progress;
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

fn failed(error: &RunError, stderr: String) -> Outcome {
    let code = match error {
        RunError::Config(_) => RunExit::InvalidConfig,
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
        Err(e) => return failed(&RunError::Config(e), String::new()),
    };
    let has_config = config.is_some();
    let mut effective = config.take().unwrap_or_default();
    if let Err(e) = configure::apply_flags(&mut effective, args, ctx) {
        return failed(&RunError::Config(e), String::new());
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
    let output_type = args
        .output_type
        .clone()
        .or_else(|| effective.options.output_type.clone())
        .unwrap_or_else(|| "err".into());
    let output_to = args
        .output_to
        .clone()
        .or_else(|| effective.options.output_to.clone())
        .unwrap_or_else(|| "-".into());
    let options = RunOptions {
        liveness: !args.no_liveness && has_config,
        options_used: configure::options_used(
            Some(&effective).filter(|_| has_config),
            ctx,
            &output_type,
            &output_to,
        ),
        paths: args.paths.clone(),
    };
    let run = match pipeline::run(ctx, &effective, &options, &mut progress) {
        Ok(run) => run,
        Err(e) => return failed(&e, stderr),
    };
    report(
        ctx,
        &effective,
        &run,
        args,
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
    args: &CruiseArgs,
    output_type: &str,
    output_to: &str,
    mut progress: Progress,
    mut stderr: String,
) -> Outcome {
    let value = match serde_json::to_value(&run.document) {
        Ok(v) => v,
        Err(e) => return failed(&RunError::Engine(e.into()), stderr),
    };
    let options = ReportOptions {
        color: color(args.color, ctx.color_terminal) && output_to == "-",
        strict_schema: args.strict_schema,
        max_findings: args.max_findings,
        timestamp: ctx.timestamp.clone(),
    };
    let rendered = match rb_report::render(output_type, &value, &options) {
        Ok(r) => r,
        Err(e) => {
            return failed(
                &RunError::Config(rb_config::ConfigError::Invalid(e.to_string())),
                stderr,
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
    let code = if run.evaluation.vacuous.is_empty() {
        RunExit::Violations(run.evaluation.error_count())
    } else {
        for v in &run.evaluation.vacuous {
            let _ = writeln!(
                stderr,
                "error: rule `{}` is vacuous: its {} side matched no module, so it checks nothing. Fix the pattern, delete the rule, or set allowEmpty: true (ADR-0007)",
                v.name, v.side
            );
        }
        RunExit::Untrustworthy
    };
    let _ = config;
    Outcome {
        stdout,
        stderr,
        code: code.code(),
    }
}
