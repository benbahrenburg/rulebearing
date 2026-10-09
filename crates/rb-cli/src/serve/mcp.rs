//! `serve --mcp`: the query commands as Model Context Protocol tools, over standard input and
//! output.
//!
//! - Source: [design § Hooks, test runners, an MCP server, an LSP](../../../../docs/artifacts/design.md#hooks-test-runners-an-mcp-server-an-lsp)
//!   (the eight tools)
//! - Plan: [Wave 3, Step 19](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#25-steps-for-sub-wave-3e-serve---mcp-and-serve---lsp),
//!   [§ 1.5](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//!   ("each tool's result is byte-identical to the corresponding CLI command with `--json`")
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Requirement: [FR-CLI-06](../../../../docs/prd.md#fr-cli-06), [NFR-SEC-01](../../../../docs/prd.md#nfr-sec-01)
//!
//! The server answers `initialize`, `ping`, `tools/list` and `tools/call`, one JSON-RPC message
//! per line, and ignores notifications. Each tool is a command line: the call's arguments become
//! the command's flags, the command runs in this process over the warm graph (`serve::graph`), and
//! the tool's text is the command's standard output, byte for byte, so a tool answers exactly what
//! `rulebearing <command> --json --graph <the warm graph's file>` prints. A command that could not
//! answer (exit 2 or 3) has its standard error added to the text, with `isError`; exit 1, a
//! forbidden import or a count over its budget, is an answer like exit 0
//! ([ADR-0008](../../../../docs/adr/0008-exit-code-contract.md)). The tools only read: `count` has no `--write`,
//! and nothing is extracted unless no graph is saved, as the commands do. The server stops at the
//! end of its input.

use std::io::{BufRead, Write};

use serde_json::{Map, Value, json};

use super::Session;
use super::jsonrpc::{self, Incoming, code};
use crate::cli::ConfigArgs;

/// The protocol versions the server speaks, newest first; it answers with the client's when it
/// is one of them, else the newest.
pub const PROTOCOL_VERSIONS: [&str; 4] = ["2025-06-18", "2025-03-26", "2024-11-05", "2024-10-07"];

/// One tool: its name, what it answers, its input schema and how its arguments become a command
/// line.
struct Tool {
    name: &'static str,
    description: &'static str,
    schema: fn() -> Value,
    command: ToCommand,
}

/// How a tool's arguments become a command line, given the server's configuration.
type ToCommand = fn(&Map<String, Value>, &ConfigArgs) -> Result<Vec<String>, String>;

fn string_property(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn graph_property() -> Value {
    string_property(
        "A saved cruise result to answer from instead of the server's warm graph (--graph)",
    )
}

/// The eight tools, in the design's order.
const TOOLS: [Tool; 8] = [
    Tool {
        name: "rules",
        description: "Every rule with its family, severity, comment, fix and what it matched on each side (rulebearing rules --json)",
        schema: || {
            json!({ "type": "object", "properties": {
                "unused": { "type": "boolean", "description": "List the rules that matched nothing in each of the last --releases snapshots (--unused)" },
                "releases": { "type": "integer", "minimum": 1, "description": "With unused: how many snapshots to read (--releases)" },
                "graph": graph_property(),
            } })
        },
        command: |args, config| {
            let mut line = vec!["rules".to_owned(), "--json".to_owned()];
            if flag(args, "unused")? {
                line.push("--unused".into());
            }
            if let Some(n) = integer(args, "releases")? {
                line.extend(["--releases".into(), n.to_string()]);
            }
            graph(args, &mut line)?;
            with_config(config, &mut line);
            Ok(line)
        },
    },
    Tool {
        name: "explain",
        description: "One rule: its sentence, why it exists, its fix, what it matches and the first edges it flagged (rulebearing explain <rule> --json)",
        schema: || {
            json!({ "type": "object", "required": ["rule"], "properties": {
                "rule": string_property("The rule's name, or allowed[n] for an allowed entry"),
                "plain": { "type": "boolean", "description": "The sentence alone, no graph read (--plain)" },
                "graph": graph_property(),
            } })
        },
        command: |args, config| {
            let mut line = vec!["explain".to_owned(), "--json".to_owned()];
            if flag(args, "plain")? {
                line.push("--plain".into());
            }
            graph(args, &mut line)?;
            with_config(config, &mut line);
            line.extend(["--".into(), required(args, "rule")?]);
            Ok(line)
        },
    },
    Tool {
        name: "can_import",
        description: "Would an import from one file to another be allowed, and if not which rule says so and what to do (rulebearing can-import <from> <to> --json)",
        schema: || {
            json!({ "type": "object", "required": ["from", "to"], "properties": {
                "from": string_property("The importing file"),
                "to": string_property("The file or package imported"),
                "graph": graph_property(),
            } })
        },
        command: |args, config| {
            let mut line = vec!["can-import".to_owned(), "--json".to_owned()];
            graph(args, &mut line)?;
            with_config(config, &mut line);
            line.extend(["--".into(), required(args, "from")?, required(args, "to")?]);
            Ok(line)
        },
    },
    Tool {
        name: "place",
        description: "The folders where a new module with these imports and importers would be legal (rulebearing place --json)",
        schema: || {
            json!({ "type": "object", "required": ["language"], "properties": {
                "language": string_property("The new module's language: ts, js, python or dotnet (--language)"),
                "imports": { "type": "array", "items": { "type": "string" }, "description": "Files the new module would import (--imports)" },
                "importedBy": { "type": "array", "items": { "type": "string" }, "description": "Files that would import the new module (--imported-by)" },
                "name": string_property("The new module's file name (--name)"),
                "graph": graph_property(),
            } })
        },
        command: |args, config| {
            let mut line = vec![
                "place".to_owned(),
                "--json".to_owned(),
                "--language".to_owned(),
                required(args, "language")?,
            ];
            for (key, option) in [("imports", "--imports"), ("importedBy", "--imported-by")] {
                let files = strings(args, key)?;
                if !files.is_empty() {
                    line.extend([option.to_owned(), files.join(",")]);
                }
            }
            if let Some(name) = optional(args, "name")? {
                line.extend(["--name".into(), name]);
            }
            graph(args, &mut line)?;
            with_config(config, &mut line);
            Ok(line)
        },
    },
    Tool {
        name: "impact",
        description: "What a file is subject to before an edit: the rules that reach it, its dependents and the violations it is in (rulebearing impact <file> --json)",
        schema: || {
            json!({ "type": "object", "required": ["file"], "properties": {
                "file": string_property("The file about to be edited"),
                "depth": { "type": "integer", "minimum": 0, "description": "How many levels of dependents to list (--depth)" },
                "graph": graph_property(),
            } })
        },
        command: |args, config| {
            let mut line = vec!["impact".to_owned(), "--json".to_owned()];
            if let Some(depth) = integer(args, "depth")? {
                line.extend(["--depth".into(), depth.to_string()]);
            }
            graph(args, &mut line)?;
            with_config(config, &mut line);
            line.extend(["--".into(), required(args, "file")?]);
            Ok(line)
        },
    },
    Tool {
        name: "count",
        description: "How many direct edges go from modules matching one pattern to targets matching another, against a ratchet's budget when given (rulebearing count --json)",
        schema: || {
            json!({ "type": "object", "required": ["from", "to"], "properties": {
                "from": string_property("Sources, a regular expression (--from)"),
                "to": string_property("Targets, a regular expression; $1 takes the capture from from (--to)"),
                "budget": string_property("A budget file, { \"ceiling\": n }, to compare with (--budget)"),
                "graph": graph_property(),
            } })
        },
        command: |args, _| {
            let mut line = vec![
                "count".to_owned(),
                "--json".to_owned(),
                "--from".to_owned(),
                required(args, "from")?,
                "--to".to_owned(),
                required(args, "to")?,
            ];
            if let Some(budget) = optional(args, "budget")? {
                line.extend(["--budget".into(), budget]);
            }
            graph(args, &mut line)?;
            Ok(line)
        },
    },
    Tool {
        name: "query",
        description: "The direct edges from modules matching one pattern to targets matching another (rulebearing query --json)",
        schema: || {
            json!({ "type": "object", "required": ["from", "to"], "properties": {
                "from": string_property("Sources, a regular expression (--from)"),
                "to": string_property("Targets, a regular expression; $1 takes the capture from from (--to)"),
                "graph": graph_property(),
            } })
        },
        command: |args, _| {
            let mut line = vec![
                "query".to_owned(),
                "--json".to_owned(),
                "--from".to_owned(),
                required(args, "from")?,
                "--to".to_owned(),
                required(args, "to")?,
            ];
            graph(args, &mut line)?;
            Ok(line)
        },
    },
    Tool {
        name: "diff",
        description: "Added and removed edges, new and resolved violations and moved ratchets between two saved results, or between a revision and the working tree (rulebearing diff -T json)",
        schema: || {
            json!({ "type": "object", "properties": {
                "old": string_property("The earlier saved cruise result"),
                "new": string_property("The later saved cruise result"),
                "base": string_property("A revision to compare the working tree with, instead of two results (--base)"),
                "paths": { "type": "array", "items": { "type": "string" }, "description": "With base: what to cruise on each side" },
            } })
        },
        command: |args, config| {
            let mut line = vec!["diff".to_owned(), "-T".to_owned(), "json".to_owned()];
            let base = optional(args, "base")?;
            if let Some(base) = &base {
                line.extend(["--base".into(), base.clone()]);
            }
            with_config(config, &mut line);
            line.push("--".into());
            match (optional(args, "old")?, optional(args, "new")?, base) {
                (Some(old), Some(new), None) => line.extend([old, new]),
                (None, None, Some(_)) => line.extend(strings(args, "paths")?),
                _ => {
                    return Err(
                        "diff takes either old and new, two saved results, or base".to_owned()
                    );
                }
            }
            Ok(line)
        },
    },
];

fn optional(args: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("{key} is a string")),
    }
}

fn required(args: &Map<String, Value>, key: &str) -> Result<String, String> {
    optional(args, key)?.ok_or_else(|| format!("{key} is required"))
}

fn flag(args: &Map<String, Value>, key: &str) -> Result<bool, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{key} is a boolean")),
    }
}

fn integer(args: &Map<String, Value>, key: &str) -> Result<Option<u64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("{key} is a whole number")),
    }
}

fn strings(args: &Map<String, Value>, key: &str) -> Result<Vec<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("{key} holds strings"))
            })
            .collect(),
        Some(_) => Err(format!("{key} is a list of strings")),
    }
}

fn graph(args: &Map<String, Value>, line: &mut Vec<String>) -> Result<(), String> {
    if let Some(file) = optional(args, "graph")? {
        line.extend(["--graph".into(), file]);
    }
    Ok(())
}

/// The configuration the server was started with, passed to each command that reads one.
fn with_config(config: &ConfigArgs, line: &mut Vec<String>) {
    if let Some(file) = &config.config {
        if file.is_empty() {
            line.push("--config".into());
        } else {
            line.extend(["--config".into(), file.clone()]);
        }
    }
}

/// Each tool's name and what it answers, in the design's order: what `docs --format skill`
/// lists.
pub fn tool_summaries() -> Vec<(&'static str, &'static str)> {
    TOOLS.iter().map(|t| (t.name, t.description)).collect()
}

/// The `tools/list` result.
fn list() -> Value {
    let tools: Vec<Value> = TOOLS
        .iter()
        .map(|tool| {
            json!({ "name": tool.name, "description": tool.description, "inputSchema": (tool.schema)() })
        })
        .collect();
    json!({ "tools": tools })
}

/// The command line a call runs, for a tool and its arguments.
///
/// # Errors
/// A message when the tool is unknown or an argument does not fit its schema.
pub fn command_line(
    name: &str,
    arguments: &Value,
    config: &ConfigArgs,
) -> Result<Vec<String>, String> {
    let tool = TOOLS
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| format!("no tool {name}"))?;
    let empty = Map::new();
    let arguments = match arguments {
        Value::Null => &empty,
        Value::Object(map) => map,
        _ => return Err("arguments is an object".to_owned()),
    };
    (tool.command)(arguments, config)
}

/// The `tools/call` result for one call.
fn call(session: &mut Session, params: &Value) -> Result<Value, String> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
    let line = command_line(name, &arguments, session.warm.config())?;
    let outcome = session.run(&line);
    // Exit 1 is an answer (a forbidden import, a count over its budget), not a failure: only a
    // command that could not answer (exit 2 or 3, ADR-0008) is an error.
    let failed = outcome.code >= 2;
    let text = if failed {
        format!("{}{}", outcome.stdout, outcome.stderr)
    } else {
        outcome.stdout
    };
    let mut result = json!({ "content": [{ "type": "text", "text": text }], "isError": failed });
    if !failed && let Ok(Value::Object(structured)) = serde_json::from_str::<Value>(&text) {
        result["structuredContent"] = Value::Object(structured);
    }
    Ok(result)
}

/// The answer to one message, or `None` for a notification.
fn answer(session: &mut Session, incoming: &Incoming) -> Option<Value> {
    let id = incoming.id.clone()?;
    let response = match incoming.method.as_str() {
        "initialize" => {
            let asked = incoming
                .params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let version = PROTOCOL_VERSIONS
                .iter()
                .find(|v| **v == asked)
                .unwrap_or(&PROTOCOL_VERSIONS[0]);
            jsonrpc::result(
                &id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "rulebearing", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": "The repository's architecture rules as tools. Ask can_import before writing an import, impact before editing a file, place before creating one; each answer names the rule and its fix.",
                }),
            )
        }
        "ping" => jsonrpc::result(&id, json!({})),
        "tools/list" => jsonrpc::result(&id, list()),
        "tools/call" => match call(session, &incoming.params) {
            Ok(result) => jsonrpc::result(&id, result),
            Err(message) => jsonrpc::error(&id, code::INVALID_PARAMS, &message),
        },
        other => jsonrpc::error(
            &id,
            code::METHOD_NOT_FOUND,
            &format!("rulebearing serve --mcp does not answer {other}"),
        ),
    };
    Some(response)
}

/// Serves until the input ends.
///
/// # Errors
/// An I/O error on the streams.
pub fn serve(
    session: &mut Session,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
) -> std::io::Result<()> {
    while let Some(line) = jsonrpc::read_line(input)? {
        let response = match jsonrpc::parse(&line) {
            Ok(incoming) => answer(session, &incoming),
            Err(error) => Some(error),
        };
        if let Some(response) = response {
            jsonrpc::write_line(output, &response)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rule_tools_turn_their_arguments_into_the_command_line() {
        let config = ConfigArgs {
            config: Some("rulebearing.yaml".into()),
            ..ConfigArgs::default()
        };
        let line = |name: &str, args: Value| command_line(name, &args, &config);
        let cfg = ["--config", "rulebearing.yaml"];
        let cases: Vec<(&str, Value, Vec<&str>)> = vec![
            (
                "rules",
                json!({}),
                [&["rules", "--json"][..], &cfg].concat(),
            ),
            (
                "rules",
                json!({ "unused": true, "releases": 2 }),
                [
                    &["rules", "--json", "--unused", "--releases", "2"][..],
                    &cfg,
                ]
                .concat(),
            ),
            (
                "explain",
                json!({ "rule": "-odd-name", "plain": true, "graph": "g.json" }),
                [
                    &["explain", "--json", "--plain", "--graph", "g.json"][..],
                    &cfg,
                    &["--", "-odd-name"],
                ]
                .concat(),
            ),
            (
                "can_import",
                json!({ "from": "src/a.ts", "to": "lib/b.ts" }),
                [
                    &["can-import", "--json"][..],
                    &cfg,
                    &["--", "src/a.ts", "lib/b.ts"],
                ]
                .concat(),
            ),
            (
                "place",
                json!({ "language": "ts", "imports": ["a.ts", "b.ts"], "importedBy": ["c.ts"], "name": "x.ts" }),
                [
                    &[
                        "place",
                        "--json",
                        "--language",
                        "ts",
                        "--imports",
                        "a.ts,b.ts",
                        "--imported-by",
                        "c.ts",
                        "--name",
                        "x.ts",
                    ][..],
                    &cfg,
                ]
                .concat(),
            ),
        ];
        for (name, args, expected) in cases {
            assert_eq!(
                line(name, args.clone()),
                Ok(expected.iter().map(|s| (*s).to_owned()).collect()),
                "{name} {args}"
            );
        }
    }

    #[test]
    fn the_graph_tools_turn_their_arguments_into_the_command_line() {
        let config = ConfigArgs {
            config: Some("rulebearing.yaml".into()),
            ..ConfigArgs::default()
        };
        let line = |name: &str, args: Value| command_line(name, &args, &config);
        let cfg = ["--config", "rulebearing.yaml"];
        let cases: Vec<(&str, Value, Vec<&str>)> = vec![
            (
                "impact",
                json!({ "file": "src/a.ts", "depth": 2 }),
                [
                    &["impact", "--json", "--depth", "2"][..],
                    &cfg,
                    &["--", "src/a.ts"],
                ]
                .concat(),
            ),
            (
                "count",
                json!({ "from": "^a", "to": "^b", "budget": "b.json" }),
                vec![
                    "count", "--json", "--from", "^a", "--to", "^b", "--budget", "b.json",
                ],
            ),
            (
                "query",
                json!({ "from": "^a", "to": "^b", "graph": "g.json" }),
                vec![
                    "query", "--json", "--from", "^a", "--to", "^b", "--graph", "g.json",
                ],
            ),
            (
                "diff",
                json!({ "old": "o.json", "new": "n.json" }),
                [
                    &["diff", "-T", "json"][..],
                    &cfg,
                    &["--", "o.json", "n.json"],
                ]
                .concat(),
            ),
            (
                "diff",
                json!({ "base": "HEAD~1", "paths": ["src"] }),
                [
                    &["diff", "-T", "json", "--base", "HEAD~1"][..],
                    &cfg,
                    &["--", "src"],
                ]
                .concat(),
            ),
        ];
        for (name, args, expected) in cases {
            assert_eq!(
                line(name, args.clone()),
                Ok(expected.iter().map(|s| (*s).to_owned()).collect()),
                "{name} {args}"
            );
        }
    }

    #[test]
    fn arguments_that_do_not_fit_are_refused_with_the_reason() {
        let config = ConfigArgs {
            config: Some("rulebearing.yaml".into()),
            ..ConfigArgs::default()
        };
        let line = |name: &str, args: Value| command_line(name, &args, &config);
        for (name, args, message) in [
            ("explain", json!({}), "rule is required"),
            ("count", json!({ "from": 1, "to": "x" }), "from is a string"),
            ("rules", json!({ "unused": "yes" }), "unused is a boolean"),
            (
                "impact",
                json!({ "file": "a", "depth": -1 }),
                "depth is a whole number",
            ),
            (
                "place",
                json!({ "language": "ts", "imports": [1] }),
                "imports holds strings",
            ),
            (
                "place",
                json!({ "language": "ts", "imports": "a" }),
                "imports is a list of strings",
            ),
            (
                "diff",
                json!({ "old": "o.json" }),
                "diff takes either old and new",
            ),
            ("frobnicate", json!({}), "no tool frobnicate"),
        ] {
            let got = line(name, args);
            assert!(
                got.as_ref().is_err_and(|e| e.starts_with(message)),
                "{name}: {got:?}"
            );
        }
        assert_eq!(
            command_line("rules", &json!([1]), &config),
            Err("arguments is an object".to_owned())
        );
        // With no configuration named, none is passed; an empty one asks the command to look.
        assert_eq!(
            command_line("rules", &Value::Null, &ConfigArgs::default()),
            Ok(vec!["rules".to_owned(), "--json".to_owned()])
        );
        let looked = ConfigArgs {
            config: Some(String::new()),
            ..ConfigArgs::default()
        };
        assert_eq!(
            command_line("rules", &Value::Null, &looked),
            Ok(vec![
                "rules".to_owned(),
                "--json".to_owned(),
                "--config".to_owned()
            ])
        );
    }

    #[test]
    fn the_tool_list_names_the_eight_tools_in_the_designs_order() {
        let listed = list();
        let names: Vec<&str> = listed["tools"]
            .as_array()
            .map(|tools| tools.iter().filter_map(|t| t["name"].as_str()).collect())
            .unwrap_or_default();
        assert_eq!(
            names,
            [
                "rules",
                "explain",
                "can_import",
                "place",
                "impact",
                "count",
                "query",
                "diff"
            ]
        );
        for tool in listed["tools"].as_array().into_iter().flatten() {
            assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
            assert!(tool["description"].as_str().is_some_and(|d| !d.is_empty()));
        }
    }
}
