//! `rulebearing attest [--verify]`: a receipt proving which configuration, inputs and results a
//! run had, at which commit.
//!
//! - Contract: [Wave 1 plan § 1.5](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#15-interfaces-and-contracts-frozen-by-this-wave)
//!   (`.graph/attest.json`: tool, configHash, inputsHash, resultsHash, head, createdAt; SHA-256)
//! - Source: [design § The agentic engineering hat](../../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two)
//! - Plan: [Wave 1, Step 15](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-15-hooks-install---claude-code-summary---format-agent-impact-attest---require-comment-token-1e)
//! - Requirement: [FR-CLI-03](../../../../docs/prd.md#fr-cli-03)
//!
//! `configHash` covers every configuration file the load read, in path order; `inputsHash` every
//! extracted source file (or the `--graph` document), in path order; `resultsHash` the violations.
//! `--verify` recomputes all three and the commit and exits 1 on any mismatch, naming it.

use std::fmt::Write as _;
use std::process::Command;

use clap::Args;
use rb_model::GraphDocument;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cli::ConfigArgs;
use crate::context::Context;
use crate::pipeline::{self, RunOptions};
use crate::progress::Progress;
use crate::{Outcome, RunExit, configure};

/// Where the receipt is written.
pub const RECEIPT: &str = ".graph/attest.json";

/// `attest`.
#[derive(Debug, Clone, Default, Args)]
pub struct AttestArgs {
    /// Recompute and compare with the saved receipt
    #[arg(long)]
    pub verify: bool,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
    /// Evaluate this graph document instead of extracting
    #[arg(long, value_name = "FILE")]
    pub graph: Option<String>,
    /// Files, directories and globs to cruise
    #[arg(value_name = "FILES-OR-DIRECTORIES")]
    pub paths: Vec<String>,
}

/// The receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    /// `rulebearing <version>`.
    pub tool: String,
    /// SHA-256 of the configuration files.
    pub config_hash: String,
    /// SHA-256 of the inputs.
    pub inputs_hash: String,
    /// SHA-256 of the violations.
    pub results_hash: String,
    /// The commit.
    pub head: String,
    /// When the receipt was written.
    pub created_at: String,
}

fn hex(digest: &[u8]) -> String {
    digest.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// SHA-256 over each (name, bytes) pair, in the order given.
pub fn hash_files<'a>(files: impl Iterator<Item = (String, &'a [u8])>) -> String {
    let mut hasher = Sha256::new();
    for (name, bytes) in files {
        hasher.update(name.as_bytes());
        hasher.update([0]);
        hasher.update(bytes);
        hasher.update([0]);
    }
    hex(&hasher.finalize())
}

fn head(ctx: &Context<'_>) -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&ctx.cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_else(
            || "unknown".to_owned(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_owned(),
        )
}

fn inputs_hash(ctx: &Context<'_>, document: &GraphDocument, graph: Option<&str>) -> String {
    if let Some(file) = graph {
        let bytes = std::fs::read(ctx.resolve(file)).unwrap_or_default();
        return hash_files(std::iter::once((file.to_owned(), bytes.as_slice())));
    }
    let mut sources: Vec<&str> = document
        .modules
        .iter()
        .filter(|m| m.language.is_some())
        .map(|m| m.source.as_str())
        .collect();
    sources.sort_unstable();
    let contents: Vec<(String, Vec<u8>)> = sources
        .iter()
        .map(|s| {
            (
                (*s).to_owned(),
                std::fs::read(ctx.resolve(s)).unwrap_or_default(),
            )
        })
        .collect();
    hash_files(contents.iter().map(|(n, b)| (n.clone(), b.as_slice())))
}

/// Computes the receipt for the current tree.
///
/// # Errors
/// An [`Outcome`] to return when the run fails.
pub fn compute(ctx: &mut Context<'_>, args: &AttestArgs) -> Result<Receipt, Outcome> {
    let config = configure::required(ctx, &args.config)?;
    let options = RunOptions {
        liveness: false,
        options_used: serde_json::Map::new(),
        paths: args.paths.clone(),
    };
    let mut progress = Progress::new(None);
    let document = match &args.graph {
        Some(file) => pipeline::load_graph(ctx, file).map_err(|m| {
            Outcome::failed(RunExit::Untrustworthy, format!("rulebearing attest: {m}\n"))
        })?,
        None => pipeline::extract(ctx, &config, &args.paths).map_err(|e| {
            Outcome::failed(RunExit::Untrustworthy, format!("rulebearing attest: {e}\n"))
        })?,
    };
    let inputs = inputs_hash(ctx, &document, args.graph.as_deref());
    let mut document = document;
    pipeline::reset(&mut document);
    let run = pipeline::evaluate_document(ctx, &config, document, &options, &mut progress)
        .map_err(|e| {
            Outcome::failed(RunExit::Untrustworthy, format!("rulebearing attest: {e}\n"))
        })?;
    let mut config_files: Vec<(String, Vec<u8>)> = config
        .files
        .iter()
        .map(|p| {
            (
                p.to_string_lossy().replace('\\', "/"),
                std::fs::read(p).unwrap_or_default(),
            )
        })
        .collect();
    config_files.sort();
    let cwd = ctx
        .cwd
        .canonicalize()
        .unwrap_or_else(|_| ctx.cwd.clone())
        .to_string_lossy()
        .replace('\\', "/");
    let relative: Vec<(String, &[u8])> = config_files
        .iter()
        .map(|(n, b)| {
            (
                n.strip_prefix(&cwd)
                    .unwrap_or(n)
                    .trim_start_matches('/')
                    .to_owned(),
                b.as_slice(),
            )
        })
        .collect();
    let results =
        serde_json::to_vec(&run.evaluation.document.summary.violations).unwrap_or_default();
    Ok(Receipt {
        tool: format!("rulebearing {}", env!("CARGO_PKG_VERSION")),
        config_hash: hash_files(relative.into_iter()),
        inputs_hash: inputs,
        results_hash: hash_files(std::iter::once((
            "violations".to_owned(),
            results.as_slice(),
        ))),
        head: head(ctx),
        created_at: ctx.timestamp.clone(),
    })
}

/// Runs `attest`.
pub fn run(ctx: &mut Context<'_>, args: &AttestArgs) -> Outcome {
    let receipt = match compute(ctx, args) {
        Ok(r) => r,
        Err(o) => return o,
    };
    let path = ctx.resolve(RECEIPT);
    if args.verify {
        let saved: Receipt = match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
        {
            Ok(r) => r,
            Err(e) => {
                return Outcome::failed(
                    RunExit::Untrustworthy,
                    format!("rulebearing attest --verify: cannot read {RECEIPT}: {e}\n"),
                );
            }
        };
        let mut mismatches = Vec::new();
        for (name, a, b) in [
            ("configHash", &saved.config_hash, &receipt.config_hash),
            ("inputsHash", &saved.inputs_hash, &receipt.inputs_hash),
            ("resultsHash", &saved.results_hash, &receipt.results_hash),
            ("head", &saved.head, &receipt.head),
        ] {
            if a != b {
                mismatches.push(format!("{name}: saved {a}, now {b}"));
            }
        }
        if mismatches.is_empty() {
            return Outcome::printed(format!("attest: {RECEIPT} matches HEAD {}\n", receipt.head));
        }
        return Outcome::failed(
            RunExit::Violations(1),
            format!(
                "rulebearing attest --verify: {RECEIPT} does not match:\n  {}\n",
                mismatches.join("\n  ")
            ),
        );
    }
    let mut text = serde_json::to_string_pretty(&receipt).unwrap_or_default();
    text.push('\n');
    let mut stdout = String::new();
    match crate::write_output(ctx, RECEIPT, &text, &mut stdout) {
        Ok(()) => Outcome::printed(format!("attest: wrote {RECEIPT}\n")),
        Err(m) => Outcome::failed(RunExit::Untrustworthy, format!("rulebearing attest: {m}\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashing_is_order_and_boundary_sensitive() {
        let a = hash_files([("a".to_owned(), b"xy".as_slice())].into_iter());
        let b = hash_files(
            [
                ("a".to_owned(), b"x".as_slice()),
                ("y".to_owned(), b"".as_slice()),
            ]
            .into_iter(),
        );
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert_eq!(
            a,
            hash_files([("a".to_owned(), b"xy".as_slice())].into_iter())
        );
    }
}
