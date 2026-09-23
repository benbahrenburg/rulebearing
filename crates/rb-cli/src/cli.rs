//! The command-line surface: every subcommand and flag, declared once with `clap`.
//!
//! - Coverage: [coverage § Command line](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line)
//!   (dependency-cruiser's flags, same names and short forms)
//! - Source: [design § The subcommands a guard reaches for](../../../docs/artifacts/design.md#the-subcommands-a-guard-reaches-for)
//! - Plan: [Wave 1, Step 13](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-13-rb-cli-cruise-fmt-exit-codes-flags-1d)
//! - Requirements: [FR-CLI-01](../../../docs/prd.md#fr-cli-01), [FR-CLI-08](../../../docs/prd.md#fr-cli-08)
//!
//! Flag names and short forms are dependency-cruiser's (`-T`, `-f`, `-I`, `-F`, `-R`, `-x`, `-X`,
//! `-P`, `-p`, `-m`, `-i` on `cruise`; `-T`, `-f`, `-I`, `-F`, `-R`, `-H`, `-x`, `-S`, `-e`, `-p`
//! on `fmt`). The help text is a committed snapshot (`tests/help.txt`), because flag parity is a
//! promise ([ADR-0024](../../../docs/adr/0024-test-quality-gates.md)).

use clap::{Args, Parser, Subcommand, ValueEnum};

/// The exit-code table, printed under every help text
/// ([ADR-0008](../../../docs/adr/0008-exit-code-contract.md)).
pub const EXIT_CODES: &str = "Exit codes:
  0       no error-severity violation
  1-255   the number of error-severity violations, capped at 255
  2       the run cannot be trusted: zero modules, an unsupported file, a vacuous rule
  3       the configuration is invalid
A run with exactly 2 or 3 error violations also exits 2 or 3; the report says which it was.";

/// `rulebearing`: one architecture rule set for TypeScript, .NET and Python.
#[derive(Debug, Parser)]
#[command(
    name = "rulebearing",
    version,
    about = "rulebearing: one architecture rule set for TypeScript, .NET and Python",
    disable_help_subcommand = true,
    after_help = EXIT_CODES
)]
pub struct Cli {
    /// The subcommand.
    #[command(subcommand)]
    pub command: Command,
}

/// Every wave 1 subcommand.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Extract, evaluate and report: dependency-cruiser's `depcruise`.
    Cruise(CruiseArgs),
    /// Re-report a saved result without extracting: dependency-cruiser's `depcruise-fmt`.
    Fmt(FmtArgs),
    /// Every rule, its family, severity and statistics
    Rules(crate::cmd::rules::RulesArgs),
    /// One rule: its sentence, its violations and the edges it matched
    Explain(crate::cmd::explain::ExplainArgs),
    /// Run each rule's examples against a synthetic graph
    Test(crate::cmd::test_rules::TestArgs),
    /// Would this import be allowed? Answers from the graph, before the import is written
    CanImport(crate::cmd::can_import::CanImportArgs),
    /// The edges a ratchet counts, against its budget
    Count(crate::cmd::count::CountArgs),
    /// Convert, expand or lint a configuration
    #[command(subcommand)]
    Config(crate::cmd::config::ConfigCommand),
    /// Install agent hooks
    #[command(subcommand)]
    Hooks(crate::cmd::hooks::HooksCommand),
    /// A brief for an agent starting a session
    Summary(crate::cmd::summary::SummaryArgs),
    /// What a file is subject to, before an edit
    Impact(crate::cmd::impact::ImpactArgs),
    /// Write or verify a receipt of the configuration, inputs and results
    Attest(crate::cmd::attest::AttestArgs),
    /// Conformance gate 1 layer 2's protocol (hidden).
    #[command(hide = true)]
    Validate(ProtocolArgs),
    /// Conformance gate 1 layer 3's protocol (hidden).
    #[command(hide = true)]
    Report(ProtocolArgs),
}

/// Arguments the protocols accept and ignore, so the harness's command lines parse.
#[derive(Debug, Args)]
pub struct ProtocolArgs {
    /// Ignored.
    #[arg(long, hide = true)]
    pub rules: Option<String>,
    /// Ignored.
    #[arg(long, hide = true)]
    pub module: Option<String>,
    /// Ignored.
    #[arg(long, hide = true)]
    pub no_liveness: bool,
    /// The reporter (`report` only).
    #[arg(short = 'T', long)]
    pub output_type: Option<String>,
}

/// `--progress` types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ProgressType {
    /// Stage names on stderr as they start.
    CliFeedback,
    /// Stage timings and memory on stderr.
    PerformanceLog,
    /// One JSON object per stage on stderr.
    Ndjson,
    /// Nothing.
    None,
}

/// When to colour terminal output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum ColorChoice {
    /// When stdout is a terminal and `NO_COLOR` is not set.
    #[default]
    Auto,
    /// Always.
    Always,
    /// Never.
    Never,
}

/// The configuration flags `cruise` and the agent commands share.
#[derive(Debug, Clone, Default, Args)]
#[allow(clippy::struct_excessive_bools)] // each is a command-line flag; dependency-cruiser names them
pub struct ConfigArgs {
    /// Read rules and options from FILE (`-` for stdin); without a value, look for
    /// rulebearing.{yaml,yml,json,jsonc,toml} or .dependency-cruiser.{js,cjs,mjs,json,yaml,yml}
    #[arg(short = 'c', long, value_name = "FILE", num_args = 0..=1, default_missing_value = "")]
    pub config: Option<String>,
    /// dependency-cruiser's older name for --config
    #[arg(long, value_name = "FILE", num_args = 0..=1, default_missing_value = "", hide = true)]
    pub validate: Option<String>,
    /// Run without a configuration, even when one is present
    #[arg(long)]
    pub no_config: bool,
    /// The configuration format when it cannot be told from the name: native or dependency-cruiser
    #[arg(long, value_name = "FORMAT")]
    pub config_format: Option<String>,
    /// Evaluate a JavaScript configuration with the local Node instead of the sandbox
    #[arg(long)]
    pub config_via_node: bool,
    /// Refuse what dependency-cruiser would refuse (nested quantifiers, Rulebearing additions)
    #[arg(long)]
    pub strict_compat: bool,
    /// Require a decision token (`adr:NNNN` or `plan:<slug>`) in every rule's comment
    #[arg(long)]
    pub require_comment_token: bool,
}

/// Where a query command finds the graph.
#[derive(Debug, Clone, Default, Args)]
pub struct GraphArgs {
    /// A saved cruise result to query (default: .graph/cruise.json when it exists, else a fresh
    /// extraction of the paths)
    #[arg(long, value_name = "FILE")]
    pub graph: Option<String>,
    /// Files, directories and globs to extract when there is no saved graph
    #[arg(value_name = "FILES-OR-DIRECTORIES")]
    pub paths: Vec<String>,
}

/// `cruise`.
#[derive(Debug, Clone, Default, Args)]
#[command(after_help = EXIT_CODES)]
#[allow(clippy::struct_excessive_bools)] // each is one of dependency-cruiser's flags, a parity promise
pub struct CruiseArgs {
    /// Files, directories and globs to cruise
    #[arg(value_name = "FILES-OR-DIRECTORIES")]
    pub paths: Vec<String>,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
    /// Evaluate the rules over this graph document instead of extracting (a cruise result, or a
    /// graph built by another tool, such as scripts/cargo-graph.sh for this repository)
    #[arg(long, value_name = "FILE")]
    pub graph: Option<String>,
    /// Output type: err, err-long, json, text, csv, teamcity, azure-devops, github-annotations, agent, null
    #[arg(short = 'T', long, value_name = "TYPE")]
    pub output_type: Option<String>,
    /// File to write output to; - for stdout
    #[arg(short = 'f', long, value_name = "FILE")]
    pub output_to: Option<String>,
    /// Calculate stability metrics (instability, folders)
    #[arg(short = 'm', long)]
    pub metrics: bool,
    /// Only include modules matching the regex
    #[arg(short = 'I', long, value_name = "REGEX")]
    pub include_only: Option<String>,
    /// Only include modules matching the regex and their neighbours
    #[arg(short = 'F', long, value_name = "REGEX")]
    pub focus: Option<String>,
    /// The depth to focus on: 1 direct neighbours, 2 their neighbours, 0 everything
    #[arg(long, value_name = "NUMBER")]
    pub focus_depth: Option<u32>,
    /// Only include modules matching the regex and every module that reaches them
    #[arg(short = 'R', long, value_name = "REGEX")]
    pub reaches: Option<String>,
    /// Exclude all modules matching the regex
    #[arg(short = 'x', long, value_name = "REGEX")]
    pub exclude: Option<String>,
    /// Include modules matching the regex but do not follow their dependencies
    #[arg(short = 'X', long, value_name = "REGEX")]
    pub do_not_follow: Option<String>,
    /// How deep to follow dependencies from the roots; 0 for no limit
    #[arg(long, value_name = "NUMBER")]
    pub max_depth: Option<u8>,
    /// Module systems to extract, comma separated: cjs, es6, amd, tsd
    #[arg(long, value_name = "LIST")]
    pub module_systems: Option<String>,
    /// Prefix for links in the reports
    #[arg(short = 'P', long, value_name = "PREFIX")]
    pub prefix: Option<String>,
    /// Suffix for links in the reports
    #[arg(long, value_name = "SUFFIX")]
    pub suffix: Option<String>,
    /// Keep TypeScript edges that vanish in compilation: true, false or specify
    #[arg(long, value_name = "VALUE", num_args = 0..=1, default_missing_value = "true")]
    pub ts_pre_compilation_deps: Option<String>,
    /// The tsconfig to resolve paths and baseUrl from
    #[arg(long, value_name = "FILE", num_args = 0..=1, default_missing_value = "tsconfig.json")]
    pub ts_config: Option<String>,
    /// Keep symlinked paths rather than their targets
    #[arg(long)]
    pub preserve_symlinks: bool,
    /// A webpack `resolve` block already evaluated to JSON (its consumer arrives in wave 2)
    #[arg(long, value_name = "FILE")]
    pub webpack_config_json: Option<String>,
    /// Show progress on stderr
    #[arg(short = 'p', long, value_name = "TYPE", num_args = 0..=1, default_missing_value = "cli-feedback")]
    pub progress: Option<ProgressType>,
    /// Show no progress
    #[arg(long)]
    pub no_progress: bool,
    /// Show the languages, extensions and parsers this build supports
    #[arg(short = 'i', long)]
    pub info: bool,
    /// Do not fail a run whose rules match nothing (ADR-0007)
    #[arg(long)]
    pub no_liveness: bool,
    /// json: strip every Rulebearing addition so the output validates against cruise-result 18.2.0
    #[arg(long)]
    pub strict_schema: bool,
    /// agent: the most violations shown per rule
    #[arg(long, value_name = "N")]
    pub max_findings: Option<usize>,
    /// Colour terminal output
    #[arg(long, value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,
}

/// `fmt`.
#[derive(Debug, Clone, Default, Args)]
#[command(after_help = EXIT_CODES)]
pub struct FmtArgs {
    /// The result to re-report; - for stdin
    #[arg(value_name = "RESULT-JSON")]
    pub input: String,
    /// Output type
    #[arg(short = 'T', long, value_name = "TYPE", default_value = "err")]
    pub output_type: String,
    /// File to write output to; - for stdout
    #[arg(short = 'f', long, value_name = "FILE", default_value = "-")]
    pub output_to: String,
    /// Only include modules matching the regex
    #[arg(short = 'I', long, value_name = "REGEX")]
    pub include_only: Option<String>,
    /// Only include modules matching the regex and their neighbours
    #[arg(short = 'F', long, value_name = "REGEX")]
    pub focus: Option<String>,
    /// The depth to focus on
    #[arg(long, value_name = "NUMBER")]
    pub focus_depth: Option<u32>,
    /// Only include modules matching the regex and every module that reaches them
    #[arg(short = 'R', long, value_name = "REGEX")]
    pub reaches: Option<String>,
    /// Mark modules matching the regex as highlighted
    #[arg(short = 'H', long, value_name = "REGEX")]
    pub highlight: Option<String>,
    /// Exclude all modules matching the regex
    #[arg(short = 'x', long, value_name = "REGEX")]
    pub exclude: Option<String>,
    /// Collapse modules to a folder depth (a single digit) or to the first match of a regex
    #[arg(short = 'S', long, value_name = "REGEX-OR-DEPTH")]
    pub collapse: Option<String>,
    /// Exit with the number of error violations
    #[arg(short = 'e', long)]
    pub exit_code: bool,
    /// Prefix for links in the reports
    #[arg(short = 'p', long, value_name = "PREFIX")]
    pub prefix: Option<String>,
    /// Where the result came from: rulebearing or dependency-cruiser
    #[arg(long, value_name = "TOOL")]
    pub from: Option<String>,
    /// json: strip every Rulebearing addition
    #[arg(long)]
    pub strict_schema: bool,
    /// agent: the most violations shown per rule
    #[arg(long, value_name = "N")]
    pub max_findings: Option<usize>,
    /// Colour terminal output
    #[arg(long, value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,
}
