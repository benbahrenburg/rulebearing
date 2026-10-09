//! `rulebearing fmt`: re-report a saved result without extracting; dependency-cruiser's
//! `depcruise-fmt`.
//!
//! - Source: [design § The subcommands a guard reaches for](../../../../docs/artifacts/design.md#the-subcommands-a-guard-reaches-for)
//! - Coverage: [coverage § Command line](../../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line),
//!   row `depcruise-fmt`
//! - Plan: [Wave 1, Step 13](../../../../docs/plans/implemented/0001-wave-1-typescript-parity.md#step-13-rb-cli-cruise-fmt-exit-codes-flags-1d)
//! - Requirements: [FR-CORE-02](../../../../docs/prd.md#fr-core-02), [FR-CLI-01](../../../../docs/prd.md#fr-cli-01)
//!
//! `fmt` never reads the source tree: its only input is the result. As `depcruise-fmt`, it exits
//! 0 unless `--exit-code` asks for the error count; an input that is not a result exits 2.
//! `--exit-code-mode strict` shifts the count to `10 + n`
//! ([Wave 3, Step 5](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)).
//! `-T plugin:<path>` renders in the sandbox, and with `--exit-code` the plugin's `exitCode` is the
//! count ([`crate::plugin`]; [Wave 3, Step 7](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)).
//! A saved result read in source mode never gates: with `--exit-code` it exits 2 unless
//! `--allow-approximate-gate` ([`crate::exit::gate`]; [Wave 3, Step 15](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)).

use rb_report::ReportOptions;
use rb_rules::graph::filters::{Filter, Filters};
use rb_rules::rewrap::{FormatOptions, collapse_pattern, rewrap};
use serde_json::{Map, Value, json};

use crate::cli::FmtArgs;
use crate::cmd::cruise::color;
use crate::context::Context;
use crate::exit::{
    APPROXIMATE_REASON, APPROXIMATE_STRICT_REASON, RunExit, gate, is_approximate,
    strips_approximate,
};
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

/// `--ignore-known [file]` softens the saved result's findings the file lists, as `cruise` would
/// have; `--no-ignore-known` puts every softened finding back at its rule's severity
/// (`summary.ruleSetUsed`). Either way the counts, and so `--exit-code`, follow.
fn known_violations(
    ctx: &Context<'_>,
    args: &FmtArgs,
    document: &mut rb_model::GraphDocument,
) -> Result<(), Outcome> {
    let result = if args.known.no_ignore_known {
        rb_rules::known::restore_severities(document)
    } else if let Some(file) = &args.known.ignore_known {
        let entries = rb_config::load::known_violations_file(&ctx.resolve(file))
            .map_err(|e| failed(RunExit::InvalidConfig, &e.to_string()))?;
        rb_rules::known::apply_to_document(document, &entries, ctx.today).map(|_| ())
    } else {
        Ok(())
    };
    result.map_err(|e| failed(RunExit::Untrustworthy, &e.to_string()))
}

/// `--from`: where the result came from (`rulebearing`, `dependency-cruiser`), or, for
/// `plantuml` only, what the diagram's nodes are.
fn from_applies(args: &FmtArgs) -> Result<(), String> {
    let Some(from) = args.from.as_deref() else {
        return Ok(());
    };
    if matches!(from, "rulebearing" | "dependency-cruiser") {
        return Ok(());
    }
    if rb_report::plantuml::From::parse(from).is_none() {
        return Err(format!(
            "--from `{from}`: use rulebearing or dependency-cruiser (where the result came from), or slices, types, namespaces or folders (the plantuml diagram's nodes)"
        ));
    }
    if args.output_type != "plantuml" {
        return Err(format!(
            "--from {from} applies to --output-type plantuml, not {}",
            args.output_type
        ));
    }
    Ok(())
}

/// The reporters' options for a saved result: the terminal, the flags and the repository.
fn report_options(ctx: &Context<'_>, args: &FmtArgs, format: &FormatOptions) -> ReportOptions {
    ReportOptions {
        color: color(args.color, ctx.color_terminal) && args.output_to == "-",
        strict_schema: args.strict_schema,
        max_findings: args.max_findings,
        timestamp: ctx.timestamp.clone(),
        path_prefix: if matches!(args.output_type.as_str(), "github-annotations" | "sarif") {
            ctx.repository_prefix()
        } else {
            String::new()
        },
        baseline: rb_report::baseline::Lifecycle::default(),
        collapse_pattern: format.collapse.clone(),
        plantuml_from: args
            .from
            .clone()
            .filter(|f| rb_report::plantuml::From::parse(f).is_some()),
        graphviz: Some(crate::graphviz::system()),
    }
}

/// Runs `fmt`.
pub fn run(ctx: &mut Context<'_>, args: &FmtArgs) -> Outcome {
    if let Err(message) = from_applies(args) {
        return failed(RunExit::InvalidConfig, &message);
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
    formatted(ctx, args, &text, None)
}

/// What a library call to `format` answers ([`answer`]): the reporter's output, never written to
/// `outputTo`, and the count the reporter gates on.
#[derive(Debug, Clone)]
pub struct Answer {
    /// The reporter's output.
    pub output: String,
    /// What `fmt --exit-code` would exit with for this reporter, uncapped: the error count for a
    /// reporter that gates (expired rules and exceeded ratchets included), a plugin's own count,
    /// 0 otherwise.
    pub violations: u64,
}

/// `text`, a saved cruise result, formatted as `args` ask, with `--exit-code`'s count and the
/// output returned rather than written. The Node binding's `format()`
/// ([Wave 3, Step 22](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#26-steps-for-sub-wave-3f-the-roslyn-analyzer-and-rb-node)).
///
/// # Errors
/// The command line's [`Outcome`] when the result cannot be read or reported (exit 2 or 3), with
/// the reason on its stderr.
pub fn answer(ctx: &Context<'_>, args: &FmtArgs, text: &str) -> Result<Answer, Outcome> {
    let mut args = args.clone();
    args.exit_code = true;
    let verdict = std::cell::Cell::new(None);
    let outcome = formatted(ctx, &args, text, Some(&verdict));
    match verdict.get() {
        Some(RunExit::Violations(violations)) => Ok(Answer {
            output: outcome.stdout,
            violations,
        }),
        _ => Err(outcome),
    }
}

/// Formats `text`; for a library call (`api`), the output is returned rather than written and
/// the verdict set.
fn formatted(
    ctx: &Context<'_>,
    args: &FmtArgs,
    text: &str,
    api: Option<&std::cell::Cell<Option<RunExit>>>,
) -> Outcome {
    let mut document = match rb_ingest::dependency_cruiser::read(text) {
        Ok(d) => d,
        Err(e) => {
            return failed(
                RunExit::Untrustworthy,
                &format!("{} is not a cruise result: {e}", args.input),
            );
        }
    };
    if let Err(outcome) = known_violations(ctx, args, &mut document) {
        return outcome;
    }
    let format = format_options(args);
    let document = match rewrap(document, &format, None) {
        Ok(d) => d,
        Err(e) => return failed(RunExit::Untrustworthy, &e.to_string()),
    };
    let approximate = is_approximate(&document);
    if strips_approximate(
        &args.output_type,
        args.strict_schema,
        approximate,
        args.allow_approximate_gate,
    ) {
        return failed(RunExit::Untrustworthy, APPROXIMATE_STRICT_REASON);
    }
    let mut value = serde_json::to_value(&document).unwrap_or(Value::Null);
    let options = report_options(ctx, args, &format);
    let plugin = rb_config::js::plugin::plugin_name(&args.output_type);
    let rendered = match plugin {
        // A plugin reporter, in the sandbox (crate::plugin); its failures are exit 3.
        Some(name) => crate::plugin::render(&ctx.cwd, name, &mut value, args.strict_schema)
            .map_err(|e| (RunExit::InvalidConfig, e.to_string())),
        None => rb_report::render(&args.output_type, &value, &options).map_err(|e| match e {
            // `x-dot-webpage` without a working `dot`: the report could not be made (ADR-0053).
            rb_report::ReportError::Graphviz(_) => (RunExit::Untrustworthy, e.to_string()),
            other => (RunExit::InvalidConfig, other.to_string()),
        }),
    };
    let rendered = match rendered {
        Ok(r) => r,
        Err((code, message)) => return failed(code, &message),
    };
    let mut stdout = String::new();
    if api.is_some() {
        rendered.output.clone_into(&mut stdout);
    } else if let Err(message) = write_output(ctx, &args.output_to, &rendered.output, &mut stdout) {
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
    let decides = args.exit_code && (rb_report::gates(&args.output_type) || plugin.is_some());
    let code = if !args.exit_code {
        RunExit::Violations(0)
    } else if no_budget || vacuous {
        RunExit::Untrustworthy
    } else if rb_report::gates(&args.output_type) {
        RunExit::Violations(document.summary.error + exceeded + expired)
    } else if plugin.is_some() {
        // A plugin decides its own count (ADR-0030).
        RunExit::Violations(rendered.exit_code)
    } else {
        RunExit::Violations(0)
    };
    // A saved result read in source mode never gates (ADR-0011).
    let refused = gate(code, decides, approximate, args.allow_approximate_gate);
    if let Some(verdict) = api {
        verdict.set(Some(refused.unwrap_or(code)));
    }
    match refused {
        Some(refused) => Outcome {
            stdout,
            stderr: format!("warning: {APPROXIMATE_REASON}\n"),
            code: refused.code_in(args.exit_code_mode),
        },
        None => Outcome {
            stdout,
            stderr: String::new(),
            code: code.code_in(args.exit_code_mode),
        },
    }
}
