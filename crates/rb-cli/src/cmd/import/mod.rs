//! `rulebearing import archunit | import-linter | eslint`: an existing rule set as a native
//! `rulebearing.yaml`.
//!
//! - Source: [design § The developer relations hat](../../../../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo)
//!   ("migration is a command, not a rewrite")
//! - Plan: [Wave 2, Step 11](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#211-step-11-the-three-importers-and-oracle-agreement-2f),
//!   and the decision rule "How `import archunit` reads C#" in [§ 1.7](../../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#17-decisions-this-wave-must-make)
//! - Decisions: [ADR-0005](../../../../../docs/adr/0005-native-config-superset-and-compat.md),
//!   [ADR-0006](../../../../../docs/adr/0006-embedded-quickjs-config-evaluator.md) (`ESLint` configurations
//!   are evaluated in the sandbox), [ADR-0013](../../../../../docs/adr/0013-ruff-parser-for-python.md)
//! - Requirements: [FR-CLI-04](../../../../../docs/prd.md#fr-cli-04), [FR-RULE-07](../../../../../docs/prd.md#fr-rule-07)
//!
//! | Module | Reads |
//! | --- | --- |
//! | [`archunit`] | ArchUnitNET and NetArchTest test projects (C#, through `tree-sitter-c-sharp`) |
//! | [`import_linter`] | `.importlinter`, `setup.cfg`, `pyproject.toml` |
//! | [`eslint`] | flat and legacy `ESLint` configurations |
//! | [`yaml`] | (writes) the ordered, commented YAML every importer produces |
//! | [`pattern`] | (writes) globs and module expressions as path patterns |
//!
//! Every importer writes a document the configuration loader accepts: the command loads its own
//! output before writing it and exits 2 if the loader refuses it, so an import never hands over
//! a file that fails on the next command. The output is deterministic: files are read in sorted
//! order and nothing depends on the clock or a hash map's order.

pub mod archunit;
pub mod csharp;
pub mod eslint;
pub mod import_linter;
pub mod netarchtest;
pub mod pattern;
pub mod types;
pub mod yaml;

use std::path::Path;

use clap::{Args, Subcommand};
use rb_config::load::LoadOptions;
use rb_config::read::Syntax;
use thiserror::Error;

use crate::context::Context;
use crate::{Outcome, RunExit};

/// The `$schema` every imported file names.
pub const SCHEMA: &str = "https://benbahrenburg.github.io/rulebearing/schema/config-v1.json";

/// `import`.
#[derive(Debug, Clone, Subcommand)]
pub enum ImportCommand {
    /// ArchUnitNET and NetArchTest tests (C#) as element and slice rules, the C# kept as comments
    Archunit(ArchunitArgs),
    /// import-linter contracts (.importlinter, setup.cfg, pyproject.toml) as rules
    #[command(name = "import-linter")]
    ImportLinter(FromArgs),
    /// import/no-restricted-paths and eslint-plugin-boundaries settings as forbidden rules
    Eslint(FromArgs),
}

/// `import archunit`.
#[derive(Debug, Clone, Args)]
pub struct ArchunitArgs {
    /// The directory of C# test files to read
    #[arg(value_name = "DIR")]
    pub dir: String,
    /// More directories whose C# declarations resolve `typeof(X)` (default: the solution folder
    /// above DIR, the nearest one holding a .sln or .slnx, else DIR itself)
    #[arg(long, value_name = "DIR")]
    pub sources: Vec<String>,
    /// A saved cruise result whose code layer names the types a test refers to, the referenced
    /// ones included, for types no source read declares (a package's interfaces)
    #[arg(long, value_name = "FILE")]
    pub graph: Option<String>,
    /// Write the configuration to FILE instead of stdout
    #[arg(long, value_name = "FILE")]
    pub out: Option<String>,
}

/// `import import-linter` and `import eslint`.
#[derive(Debug, Clone, Args)]
pub struct FromArgs {
    /// The file to read (default: the first of the usual names in the working directory)
    #[arg(long, value_name = "FILE")]
    pub from: Option<String>,
    /// Write the configuration to FILE instead of stdout
    #[arg(long, value_name = "FILE")]
    pub out: Option<String>,
}

/// Why an import could not be made.
#[derive(Debug, Error)]
pub enum ImportError {
    /// A file could not be read.
    #[error("cannot read {file}: {reason}")]
    Read {
        /// The file.
        file: String,
        /// Why.
        reason: String,
    },
    /// A file does not parse.
    #[error("{file} does not parse: {reason}")]
    Parse {
        /// The file.
        file: String,
        /// What the parser said.
        reason: String,
    },
    /// Nothing to import was found, or it has the wrong shape.
    #[error("{0}")]
    Invalid(String),
}

/// Runs `import`.
pub fn run(ctx: &Context<'_>, command: &ImportCommand) -> Outcome {
    let (made, out) = match command {
        ImportCommand::Archunit(a) => (archunit_command(ctx, a), a.out.as_deref()),
        ImportCommand::ImportLinter(a) => (
            located(ctx, a.from.as_deref(), import_linter::DEFAULT_FILES, |p| {
                import_linter::holds_settings(p)
            })
            .and_then(|(path, shown)| import_linter::import(&path, &shown)),
            a.out.as_deref(),
        ),
        ImportCommand::Eslint(a) => (
            located(ctx, a.from.as_deref(), eslint::DEFAULT_FILES, |p| {
                eslint::holds_config(p)
            })
            .and_then(|(path, shown)| eslint::import(&path, &shown, &ctx.cwd)),
            a.out.as_deref(),
        ),
    };
    let document = match made {
        Ok(document) => document,
        Err(e) => {
            return Outcome::failed(RunExit::InvalidConfig, format!("rulebearing import: {e}\n"));
        }
    };
    let text = yaml::render(&document);
    if let Err(e) = loads(&text, &ctx.cwd) {
        return Outcome::failed(
            RunExit::Untrustworthy,
            format!(
                "rulebearing import: the configuration written does not load, which is a bug in the importer: {e}\n"
            ),
        );
    }
    let mut stdout = String::new();
    match crate::write_output(ctx, out.unwrap_or("-"), &text, &mut stdout) {
        Ok(()) => Outcome::printed(stdout),
        Err(e) => Outcome::failed(RunExit::Untrustworthy, format!("rulebearing import: {e}\n")),
    }
}

/// Loads rendered YAML through the configuration loader, as `rulebearing cruise` would.
///
/// # Errors
/// The loader's message.
pub fn loads(text: &str, base: &Path) -> Result<rb_config::Config, String> {
    let options = LoadOptions {
        root: Some(base.to_path_buf()),
        ..LoadOptions::default()
    };
    rb_config::load_text(text, Syntax::Yaml, base, &options).map_err(|e| e.to_string())
}

fn archunit_command(ctx: &Context<'_>, args: &ArchunitArgs) -> Result<yaml::Document, ImportError> {
    let dir = ctx.resolve(&args.dir);
    if !dir.is_dir() {
        return Err(ImportError::Invalid(format!(
            "{} is not a directory",
            args.dir
        )));
    }
    let sources: Vec<_> = args.sources.iter().map(|s| ctx.resolve(s)).collect();
    archunit::import(&archunit::Request {
        dir,
        shown: pattern::relative(&args.dir),
        sources,
        graph: args.graph.as_ref().map(|g| ctx.resolve(g)),
        cwd: ctx.cwd.clone(),
    })
}

/// The file an importer reads: `--from`, else the first default name in the working directory
/// that holds what the importer reads. Returns the path and the name to show.
fn located(
    ctx: &Context<'_>,
    from: Option<&str>,
    defaults: &[&str],
    holds: impl Fn(&Path) -> bool,
) -> Result<(std::path::PathBuf, String), ImportError> {
    if let Some(file) = from {
        let path = ctx.resolve(file);
        return if path.is_file() {
            Ok((path, pattern::relative(file)))
        } else {
            Err(ImportError::Read {
                file: file.to_owned(),
                reason: "no such file".into(),
            })
        };
    }
    defaults
        .iter()
        .map(|name| (ctx.cwd.join(name), (*name).to_owned()))
        .find(|(path, _)| path.is_file() && holds(path))
        .ok_or_else(|| {
            ImportError::Invalid(format!(
                "none of {} is here with settings to import; name the file with --from",
                defaults.join(", ")
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_render() {
        let read = ImportError::Read {
            file: "a".into(),
            reason: "b".into(),
        };
        assert_eq!(read.to_string(), "cannot read a: b");
        let parse = ImportError::Parse {
            file: "a".into(),
            reason: "b".into(),
        };
        assert_eq!(parse.to_string(), "a does not parse: b");
        assert_eq!(ImportError::Invalid("x".into()).to_string(), "x");
    }

    #[test]
    fn output_that_does_not_load_is_caught() {
        assert!(loads("rules:\n  elements:\n    - name: x\n", Path::new(".")).is_err());
        assert!(loads("rules: {}\n", Path::new(".")).is_ok());
    }
}
