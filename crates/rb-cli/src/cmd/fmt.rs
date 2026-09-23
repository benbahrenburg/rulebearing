//! `rulebearing fmt`: re-report a saved result without extracting; dependency-cruiser's
//! `depcruise-fmt`.
//!
//! - Source: [design § The subcommands a guard reaches for](../../../../docs/artifacts/design.md#the-subcommands-a-guard-reaches-for)
//! - Coverage: [coverage § Command line](../../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line),
//!   row `depcruise-fmt`
//! - Plan: [Wave 1, Step 13](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-13-rb-cli-cruise-fmt-exit-codes-flags-1d)
//! - Requirements: [FR-CORE-02](../../../../docs/prd.md#fr-core-02), [FR-CLI-01](../../../../docs/prd.md#fr-cli-01)
//!
//! `fmt` never reads the source tree: its only input is the result. As `depcruise-fmt`, it exits
//! 0 unless `--exit-code` asks for the error count; an input that is not a result exits 2.

use rb_report::ReportOptions;
use rb_rules::graph::filters::{Filter, Filters};
use rb_rules::rewrap::{FormatOptions, collapse_pattern, rewrap};
use serde_json::{Map, Value, json};

use crate::cli::FmtArgs;
use crate::cmd::cruise::color;
use crate::context::Context;
use crate::exit::RunExit;
use crate::{Outcome, ratchets, write_output};

fn failed(code: RunExit, message: &str) -> Outcome {
    Outcome {
        stdout: String::new(),
        stderr: format!("rulebearing fmt: {message}\n"),
        code: code.code(),
    }
}

fn filter(pattern: Option<&String>, depth: Option<u32>) -> Option<Filter> {
    pattern.map(|p| Filter {
        path: Some(p.clone()),
        depth,
    })
}

/// The format options the flags ask for.
pub fn format_options(args: &FmtArgs) -> FormatOptions {
    let mut options = Map::new();
    options.insert("outputType".into(), json!(args.output_type));
    options.insert("outputTo".into(), json!(args.output_to));
    if let Some(prefix) = &args.prefix {
        options.insert("prefix".into(), json!(prefix));
    }
    FormatOptions {
        filters: Filters {
            exclude: filter(args.exclude.as_ref(), None),
            include_only: filter(args.include_only.as_ref(), None),
            focus: filter(args.focus.as_ref(), args.focus_depth),
            reaches: filter(args.reaches.as_ref(), None),
            highlight: filter(args.highlight.as_ref(), None),
        },
        collapse: args
            .collapse
            .as_ref()
            .and_then(|c| collapse_pattern(&Value::String(c.clone()))),
        options,
    }
}

/// Runs `fmt`.
pub fn run(ctx: &mut Context<'_>, args: &FmtArgs) -> Outcome {
    if let Some(from) = args
        .from
        .as_deref()
        .filter(|f| !matches!(*f, "rulebearing" | "dependency-cruiser"))
    {
        return failed(
            RunExit::InvalidConfig,
            &format!("--from `{from}`: use rulebearing or dependency-cruiser"),
        );
    }
    let text = if args.input == "-" {
        ctx.read_stdin().map_err(|e| e.to_string())
    } else {
        std::fs::read_to_string(ctx.resolve(&args.input))
            .map_err(|e| format!("{}: {e}", args.input))
    };
    let text = match text {
        Ok(t) => t,
        Err(e) => return failed(RunExit::Untrustworthy, &e),
    };
    let document = match rb_ingest::dependency_cruiser::read(&text) {
        Ok(d) => d,
        Err(e) => {
            return failed(
                RunExit::Untrustworthy,
                &format!("{} is not a cruise result: {e}", args.input),
            );
        }
    };
    let document = match rewrap(document, &format_options(args), None) {
        Ok(d) => d,
        Err(e) => return failed(RunExit::Untrustworthy, &e.to_string()),
    };
    let value = serde_json::to_value(&document).unwrap_or(Value::Null);
    let options = ReportOptions {
        color: color(args.color, ctx.color_terminal) && args.output_to == "-",
        strict_schema: args.strict_schema,
        max_findings: args.max_findings,
        timestamp: ctx.timestamp.clone(),
    };
    let rendered = match rb_report::render(&args.output_type, &value, &options) {
        Ok(r) => r,
        Err(e) => return failed(RunExit::InvalidConfig, &e.to_string()),
    };
    let mut stdout = String::new();
    if let Err(message) = write_output(ctx, &args.output_to, &rendered.output, &mut stdout) {
        return failed(RunExit::Untrustworthy, &message);
    }
    let (exceeded, no_budget) = ratchets::from_summary(document.summary.ratchets.as_deref());
    // An entry the run only warned about (liveness `warn`) does not fail it (ADR-0032).
    let vacuous = document
        .summary
        .vacuous_rules
        .iter()
        .flatten()
        .any(rb_model::VacuousRule::fails);
    let expired = document.summary.expired.as_ref().map_or(0, Vec::len) as u64;
    // As depcruise-fmt: without --exit-code, 0; with it, the code the cruise gave for this
    // reporter, from what the saved result carries (ADR-0030, ADR-0031).
    let code = if !args.exit_code {
        RunExit::Violations(0)
    } else if no_budget || vacuous {
        RunExit::Untrustworthy
    } else if rb_report::gates(&args.output_type) {
        RunExit::Violations(document.summary.error + exceeded + expired)
    } else {
        RunExit::Violations(0)
    };
    Outcome {
        stdout,
        stderr: String::new(),
        code: code.code(),
    }
}
