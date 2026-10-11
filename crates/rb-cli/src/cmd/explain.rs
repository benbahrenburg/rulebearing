//! `rulebearing explain <rule> [--plain]`: a rule, its fix, what it matches and the first ten edges.
//!
//! - Source: [design § Questions an agent can ask](../../../../docs/artifacts/design.md#questions-an-agent-can-ask-before-it-writes-the-import),
//!   [design § The agentic engineering hat](../../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two) (`explain --plain`)
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Plan: [Wave 1, Step 14](../../../../docs/plans/implemented/0001-wave-1-typescript-parity.md#step-14-rules---json-explain-explain---plain-test-can-import-1e)
//! - Plan: [Wave 3, Step 19](../../../../docs/plans/implemented/0003-wave-3-operations-surface-inner-loop.md#25-steps-for-sub-wave-3e-serve---mcp-and-serve---lsp)
//!   (`--json`, the MCP tool's answer)
//! - Requirement: [FR-CLI-01](../../../../docs/prd.md#fr-cli-01), [FR-CLI-06](../../../../docs/prd.md#fr-cli-06)
//!
//! `--json` prints the rule as `rules --json` lists it (with its statistics, or without them under
//! `--plain`), plus its `sentence` and, unless `--plain`, the `firstEdges` the text lists. What
//! is printed is [`rb_rules::explain`]'s, which the playground's `explain` prints too
//! ([Wave 4, Step 2](../../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables)).

use clap::Args;

use crate::cli::{ConfigArgs, GraphArgs};
use crate::cmd::rules;
use crate::context::Context;
use crate::{Outcome, RunExit, configure};

/// `explain`.
#[derive(Debug, Clone, Default, Args)]
pub struct ExplainArgs {
    /// The rule (for an `allowed` entry, `allowed[n]`)
    pub rule: String,
    /// One English sentence per rule, no graph needed
    #[arg(long)]
    pub plain: bool,
    /// Print JSON
    #[arg(long)]
    pub json: bool,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
    /// The graph
    #[command(flatten)]
    pub graph: GraphArgs,
}

/// Runs `explain`.
pub fn run(ctx: &mut Context<'_>, args: &ExplainArgs) -> Outcome {
    let config = match configure::required(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let explained = match rb_rules::explain::find(&config, &args.rule) {
        Ok(explained) => explained,
        Err(no_rule) => {
            return Outcome::failed(
                RunExit::InvalidConfig,
                format!("rulebearing explain: {no_rule}\n"),
            );
        }
    };
    if args.plain {
        return Outcome::printed(explained.plain(args.json));
    }
    match rules::statistics(ctx, &config, &args.graph) {
        Ok(evaluation) => Outcome::printed(explained.with_graph(&evaluation, args.json)),
        Err(o) => o,
    }
}
