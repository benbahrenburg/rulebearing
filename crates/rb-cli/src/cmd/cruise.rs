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
//! `--affected [revision]` narrows the report to the changed modules and the modules that reach
//! them ([`crate::affected`]); `--exit-code-mode strict` shifts the count to `10 + n`
//! ([Wave 3, Step 5](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)).
//!
//! A gating reporter exits with the error count ([ADR-0030](../../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md)),
//! every reporter exits 2 when the run cannot be trusted (an empty cruise, an unsupported file,
//! a vacuous rule) and 3 for an invalid configuration. The report is still written for a vacuous
//! run, so the reader sees which rules matched nothing. A `plugin:<path>` reporter runs in the
//! sandbox and its `exitCode` is the count ([`crate::plugin`]). The cache's evaluated layer serves
//! a plugin as it serves any reporter, since the verdict does not depend on the reporter; the
//! rendered layer never serves or stores a plugin's output, because that output depends on the
//! plugin's code and everything it requires, which the key does not cover
//! ([Wave 3, Step 7](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)).
//!
//! `--from-hook` answers as a Claude Code `Stop` hook ([docs/agents.md](../../../../docs/agents.md#the-hooks)):
//! with error findings it prints `{"decision": "block", "reason": <the agent report>}`, which
//! hands the findings to the agent before the turn ends; otherwise nothing. It exits 0 always, and
//! does nothing when the hook input says a `Stop` hook already kept the turn going
//! (`stop_hook_active`), so the agent is never held in a loop.
//!
//! A run read in source mode whose reporter gates is refused, exit 2 with
//! `approximate-mode-not-a-gate`, unless `--allow-approximate-gate`; `--from-hook` still answers
//! with it, since the hook is the inner loop source mode is for ([`crate::exit::gate`];
//! [Wave 3, Step 15](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)).

use std::fmt::Write as _;

use rb_config::Config;
use rb_report::ReportOptions;

use crate::cache::Content;
use crate::cache::evaluated::{Tail, Verdict};
use crate::cli::{ColorChoice, CruiseArgs, Liveness};
use crate::context::Context;
use crate::exit::{
    APPROXIMATE_REASON, APPROXIMATE_STRICT_REASON, RunExit, gate, strips_approximate,
};
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
                "  .{ext:<9} --sidecar node: the repository's dependency-cruiser, run by Node; without it the run exits 2 (ADR-0017)"
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
    let code = error.exit();
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
    // A running `guard --watch` has the answer already, when it is fresh and for this command.
    if let Some(answer) = crate::cmd::guard::served(ctx, args) {
        return Outcome::printed(answer);
    }
    let outcome = cruise(ctx, args);
    Outcome { code: 0, ..outcome }
}

/// `--from` says what a plantuml diagram draws; with any other reporter it would be ignored, so
/// it is refused.
fn from_applies(args: &CruiseArgs, output_type: &str) -> Result<(), RunError> {
    match args.from.as_deref() {
        Some(from) if output_type != "plantuml" => {
            Err(RunError::Config(rb_config::ConfigError::Invalid(format!(
                "--from {from} applies to --output-type plantuml, not {output_type}"
            ))))
        }
        _ => Ok(()),
    }
}

/// The configuration's warnings as `warning:` lines, each naming its rule when it has one.
fn config_warnings(config: &Config) -> String {
    let mut out = String::new();
    for warning in &config.warnings {
        let rule = warning
            .rule
            .as_deref()
            .map(|r| format!("rule `{r}`: "))
            .unwrap_or_default();
        let _ = writeln!(out, "warning: {rule}{}", warning.message);
    }
    out
}

/// Whether the rendered cache layer may serve and store `output_type`'s output. A plugin's
/// output is never served from, or stored in, it; nor is `x-dot-webpage`'s, which depends on the
/// GraphViz installed at the time of the run, not on the key (ADR-0053): without `dot` it must
/// exit 2, never serve an old page.
fn renders_cached(output_type: &str) -> bool {
    rb_config::js::plugin::plugin_name(output_type).is_none() && output_type != "x-dot-webpage"
}

/// An extraction a caller already made (`guard` keeps one in memory): the merged document and
/// the extractors' warnings, evaluated and reported as a cruise's own would be.
#[derive(Debug, Clone)]
pub struct Given {
    /// The merged document.
    pub document: rb_model::GraphDocument,
    /// The extractors' warnings.
    pub warnings: Vec<rb_model::Warning>,
}

/// What `cruise --from-hook` answers for an extraction the caller made, without reading stdin
/// or the guard's findings: the answer the Stop hook would give, exit 0
/// ([Wave 3, Step 16](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)).
pub fn hook_answer(ctx: &mut Context<'_>, args: &CruiseArgs, given: Given) -> Outcome {
    let mut args = args.clone();
    args.from_hook = true;
    let outcome = cruise_with(ctx, &args, Some(given));
    Outcome { code: 0, ..outcome }
}

fn cruise(ctx: &mut Context<'_>, args: &CruiseArgs) -> Outcome {
    cruise_with(ctx, args, None)
}

fn cruise_with(ctx: &mut Context<'_>, args: &CruiseArgs, given: Option<Given>) -> Outcome {
    if args.info {
        return Outcome {
            stdout: info(),
            stderr: String::new(),
            code: 0,
        };
    }
    if let Some(oneshot) = &args.init {
        return crate::cmd::init::oneshot(ctx, oneshot, args);
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
    let affected = match affected(ctx, args, &mut effective) {
        Ok(selection) => selection,
        Err(outcome) => return *outcome,
    };
    progress.stage("configuration");
    let stderr = config_warnings(&effective);
    let (output_type, output_to) = outputs(args, &effective);
    if let Err(e) = from_applies(args, &output_type) {
        return failed(&e, &stderr);
    }
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
        affected,
    };
    let (cache, evaluation) = if given.is_some() {
        (None, None)
    } else {
        cache_of(ctx, &effective, args, &options, liveness)
    };
    let report_options = report_options(ctx, &effective, args, &output_type, &output_to);
    let report_part = report_key(&output_type, &output_to, &report_options);
    let renders_cached = renders_cached(&output_type);
    let mode = Mode {
        cache: cache.as_ref(),
        evaluation: evaluation.as_deref(),
        report: renders_cached.then_some(report_part.as_str()),
    };
    let produced = match given {
        Some(given) => evaluate_given(ctx, &effective, given, &options, &mut progress),
        None => produce(ctx, &effective, args, &options, mode, &mut progress),
    };
    let (produced, writing) = match produced {
        Ok(produced) => produced,
        Err(Produced::Graph(message)) => {
            return Outcome::failed(
                RunExit::Untrustworthy,
                format!("{stderr}rulebearing cruise: {message}\n"),
            );
        }
        Err(Produced::Run(e)) => return failed(&e, &stderr),
    };
    let finishing = Finishing {
        args,
        liveness,
        output_type: &output_type,
        output_to: &output_to,
    };
    let reporting = Reporting {
        cache: cache.as_ref(),
        evaluation,
        options: &report_options,
        part: renders_cached.then_some(report_part.as_str()),
    };
    finish(
        ctx,
        &effective,
        &options,
        produced,
        writing,
        finishing,
        &reporting,
        (progress, stderr),
    )
}

/// The report settings [`finish`] needs besides the command line.
struct Reporting<'a> {
    cache: Option<&'a rb_model::CacheOptions>,
    evaluation: Option<String>,
    options: &'a ReportOptions,
    /// The report's part of the rendered layer's key; none when the layer is off for this report.
    part: Option<&'a str>,
}

/// Reports what [`produce`] made, stores what the next run can reuse, and waits for the cache
/// writer.
#[expect(
    clippy::too_many_arguments,
    reason = "the last step reads every part of the run once"
)]
fn finish(
    ctx: &Context<'_>,
    effective: &Config,
    options: &RunOptions,
    produced: Evaluated,
    writing: crate::cache::Writing,
    finishing: Finishing<'_>,
    reporting: &Reporting<'_>,
    (progress, stderr): (Progress, String),
) -> Outcome {
    let Finishing {
        liveness,
        output_type,
        ..
    } = finishing;
    let (cache, report_options, report_part) = (reporting.cache, reporting.options, reporting.part);
    let evaluation = reporting.evaluation.clone();
    let (mut outcome, writing) = match produced {
        Evaluated::Rendered(output, tail) => (
            conclude(
                ctx,
                effective,
                &tail,
                (&output, None),
                finishing,
                progress,
                stderr,
            ),
            writing,
        ),
        Evaluated::Stored(verdict, key) => {
            let tail = verdict.tail();
            match render(ctx, &verdict, output_type, report_options) {
                Ok((output, count)) => {
                    let outcome = conclude(
                        ctx,
                        effective,
                        &tail,
                        (&output, count),
                        finishing,
                        progress,
                        stderr,
                    );
                    // The rendered output is kept for the next run with the same report.
                    let writing = match (cache, report_part) {
                        (Some(cache), Some(report_part)) => writing.then_render(
                            ctx.resolve(&cache.folder),
                            crate::cache::evaluated::render_key(&key, report_part),
                            output,
                            tail,
                        ),
                        _ => writing,
                    };
                    (outcome, writing)
                }
                Err(e) => (failed(&e, &warned(&stderr, &tail)), writing),
            }
        }
        Evaluated::Run(mut run) => {
            let ratchets =
                ratchets::evaluate(ctx, effective, &run.evaluation.document, options.liveness);
            summarise(&mut run.document.summary, &ratchets, liveness);
            let verdict = Verdict::of(*run, ratchets);
            let tail = verdict.tail();
            let outcome = match render(ctx, &verdict, output_type, report_options) {
                Ok((output, count)) => conclude(
                    ctx,
                    effective,
                    &tail,
                    (&output, count),
                    finishing,
                    progress,
                    stderr,
                ),
                Err(e) => failed(&e, &warned(&stderr, &tail)),
            };
            // The evaluated run is stored for the next one once the extraction it came from is.
            let writing = match (cache, evaluation) {
                (Some(cache), Some(partial)) => writing.then_remember(
                    ctx.resolve(&cache.folder),
                    partial,
                    verdict,
                    cache.compressed(),
                ),
                _ => writing,
            };
            (outcome, writing)
        }
    };
    if let Err(reason) = writing.wait() {
        let _ = writeln!(
            outcome.stderr,
            "warning: the cache was not written: {reason}"
        );
    }
    outcome
}

/// The cache options of a run, none over `--graph FILE`, and the evaluated layer's key without
/// the extraction: none when the layer is off for this run.
fn cache_of(
    ctx: &Context<'_>,
    config: &Config,
    args: &CruiseArgs,
    options: &RunOptions,
    liveness: Liveness,
) -> (Option<rb_model::CacheOptions>, Option<String>) {
    let cache = config
        .options
        .cache
        .as_ref()
        .and_then(rb_config::model::CacheSetting::options)
        .filter(|_| args.graph.is_none())
        .cloned();
    let evaluation = cache.as_ref().and_then(|_| {
        crate::cache::evaluated::partial_key(ctx, config, options, &format!("{liveness:?}"))
    });
    (cache, evaluation)
}

/// Why [`produce`] made no run.
enum Produced {
    /// `--graph FILE` cannot be read.
    Graph(String),
    /// The run stopped.
    Run(RunError),
}

/// What [`produce`] made.
enum Evaluated {
    /// A run evaluated now.
    Run(Box<pipeline::Run>),
    /// The evaluated run the cache held, and its key.
    Stored(Box<Verdict>, String),
    /// The rendered output the cache held for this report of that run, and its tail.
    Rendered(String, Box<Tail>),
}

/// The cache settings of a run: the options, the evaluated layer's partial key, and the report's
/// part of the rendered layer's key (none for a `plugin:` or `x-dot-webpage` report, which that
/// layer never serves).
#[derive(Clone, Copy)]
struct Mode<'a> {
    cache: Option<&'a rb_model::CacheOptions>,
    evaluation: Option<&'a str>,
    report: Option<&'a str>,
}

/// The evaluated run: over `--graph FILE`, through the cache when `--cache` or `options.cache`
/// asks, else extracted afresh; with the cache entry still being written, when there is one.
fn produce(
    ctx: &Context<'_>,
    config: &Config,
    args: &CruiseArgs,
    options: &RunOptions,
    mode: Mode<'_>,
    progress: &mut Progress,
) -> Result<(Evaluated, crate::cache::Writing), Produced> {
    if let Some(file) = &args.graph {
        let mut document = pipeline::load_graph(ctx, file).map_err(Produced::Graph)?;
        pipeline::reset(&mut document);
        progress.stage("read graph");
        return pipeline::evaluate_document(ctx, config, document, options, progress)
            .map(|run| {
                (
                    Evaluated::Run(Box::new(run)),
                    crate::cache::Writing::default(),
                )
            })
            .map_err(Produced::Run);
    }
    match mode.cache {
        Some(cache) => cached(ctx, config, cache, options, mode, progress).map_err(Produced::Run),
        None => pipeline::run(ctx, config, options, progress)
            .map(|run| {
                (
                    Evaluated::Run(Box::new(run)),
                    crate::cache::Writing::default(),
                )
            })
            .map_err(Produced::Run),
    }
}

/// The evaluated run of an extraction the caller made ([`Given`]); nothing is cached.
fn evaluate_given(
    ctx: &Context<'_>,
    config: &Config,
    given: Given,
    options: &RunOptions,
    progress: &mut Progress,
) -> Result<(Evaluated, crate::cache::Writing), Produced> {
    pipeline::run_extracted(
        ctx,
        config,
        (given.document, given.warnings),
        options,
        progress,
    )
    .map(|run| {
        (
            Evaluated::Run(Box::new(run)),
            crate::cache::Writing::default(),
        )
    })
    .map_err(Produced::Run)
}

/// The run through the `--cache` entry ([`crate::cache::extract_cached`]): the rendered output
/// when the cache holds it for this report of this run, else the evaluated run when the cache
/// holds it for this extraction and this evaluation, else the extraction from the cache where it
/// is fresh, evaluated as a cold run's is; `summary.cache` recorded; and the entry being written
/// for the next run, which the caller waits for. A stored verdict that cannot be read after all
/// sends the run back through the extraction. A cache that cannot be written is a warning; the
/// run is not less trustworthy for it.
fn cached(
    ctx: &Context<'_>,
    config: &Config,
    cache: &rb_model::CacheOptions,
    options: &RunOptions,
    mode: Mode<'_>,
    progress: &mut Progress,
) -> Result<(Evaluated, crate::cache::Writing), RunError> {
    use crate::cache::evaluated;
    let mut extracted =
        crate::cache::extract_cached(ctx, config, &options.paths, cache, mode.evaluation)?;
    progress.stage(&format!("extract ({})", extracted.served));
    if let Content::Evaluated(stored) = &extracted.content {
        let rendered = mode
            .report
            .map(|report| evaluated::render_key(&stored.key, report))
            .and_then(|key| evaluated::load_rendered(&stored.folder, &key).ok());
        if let Some((output, tail)) = rendered {
            progress.stage("evaluate (from the cache)");
            return Ok((
                Evaluated::Rendered(output, Box::new(tail)),
                extracted.writing,
            ));
        }
        if let Ok(mut verdict) = evaluated::load(&stored.folder, &stored.key, stored.compressed) {
            progress.stage("evaluate (from the cache)");
            verdict.document.summary.cache = Some(extracted.summary);
            return Ok((
                Evaluated::Stored(Box::new(verdict), stored.key.clone()),
                extracted.writing,
            ));
        }
        // The verdict named is unusable: extract as if it had never been stored.
        drop(extracted.writing);
        extracted = crate::cache::extract_cached(ctx, config, &options.paths, cache, None)?;
    }
    match extracted.content {
        Content::Extracted(document, warnings) => {
            let mut run =
                pipeline::run_extracted(ctx, config, (*document, warnings), options, progress)?;
            run.document.summary.cache = Some(extracted.summary);
            Ok((Evaluated::Run(Box::new(run)), extracted.writing))
        }
        Content::Evaluated(_) => Err(RunError::Extract(rb_model::ExtractError::NoModulesFound)),
    }
}

/// The report options of a run.
fn report_options(
    ctx: &Context<'_>,
    config: &Config,
    args: &CruiseArgs,
    output_type: &str,
    output_to: &str,
) -> ReportOptions {
    ReportOptions {
        color: color(args.color, ctx.color_terminal) && output_to == "-",
        strict_schema: args.strict_schema,
        max_findings: args.max_findings,
        timestamp: ctx.timestamp.clone(),
        path_prefix: if matches!(output_type, "github-annotations" | "sarif") {
            ctx.repository_prefix()
        } else {
            String::new()
        },
        baseline: rb_report::baseline::Lifecycle::default(),
        // The same collapse the cruise applied to its modules (pipeline.rs), as upstream passes it.
        collapse_pattern: crate::pipeline::cruise_collapse(config),
        plantuml_from: args.from.clone(),
        graphviz: Some(crate::graphviz::system()),
    }
}

/// The report's part of the rendered layer's key: the output type and destination and every
/// report option, the timestamp only for a reporter that prints it.
fn report_key(output_type: &str, output_to: &str, options: &ReportOptions) -> String {
    let mut keyed = options.clone();
    if !rb_report::stamps(output_type) {
        keyed.timestamp.clear();
    }
    format!("{output_type}\n{output_to}\n{keyed:?}")
}

/// The reporter's output for `verdict`, and for a `plugin:` reporter the count it returned
/// ([`crate::plugin`]).
fn render(
    ctx: &Context<'_>,
    verdict: &Verdict,
    output_type: &str,
    options: &ReportOptions,
) -> Result<(String, Option<u64>), RunError> {
    let mut value =
        crate::value::document(&verdict.document).map_err(|e| RunError::Engine(e.into()))?;
    if let Some(name) = rb_config::js::plugin::plugin_name(output_type) {
        let rendered = crate::plugin::render(&ctx.cwd, name, &mut value, options.strict_schema)?;
        crate::value::release(value);
        return Ok((rendered.output, Some(rendered.exit_code)));
    }
    let rendered = rb_report::render(output_type, &value, options);
    crate::value::release(value);
    rendered
        .map(|rendered| (rendered.output, None))
        .map_err(|e| match e {
            // `x-dot-webpage` without a working `dot`: the report could not be made (ADR-0053).
            rb_report::ReportError::Graphviz(message) => RunError::Report(message),
            e => RunError::Config(rb_config::ConfigError::Invalid(e.to_string())),
        })
}

/// `stderr` with the extractors' warnings after it, as a failed report prints them.
fn warned(stderr: &str, tail: &Tail) -> String {
    let mut out = stderr.to_owned();
    for warning in &tail.warnings {
        let _ = writeln!(out, "warning: {warning}");
    }
    out
}

/// What [`conclude`] needs of the command line.
#[derive(Clone, Copy)]
struct Finishing<'a> {
    args: &'a CruiseArgs,
    liveness: Liveness,
    output_type: &'a str,
    output_to: &'a str,
}

/// The finished run from a rendered output (with a plugin's count, when a plugin rendered it) and
/// the verdict's tail: the output written, the warnings and the messages on stderr, and the exit
/// code.
fn conclude(
    ctx: &Context<'_>,
    config: &Config,
    tail: &Tail,
    (output, plugin_count): (&str, Option<u64>),
    finishing: Finishing<'_>,
    mut progress: Progress,
    mut stderr: String,
) -> Outcome {
    let Finishing {
        args,
        liveness,
        output_type,
        output_to,
    } = finishing;
    for warning in &tail.warnings {
        let _ = writeln!(stderr, "warning: {warning}");
    }
    // Output without the approximate marks could later gate through fmt, diff or attest
    // (ADR-0011), so it is not written.
    if strips_approximate(
        output_type,
        args.strict_schema,
        tail.approximate,
        args.allow_approximate_gate,
    ) {
        let _ = writeln!(stderr, "rulebearing cruise: {APPROXIMATE_STRICT_REASON}");
        return Outcome {
            stdout: String::new(),
            stderr,
            code: RunExit::Untrustworthy.code(),
        };
    }
    progress.stage("report");
    let mut stdout = String::new();
    if let Err(message) = write_output(ctx, output_to, output, &mut stdout) {
        let _ = writeln!(stderr, "rulebearing cruise: {message}");
        return Outcome {
            stdout,
            stderr,
            code: RunExit::Untrustworthy.code(),
        };
    }
    stderr.insert_str(0, &progress.finish());
    for expired in &tail.expired {
        let _ = writeln!(
            stderr,
            "error: {} `{}` expired on {}; it no longer applies and the run fails",
            expired.kind, expired.name, expired.expires
        );
    }
    let strict = liveness == Liveness::Strict;
    let ratchets = &tail.ratchets;
    stderr.push_str(&ratchets::messages(config, ratchets, strict));
    for v in &tail.vacuous {
        let _ = writeln!(stderr, "{}", vacuous_message(&v.name, &v.side, strict));
    }
    let vacuous = !tail.vacuous.is_empty() || !ratchets.vacuous.is_empty();
    // 2 whatever the reporter; the error count only for a reporter that gates (ADR-0030). A rule
    // that matches nothing counts under strict liveness only (ADR-0032). The count is the
    // report's, after `reaches` and `--affected` kept their modules, as upstream's reporter
    // counts the re-summarised result.
    let decides = rb_report::gates(output_type) || plugin_count.is_some();
    let mut code = if (strict && vacuous) || ratchets.no_budget() {
        RunExit::Untrustworthy
    } else if rb_report::gates(output_type) {
        RunExit::Violations(tail.error + tail.expired.len() as u64 + ratchets.exceeded())
    } else if let Some(count) = plugin_count {
        // A plugin decides its own count, as upstream's command line exits with it (ADR-0030).
        RunExit::Violations(count)
    } else {
        RunExit::Violations(0)
    };
    if args.from_hook {
        return stop_hook(code, &stdout, stderr);
    }
    // Source mode is the inner loop's, never the gate's (ADR-0011): the hook above answers with
    // it, and any other run whose count would be the exit code is refused.
    if let Some(refused) = gate(code, decides, tail.approximate, args.allow_approximate_gate) {
        let _ = writeln!(stderr, "warning: {APPROXIMATE_REASON}");
        code = refused;
    }
    Outcome {
        stdout,
        stderr,
        code: code.code_in(args.exit_code_mode),
    }
}

/// The reporter and where it writes: the flags, else the configuration, else `err` to stdout;
/// always `agent` to stdout for `--from-hook`.
fn outputs(args: &CruiseArgs, config: &Config) -> (String, String) {
    if args.from_hook {
        return ("agent".to_owned(), "-".to_owned());
    }
    (
        args.output_type
            .clone()
            .or_else(|| config.options.output_type.clone())
            .unwrap_or_else(|| "err".into()),
        args.output_to
            .clone()
            .or_else(|| config.options.output_to.clone())
            .unwrap_or_else(|| "-".into()),
    )
}

/// `--affected [revision]` (or a native configuration's `options.affected`): the changes since
/// the revision, with `reaches` set to dependency-cruiser's expression for them
/// ([`crate::affected`]). A revision git does not know, or a directory outside a repository,
/// exits 2; `--affected-depth` without either exits 3.
fn affected(
    ctx: &Context<'_>,
    args: &CruiseArgs,
    config: &mut Config,
) -> Result<Option<crate::affected::Selection>, Box<Outcome>> {
    let Some(request) =
        crate::affected::request(args.affected.as_deref(), args.affected_depth, config)
    else {
        return match args.affected_depth {
            Some(_) => Err(Box::new(Outcome::failed(
                RunExit::InvalidConfig,
                "rulebearing cruise: --affected-depth needs --affected [revision], or options.affected in a rulebearing.* configuration\n",
            ))),
            None => Ok(None),
        };
    };
    let saved = ctx.resolve(pipeline::SAVED_GRAPH);
    crate::affected::select(&ctx.cwd, &saved, request, config)
        .map(Some)
        .map_err(|e| {
            Box::new(Outcome::failed(
                RunExit::Untrustworthy,
                format!("rulebearing cruise: {e}\n"),
            ))
        })
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
    // A `graph.ignore` entry that removes no edge excuses nothing any more (ADR-0038).
    if side.starts_with("graph.ignore[") {
        let level = if strict { "error" } else { "warning" };
        return format!(
            "{level}: rule `{name}`: its {side} entry matches no import, so it removes nothing. Fix or delete the entry, or excuse it with allowEmpty (ADR-0038)"
        );
    }
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

#[cfg(test)]
mod tests {
    use super::{config_warnings, renders_cached, vacuous_message};

    #[test]
    fn only_reporters_that_depend_on_the_key_alone_are_served_rendered() {
        for (output_type, cached) in [
            ("err", true),
            ("json", true),
            ("dot", true),
            ("plantuml", true),
            ("x-dot-webpage", false),
            ("plugin:./p.cjs", false),
            ("plugin:file:///repo/p.cjs", false),
        ] {
            assert_eq!(renders_cached(output_type), cached, "{output_type}");
        }
    }

    #[test]
    fn config_warnings_name_their_rule() {
        let mut config = rb_config::Config::default();
        assert_eq!(config_warnings(&config), "");
        config.warnings = vec![
            rb_config::ConfigWarning {
                rule: Some("r".into()),
                message: "one".into(),
            },
            rb_config::ConfigWarning {
                rule: None,
                message: "two".into(),
            },
        ];
        assert_eq!(
            config_warnings(&config),
            "warning: rule `r`: one\nwarning: two\n"
        );
    }

    #[test]
    fn a_stale_ignore_entry_is_named_as_one() {
        assert_eq!(
            vacuous_message("r", "graph.ignore[2]", true),
            "error: rule `r`: its graph.ignore[2] entry matches no import, so it removes nothing. Fix or delete the entry, or excuse it with allowEmpty (ADR-0038)"
        );
        assert!(
            vacuous_message("r", "graph.ignore[0]", false)
                .starts_with("warning: rule `r`: its graph.ignore[0] entry")
        );
        assert!(vacuous_message("r", "from", true).contains("its from side matched no module"));
        assert!(vacuous_message("r", "from", false).starts_with("warning: rule `r` is vacuous"));
    }
}
