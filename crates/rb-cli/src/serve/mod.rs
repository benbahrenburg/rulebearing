//! `rulebearing serve --mcp | --lsp`: the architecture as tools for an agent and as diagnostics
//! for an editor, over standard input and output, from one warm graph.
//!
//! - Architecture: [`docs/architecture.md#agent-surface`](../../../../docs/architecture.md#agent-surface)
//! - Plan: [Wave 3, Steps 18 to 20](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#25-steps-for-sub-wave-3e-serve---mcp-and-serve---lsp)
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Requirement: [FR-CLI-06](../../../../docs/prd.md#fr-cli-06), [NFR-SEC-01](../../../../docs/prd.md#nfr-sec-01)

pub mod graph;
pub mod jsonrpc;
pub mod mcp;

use std::io::{BufRead, Write};

use chrono::NaiveDate;
use clap::Args;

use crate::cli::ConfigArgs;
use crate::context::Context;
use crate::{Outcome, RunExit};

/// `serve`.
#[derive(Debug, Clone, Default, Args)]
pub struct ServeArgs {
    /// Answer Model Context Protocol tool calls, one JSON-RPC message per line
    #[arg(long, required = true)]
    pub mcp: bool,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
}

/// Where a server runs and the clock it was started with: the parts of a [`Context`] each
/// command it runs gets.
#[derive(Debug, Clone)]
pub struct Origin {
    /// The working directory.
    pub cwd: std::path::PathBuf,
    /// Today, for `expires`.
    pub today: NaiveDate,
    /// Now, for receipts.
    pub timestamp: String,
}

impl Origin {
    /// The origin of `ctx`.
    pub fn of(ctx: &Context<'_>) -> Self {
        Self {
            cwd: ctx.cwd.clone(),
            today: ctx.today,
            timestamp: ctx.timestamp.clone(),
        }
    }
}

/// What a server keeps between messages: where it runs, the clock it was started with, and the
/// warm graph.
pub struct Session {
    cwd: std::path::PathBuf,
    today: NaiveDate,
    timestamp: String,
    /// The graph every command run answers from.
    pub warm: graph::WarmGraph,
}

impl Session {
    /// A session at `origin`, with the configuration `config` names.
    pub fn new(origin: Origin, config: &ConfigArgs) -> Self {
        Self {
            cwd: origin.cwd,
            today: origin.today,
            timestamp: origin.timestamp,
            warm: graph::WarmGraph::open(config),
        }
    }

    /// Runs one command line over the warm graph, reading the graph again first when it changed.
    /// The command reads no standard input: the server's input is the protocol's.
    pub fn run(&mut self, line: &[String]) -> Outcome {
        let mut empty: &[u8] = &[];
        let plain = Context {
            cwd: self.cwd.clone(),
            stdin: &mut empty,
            today: self.today,
            timestamp: self.timestamp.clone(),
            color_terminal: false,
            warm: None,
        };
        if let Err(message) = self.warm.ensure_fresh(&plain) {
            return Outcome::failed(
                RunExit::Untrustworthy,
                format!("rulebearing serve: {message}\n"),
            );
        }
        let mut empty: &[u8] = &[];
        let mut ctx = Context {
            cwd: self.cwd.clone(),
            stdin: &mut empty,
            today: self.today,
            timestamp: self.timestamp.clone(),
            color_terminal: false,
            warm: Some(&self.warm),
        };
        crate::run_in(&mut ctx, line)
    }
}

/// Serves the protocol `args` names over `input` and `output` until the input ends; logs to
/// `log` only.
pub fn run(
    origin: Origin,
    args: &ServeArgs,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    log: &mut dyn Write,
) -> u8 {
    if args.config.config.as_deref() == Some("-") {
        let _ = writeln!(
            log,
            "rulebearing serve: --config - reads standard input, which carries the protocol; name the file"
        );
        return RunExit::InvalidConfig.code();
    }
    let mut session = Session::new(origin, &args.config);
    let served = mcp::serve(&mut session, input, output);
    match served {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(log, "rulebearing serve: {error}");
            RunExit::Untrustworthy.code()
        }
    }
}
