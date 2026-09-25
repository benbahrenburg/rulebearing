//! `rulebearing config convert | expand | lint`.
//!
//! - Source: [design § The native format](../../../../docs/artifacts/design.md#the-native-format),
//!   [design § Shorthands](../../../../docs/artifacts/design.md#shorthands),
//!   [design § Rules an agent writes](../../../../docs/artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar)
//! - Decision: [ADR-0005](../../../../docs/adr/0005-native-config-superset-and-compat.md)
//! - Plan: [Wave 1, Step 4](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-4-config-convert-config-expand-config-lint-shorthands-1a)
//! - Requirement: [FR-CFG-05](../../../../docs/prd.md#fr-cfg-05)
//!
//! `convert` and `expand` work on the file as written (a JavaScript file is evaluated first);
//! `lint` works on the loaded configuration and, with `--graph`, on the graph as well.

use clap::{Args, Subcommand, ValueEnum};
use rb_config::convert;
use rb_config::lint::{LintOptions, lint, render};
use rb_config::load::repository_root;
use rb_config::read::{self, Evaluation, Syntax};
use rb_config::{ConfigFormat, native};
use serde_json::{Map, Value};

use crate::cli::ConfigArgs;
use crate::context::Context;
use crate::{Outcome, RunExit, configure, pipeline};

/// `config`.
#[derive(Debug, Clone, Subcommand)]
pub enum ConfigCommand {
    /// Translate between the formats: dependency-cruiser to native is lossless, native to
    /// dependency-cruiser says what it dropped
    Convert(ConvertArgs),
    /// Print a native file with defines substituted and shorthands expanded
    Expand(FileArgs),
    /// Report rules that can never match, shadowed or overlapping rules, missing fixes
    Lint(LintArgs),
}

/// The output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Target {
    /// `rulebearing.yaml`.
    #[default]
    Native,
    /// `.dependency-cruiser.json`.
    DependencyCruiser,
}

/// `config convert`.
#[derive(Debug, Clone, Args)]
pub struct ConvertArgs {
    /// The configuration file
    pub file: String,
    /// The format to write
    #[arg(long, value_enum, default_value_t = Target::Native)]
    pub to: Target,
    /// Write JSON rather than YAML (native output only)
    #[arg(long)]
    pub json: bool,
    /// Evaluate a JavaScript file with Node
    #[arg(long)]
    pub config_via_node: bool,
}

/// `config expand`.
#[derive(Debug, Clone, Args)]
pub struct FileArgs {
    /// The configuration file
    pub file: String,
    /// Write JSON rather than YAML
    #[arg(long)]
    pub json: bool,
}

/// `config lint`.
#[derive(Debug, Clone, Default, Args)]
pub struct LintArgs {
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
    /// A saved graph, for the findings that need one (a rule that matches nothing, a severity
    /// below error with no violations)
    #[arg(long, value_name = "FILE")]
    pub graph: Option<String>,
}

fn failed(error: impl std::fmt::Display) -> Outcome {
    Outcome::failed(
        RunExit::InvalidConfig,
        format!("rulebearing config: {error}\n"),
    )
}

/// Reads a file as written, JavaScript evaluated.
fn read_raw(
    ctx: &Context<'_>,
    file: &str,
    via_node: bool,
) -> Result<(Map<String, Value>, std::path::PathBuf), Outcome> {
    let path = ctx.resolve(file);
    let syntax = Syntax::of(&path).unwrap_or(Syntax::JavaScript);
    let dir = path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    let evaluation = Evaluation {
        via_node,
        limits: rb_config::js::Limits::default(),
    };
    let read =
        read::read_file(&path, syntax, &repository_root(&dir), evaluation).map_err(failed)?;
    match read.value {
        Value::Object(map) => Ok((map, dir)),
        _ => Err(failed(format!("{file} is not an object"))),
    }
}

fn is_native(map: &Map<String, Value>, file: &str) -> bool {
    match ConfigFormat::detect(file) {
        Some(ConfigFormat::Native) => true,
        Some(ConfigFormat::DependencyCruiser) => false,
        None => native::looks_native(map),
    }
}

/// Runs `config`.
pub fn run(ctx: &mut Context<'_>, command: &ConfigCommand) -> Outcome {
    match command {
        ConfigCommand::Convert(args) => {
            let (map, dir) = match read_raw(ctx, &args.file, args.config_via_node) {
                Ok(v) => v,
                Err(o) => return o,
            };
            let native_input = is_native(&map, &args.file);
            match (args.to, native_input) {
                (Target::Native, false) => convert::render(&convert::to_native(&map), !args.json)
                    .map_or_else(failed, Outcome::printed),
                (Target::Native, true) => {
                    convert::render(&map, !args.json).map_or_else(failed, Outcome::printed)
                }
                (Target::DependencyCruiser, true) => {
                    match convert::to_dependency_cruiser(&map, &dir) {
                        Ok((dc, dropped)) => match convert::render(&dc, false) {
                            Ok(text) => Outcome {
                                stdout: text,
                                stderr: convert::describe(&dropped),
                                code: 0,
                            },
                            Err(e) => failed(e),
                        },
                        Err(e) => failed(e),
                    }
                }
                (Target::DependencyCruiser, false) => {
                    convert::render(&map, false).map_or_else(failed, Outcome::printed)
                }
            }
        }
        ConfigCommand::Expand(args) => {
            let (map, dir) = match read_raw(ctx, &args.file, false) {
                Ok(v) => v,
                Err(o) => return o,
            };
            let map = if is_native(&map, &args.file) {
                map
            } else {
                convert::to_native(&map)
            };
            match convert::expand(&map, &dir) {
                Ok(expanded) => {
                    convert::render(&expanded, !args.json).map_or_else(failed, Outcome::printed)
                }
                Err(e) => failed(e),
            }
        }
        ConfigCommand::Lint(args) => {
            // A missing token is a finding here, not a reason to refuse the configuration.
            let load = ConfigArgs {
                require_comment_token: false,
                ..args.config.clone()
            };
            let config = match configure::required(ctx, &load) {
                Ok(c) => c,
                Err(o) => return o,
            };
            let graph = match &args.graph {
                Some(file) => match pipeline::load_graph(ctx, file) {
                    Ok(g) => Some(g),
                    Err(m) => {
                        return Outcome::failed(
                            RunExit::Untrustworthy,
                            format!("rulebearing config lint: {m}\n"),
                        );
                    }
                },
                None => None,
            };
            let findings = lint(
                &config,
                graph.as_ref(),
                LintOptions {
                    require_comment_token: args.config.require_comment_token,
                },
            );
            Outcome {
                stdout: render(&findings),
                stderr: String::new(),
                code: RunExit::Violations(findings.len() as u64).code(),
            }
        }
    }
}
