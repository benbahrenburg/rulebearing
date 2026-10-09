//! `rulebearing query --from <regex> --to <regex> [--json]`: the edges matching a from and to
//! pair.
//!
//! - Source: [design § Hooks, test runners, an MCP server, an LSP](../../../../docs/artifacts/design.md#hooks-test-runners-an-mcp-server-an-lsp)
//!   (`query`, "edges matching a from and to pair", one of the MCP server's tools)
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md) (every MCP tool is
//!   a command first)
//! - Plan: [Wave 3, Step 19](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#25-steps-for-sub-wave-3e-serve---mcp-and-serve---lsp)
//! - Requirement: [FR-CLI-06](../../../../docs/prd.md#fr-cli-06)
//!
//! The edges are the ones `count` counts, from the same graph (`--graph`, the saved
//! `.graph/cruise.json`, or a fresh extraction of the paths) and with the same patterns (`$1` in
//! `--to` takes the capture from `--from`), in document order. Text prints one `from -> to` per
//! line; `--json` prints `{ "from", "to", "count", "edges": [{ "from", "to" }] }`.

use std::fmt::Write as _;

use clap::Args;
use serde_json::json;

use crate::cli::GraphArgs;
use crate::cmd::count::matching_edges;
use crate::context::Context;
use crate::{Outcome, RunExit};

/// `query`.
#[derive(Debug, Clone, Default, Args)]
pub struct QueryArgs {
    /// Sources the edges start from
    #[arg(long, value_name = "REGEX")]
    pub from: String,
    /// Targets the edges end at; $1 takes the capture from --from
    #[arg(long, value_name = "REGEX")]
    pub to: String,
    /// Print JSON
    #[arg(long)]
    pub json: bool,
    /// The graph
    #[command(flatten)]
    pub graph: GraphArgs,
}

/// Runs `query`.
pub fn run(ctx: &mut Context<'_>, args: &QueryArgs) -> Outcome {
    let edges = match matching_edges(ctx, &args.graph, &args.from, &args.to) {
        Ok(edges) => edges,
        Err(m) => {
            return Outcome::failed(RunExit::Untrustworthy, format!("rulebearing query: {m}\n"));
        }
    };
    if args.json {
        let mut text = serde_json::to_string_pretty(&json!({
            "from": args.from,
            "to": args.to,
            "count": edges.len(),
            "edges": edges,
        }))
        .unwrap_or_default();
        text.push('\n');
        return Outcome::printed(text);
    }
    let mut out = String::new();
    for edge in &edges {
        let _ = writeln!(out, "{} -> {}", edge.from, edge.to);
    }
    Outcome::printed(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(dir: &std::path::Path, run: impl FnOnce(&mut Context<'_>)) {
        let mut stdin: &[u8] = b"";
        let mut ctx = Context {
            cwd: dir.to_path_buf(),
            stdin: &mut stdin,
            today: chrono::NaiveDate::default(),
            timestamp: String::new(),
            color_terminal: false,
            warm: None,
        };
        run(&mut ctx);
    }

    #[test]
    fn query_lists_what_count_counts_in_document_order() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("rb-query-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let graph = json!({ "modules": [
            { "source": "apps/a/x.ts", "dependencies": [
                { "module": "../../lib/b.ts", "resolved": "lib/b.ts", "moduleSystem": "es6",
                  "dynamic": false, "exoticallyRequired": false, "dependencyTypes": ["local"],
                  "coreModule": false, "followable": true, "couldNotResolve": false,
                  "matchesDoNotFollow": false, "circular": false, "valid": true },
                { "module": "./y.ts", "resolved": "apps/a/y.ts", "moduleSystem": "es6",
                  "dynamic": false, "exoticallyRequired": false, "dependencyTypes": ["local"],
                  "coreModule": false, "followable": true, "couldNotResolve": false,
                  "matchesDoNotFollow": false, "circular": false, "valid": true } ],
              "dependents": [], "orphan": false, "valid": true },
            { "source": "apps/a/y.ts", "dependencies": [], "dependents": [], "orphan": false, "valid": true },
            { "source": "lib/b.ts", "dependencies": [], "dependents": [], "orphan": false, "valid": true }
        ], "summary": { "violations": [], "error": 0, "warn": 0, "info": 0, "ignore": 0,
            "totalCruised": 3, "totalDependenciesCruised": 2, "optionsUsed": {} } });
        std::fs::write(dir.join("graph.json"), graph.to_string())?;
        let graph_args = GraphArgs {
            graph: Some("graph.json".into()),
            paths: Vec::new(),
        };
        let query = |json: bool, to: &str| QueryArgs {
            from: "^apps/([^/]+)/".into(),
            to: to.into(),
            json,
            graph: graph_args.clone(),
        };
        context(&dir, |ctx| {
            let text = run(ctx, &query(false, "."));
            assert_eq!(text.code, 0, "{}", text.stderr);
            assert_eq!(
                text.stdout,
                "apps/a/x.ts -> lib/b.ts\napps/a/x.ts -> apps/a/y.ts\n"
            );
            let json = run(ctx, &query(true, "^apps/$1/"));
            let value: serde_json::Value = serde_json::from_str(&json.stdout).unwrap_or_default();
            assert_eq!(value["count"], 1);
            assert_eq!(
                value["edges"],
                json!([{ "from": "apps/a/x.ts", "to": "apps/a/y.ts" }])
            );
            assert_eq!(value["to"], "^apps/$1/");
            let counted = crate::cmd::count::run(
                ctx,
                &crate::cmd::count::CountArgs {
                    from: "^apps/([^/]+)/".into(),
                    to: "^apps/$1/".into(),
                    json: true,
                    graph: graph_args.clone(),
                    ..crate::cmd::count::CountArgs::default()
                },
            );
            assert_eq!(counted.stdout, "{\n  \"count\": 1\n}\n");
            let missing = run(
                ctx,
                &QueryArgs {
                    graph: GraphArgs {
                        graph: Some("missing.json".into()),
                        paths: Vec::new(),
                    },
                    ..query(false, ".")
                },
            );
            assert_eq!(missing.code, 2);
            assert!(missing.stderr.starts_with("rulebearing query: "));
        });
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
