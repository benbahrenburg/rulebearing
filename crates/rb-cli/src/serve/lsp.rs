//! `serve --lsp`: the gate's findings as editor diagnostics, with the rule's `fix` as the
//! quick-fix title, over standard input and output.
//!
//! - Source: [design § Hooks, test runners, an MCP server, an LSP](../../../../docs/artifacts/design.md#hooks-test-runners-an-mcp-server-an-lsp)
//! - Plan: [Wave 3, Step 20](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#25-steps-for-sub-wave-3e-serve---mcp-and-serve---lsp),
//!   [§ 1.5](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//!   (the diagnostic's `code`, `source` and `message`)
//! - Decisions: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md),
//!   [ADR-0015](../../../../docs/adr/0015-stable-violation-id.md) (the `code`)
//! - Requirement: [FR-CLI-06](../../../../docs/prd.md#fr-cli-06), [NFR-SEC-01](../../../../docs/prd.md#nfr-sec-01)
//!
//! The server evaluates the warm graph with the configuration, as `cruise` would, each time the
//! graph or the configuration changes, and publishes the violations of every open file as
//! `textDocument/publishDiagnostics`: `code` the stable violation id, `source` `rulebearing`,
//! `message` the rule's name and its `fix` (its comment when it has no `fix`), the severity from
//! the rule's, and the range the line of the offending import when the extractor recorded one,
//! else the file's first line. `textDocument/codeAction` offers one `quickfix` per diagnostic,
//! titled with the `fix`, whose command `rulebearing.explain` runs `explain <rule>` and shows its
//! text. A save re-checks: `cruise --cache -T json` over the paths the warm graph was cruised from
//! (`optionsUsed.args`, else `.`), whose cache reads again only what changed, and the result
//! becomes the warm graph. An edit not yet saved changes nothing, since the gate reads files. The
//! server answers `shutdown`, stops at `exit` or at the end of its input, and writes nothing but
//! the cache under `.graph/`.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};

use rb_model::{GraphDocument, Severity, Violation};
use serde_json::{Value, json};

use super::Session;
use super::jsonrpc::{self, Incoming, code};
use crate::pipeline::{self, RunOptions};
use crate::progress::Progress;

/// The command a quick fix runs.
pub const EXPLAIN_COMMAND: &str = "rulebearing.explain";

/// What the server keeps beside the session: the open files, and the violations of the last
/// evaluation with the generation of the graph they were evaluated for.
#[derive(Default)]
struct State {
    open: BTreeMap<String, String>,
    evaluated: Option<(u64, Vec<Violation>, GraphDocument)>,
    shutdown: bool,
    /// Why the last evaluation could not be made, sent once as a message.
    failure: Option<String>,
}

/// The path a `file://` URI names, relative to `root` when inside it, with `/` separators.
pub fn relative_path(uri: &str, root: &std::path::Path) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let decoded = percent_decode(rest);
    // `file:///C:/x` on Windows names `C:/x`.
    let path = match decoded.strip_prefix('/') {
        Some(drive) if drive.as_bytes().get(1) == Some(&b':') => drive.to_owned(),
        _ => decoded,
    };
    let root = crate::cache::key::slashed(&rb_model::without_verbatim(root));
    let root = root.trim_end_matches('/');
    let normalised = path.replace('\\', "/");
    Some(
        normalised
            .strip_prefix(root)
            .and_then(|r| r.strip_prefix('/'))
            .map_or_else(|| normalised.clone(), str::to_owned),
    )
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The LSP severity of a rule's severity; `None` for `ignore`, which is not reported.
fn severity(severity: Severity) -> Option<u8> {
    match severity {
        Severity::Error => Some(1),
        Severity::Warn => Some(2),
        Severity::Info => Some(3),
        Severity::Ignore => None,
    }
}

/// The zero-based line of the import a violation is about, when the extractor recorded it.
fn line_of(document: &GraphDocument, violation: &Violation) -> Option<u32> {
    document
        .modules
        .iter()
        .find(|m| m.source == violation.from)?
        .dependencies
        .iter()
        .find(|d| d.resolved == violation.to)?
        .line
        .map(|line| line.saturating_sub(1))
}

/// The diagnostics of `file`: its violations, in the order the evaluation gave them.
pub fn diagnostics(document: &GraphDocument, violations: &[Violation], file: &str) -> Vec<Value> {
    violations
        .iter()
        .filter(|v| v.from == file)
        .filter_map(|v| {
            let level = severity(v.rule.severity)?;
            let line = line_of(document, v).unwrap_or(0);
            let advice = v.fix.as_deref().or(v.comment.as_deref());
            let message = match advice {
                Some(advice) => format!("{}: {advice}", v.rule.name),
                None => v.rule.name.clone(),
            };
            Some(json!({
                "range": {
                    "start": { "line": line, "character": 0 },
                    "end": { "line": line + 1, "character": 0 },
                },
                "severity": level,
                "code": v.id,
                "source": "rulebearing",
                "message": message,
                "data": { "rule": v.rule.name, "to": v.to, "fix": advice },
            }))
        })
        .collect()
}

/// The quick fixes for the diagnostics a `codeAction` request carries: one per diagnostic of
/// this server, titled with its `fix`.
pub fn code_actions(context_diagnostics: &[Value]) -> Vec<Value> {
    context_diagnostics
        .iter()
        .filter(|d| d["source"] == "rulebearing")
        .filter_map(|d| {
            let rule = d["data"]["rule"].as_str()?;
            let title = d["data"]["fix"]
                .as_str()
                .map_or_else(|| format!("Explain {rule}"), str::to_owned);
            Some(json!({
                "title": title,
                "kind": "quickfix",
                "diagnostics": [d],
                "command": {
                    "title": format!("Explain {rule}"),
                    "command": EXPLAIN_COMMAND,
                    "arguments": [rule],
                },
            }))
        })
        .collect()
}

/// Evaluates the warm graph again when it changed since the last evaluation.
fn evaluate(session: &mut Session, state: &mut State) {
    match session.ensure_fresh() {
        Ok(_) => {}
        Err(message) => {
            state.failure = Some(message);
            return;
        }
    }
    let generation = session.warm.generation();
    if state
        .evaluated
        .as_ref()
        .is_some_and(|(g, _, _)| *g == generation)
    {
        return;
    }
    let Some(held) = session.warm.document().cloned() else {
        state.evaluated = Some((generation, Vec::new(), GraphDocument::default()));
        return;
    };
    let config_args = session.warm.config().clone();
    let evaluated = session.with_context(false, |ctx| {
        let config = crate::configure::load(ctx, &config_args)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let files = config.files.clone();
        let mut document = held;
        pipeline::reset(&mut document);
        let options = RunOptions {
            liveness: false,
            options_used: serde_json::Map::new(),
            paths: Vec::new(),
            affected: None,
        };
        let run =
            pipeline::evaluate_document(ctx, &config, document, &options, &mut Progress::new(None))
                .map_err(|e| e.to_string())?;
        let violations = run.document.summary.violations.clone();
        Ok::<_, String>((files, violations, run.into_evaluation().document))
    });
    match evaluated {
        Ok((files, violations, document)) => {
            session.warm.watch_config(&files);
            state.evaluated = Some((session.warm.generation(), violations, document));
            state.failure = None;
        }
        Err(message) => state.failure = Some(message),
    }
}

/// The paths the warm graph was cruised from: `optionsUsed.args`, else `.`.
fn cruised_paths(session: &Session) -> Vec<String> {
    let args = session
        .warm
        .document()
        .and_then(|d| d.summary.options_used.get("args"))
        .and_then(Value::as_str)
        .map(|a| a.split_whitespace().map(str::to_owned).collect::<Vec<_>>())
        .unwrap_or_default();
    if args.is_empty() {
        vec![".".to_owned()]
    } else {
        args
    }
}

/// The re-check a save triggers: `cruise --cache -T json` over the cruised paths, its result
/// handed to the warm graph.
fn recheck(session: &mut Session) -> Result<(), String> {
    let mut line = vec![
        "cruise".to_owned(),
        "--cache".to_owned(),
        "-T".to_owned(),
        "json".to_owned(),
        "--no-progress".to_owned(),
    ];
    if let Some(file) = &session.warm.config().config {
        line.push("--config".into());
        if !file.is_empty() {
            line.push(file.clone());
        }
    }
    line.push("--".into());
    line.extend(cruised_paths(session));
    let outcome = session.with_context(false, |ctx| crate::run_in(ctx, &line));
    if outcome.code >= 2 {
        return Err(outcome.stderr.trim().to_owned());
    }
    let document = rb_ingest::dependency_cruiser::read(&outcome.stdout)
        .map_err(|e| format!("the re-check's result is not a cruise result: {e}"))?;
    // A context of its own, so the warm graph is not borrowed while it is replaced.
    let mut empty: &[u8] = &[];
    let ctx = crate::context::Context {
        cwd: session.cwd().to_path_buf(),
        stdin: &mut empty,
        today: chrono::NaiveDate::default(),
        timestamp: String::new(),
        color_terminal: false,
        warm: None,
    };
    session.warm.replace(&ctx, document);
    Ok(())
}

/// Publishes the diagnostics of every open file.
fn publish_all(
    session: &mut Session,
    state: &mut State,
    output: &mut dyn Write,
) -> std::io::Result<()> {
    evaluate(session, state);
    if let Some(message) = state.failure.take() {
        jsonrpc::write_framed(
            output,
            &jsonrpc::notification(
                "window/showMessage",
                json!({ "type": 1, "message": format!("rulebearing: {message}") }),
            ),
        )?;
    }
    let uris: Vec<(String, String)> = state
        .open
        .iter()
        .map(|(uri, file)| (uri.clone(), file.clone()))
        .collect();
    for (uri, file) in uris {
        publish(state, &uri, &file, output)?;
    }
    Ok(())
}

fn publish(state: &State, uri: &str, file: &str, output: &mut dyn Write) -> std::io::Result<()> {
    let diagnostics = state
        .evaluated
        .as_ref()
        .map(|(_, violations, document)| diagnostics(document, violations, file))
        .unwrap_or_default();
    jsonrpc::write_framed(
        output,
        &jsonrpc::notification(
            "textDocument/publishDiagnostics",
            json!({ "uri": uri, "diagnostics": diagnostics }),
        ),
    )
}

fn document_uri(params: &Value) -> Option<String> {
    params["textDocument"]["uri"].as_str().map(str::to_owned)
}

/// `workspace/executeCommand`: `rulebearing.explain <rule>` runs `explain` and shows its text,
/// which is also the result. `None` when the response, an error, was already written.
fn execute(
    session: &mut Session,
    incoming: &Incoming,
    output: &mut dyn Write,
) -> std::io::Result<Option<Value>> {
    let params = &incoming.params;
    let rule = params["arguments"][0]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if params["command"] != EXPLAIN_COMMAND || rule.is_empty() {
        if let Some(id) = &incoming.id {
            jsonrpc::write_framed(
                output,
                &jsonrpc::error(
                    id,
                    code::INVALID_PARAMS,
                    &format!("{EXPLAIN_COMMAND} takes the rule's name"),
                ),
            )?;
        }
        return Ok(None);
    }
    let mut line = vec!["explain".to_owned()];
    if let Some(file) = &session.warm.config().config {
        line.push("--config".into());
        if !file.is_empty() {
            line.push(file.clone());
        }
    }
    line.extend(["--".to_owned(), rule]);
    let outcome = session.run(&line);
    let text = if outcome.code >= 2 {
        outcome.stderr
    } else {
        outcome.stdout
    };
    jsonrpc::write_framed(
        output,
        &jsonrpc::notification(
            "window/showMessage",
            json!({ "type": 3, "message": text.trim_end() }),
        ),
    )?;
    Ok(Some(json!(text)))
}

/// Handles one message, writing its response and any notifications it causes; `false` once the
/// client asked the server to exit.
fn handle(
    session: &mut Session,
    state: &mut State,
    incoming: &Incoming,
    output: &mut dyn Write,
) -> std::io::Result<bool> {
    let params = &incoming.params;
    let response = match incoming.method.as_str() {
        "initialize" => Some(json!({
            "capabilities": {
                "textDocumentSync": { "openClose": true, "change": 0, "save": { "includeText": false } },
                "codeActionProvider": { "codeActionKinds": ["quickfix"] },
                "executeCommandProvider": { "commands": [EXPLAIN_COMMAND] },
            },
            "serverInfo": { "name": "rulebearing", "version": env!("CARGO_PKG_VERSION") },
        })),
        "initialized" | "textDocument/didChange" | "$/cancelRequest" | "$/setTrace" => None,
        "textDocument/didOpen" => {
            if let Some(uri) = document_uri(params)
                && let Some(file) = relative_path(&uri, session.cwd())
            {
                state.open.insert(uri.clone(), file.clone());
                evaluate(session, state);
                publish(state, &uri, &file, output)?;
            }
            None
        }
        "textDocument/didClose" => {
            if let Some(uri) = document_uri(params) {
                state.open.remove(&uri);
                jsonrpc::write_framed(
                    output,
                    &jsonrpc::notification(
                        "textDocument/publishDiagnostics",
                        json!({ "uri": uri, "diagnostics": [] }),
                    ),
                )?;
            }
            None
        }
        "textDocument/didSave" => {
            if let Err(message) = recheck(session) {
                state.failure = Some(message);
            }
            publish_all(session, state, output)?;
            None
        }
        "textDocument/codeAction" => {
            let carried = params["context"]["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            Some(Value::Array(code_actions(&carried)))
        }
        "workspace/executeCommand" => match execute(session, incoming, output)? {
            Some(result) => Some(result),
            None => return Ok(true),
        },
        "shutdown" => {
            state.shutdown = true;
            Some(Value::Null)
        }
        "exit" => return Ok(false),
        other => {
            if let Some(id) = &incoming.id {
                jsonrpc::write_framed(
                    output,
                    &jsonrpc::error(
                        id,
                        code::METHOD_NOT_FOUND,
                        &format!("rulebearing serve --lsp does not answer {other}"),
                    ),
                )?;
            }
            return Ok(true);
        }
    };
    if let (Some(id), Some(result)) = (&incoming.id, response) {
        jsonrpc::write_framed(output, &jsonrpc::result(id, result))?;
    }
    Ok(true)
}

/// Serves until `exit` or the end of the input; `true` when the client shut the server down
/// before it stopped, as the protocol asks.
///
/// # Errors
/// An I/O error on the streams.
pub fn serve(
    session: &mut Session,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
) -> std::io::Result<bool> {
    let mut state = State::default();
    while let Some(text) = jsonrpc::read_framed(input)? {
        match jsonrpc::parse(&text) {
            Ok(incoming) => {
                if !handle(session, &mut state, &incoming, output)? {
                    break;
                }
            }
            Err(error) => jsonrpc::write_framed(output, &error)?,
        }
    }
    Ok(state.shutdown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn uris_become_paths_relative_to_the_root() {
        let root = Path::new("/work/repo");
        let cases = [
            ("file:///work/repo/src/a.ts", Some("src/a.ts")),
            (
                "file:///work/repo/src/with%20space.ts",
                Some("src/with space.ts"),
            ),
            ("file:///elsewhere/b.ts", Some("/elsewhere/b.ts")),
            (
                "file:///work/repository/c.ts",
                Some("/work/repository/c.ts"),
            ),
            ("untitled:Untitled-1", None),
        ];
        for (uri, expected) in cases {
            assert_eq!(relative_path(uri, root).as_deref(), expected, "{uri}");
        }
        assert_eq!(
            relative_path("file:///C:/repo/src/a.ts", Path::new("C:/repo")).as_deref(),
            Some("src/a.ts")
        );
        assert_eq!(percent_decode("%41%2"), "A%2");
    }

    #[test]
    fn a_violation_is_a_diagnostic_with_its_fix_and_a_quick_fix_titled_with_it() {
        let violation = |rule: &str, severity: Severity, fix: Option<&str>| Violation {
            from: "src/a.ts".into(),
            to: "lib/b.ts".into(),
            rule: rb_model::RuleSummary {
                name: rule.into(),
                severity,
            },
            id: Some(format!("RB-{rule}")),
            fix: fix.map(str::to_owned),
            comment: Some("why".into()),
            unresolved_to: None,
            dependency_types: None,
            violation_type: None,
            cycle: None,
            via: None,
            metrics: None,
            decision: None,
        };
        let mut dependency =
            rb_model::Dependency::new("../lib/b", "lib/b.ts", rb_model::ModuleSystem::Es6);
        dependency.line = Some(3);
        let document = GraphDocument {
            modules: vec![rb_model::Module {
                dependencies: vec![dependency],
                ..rb_model::Module::new("src/a.ts")
            }],
            ..GraphDocument::default()
        };
        let violations = [
            violation("with-fix", Severity::Error, Some("Do this")),
            violation("no-fix", Severity::Warn, None),
            violation("quiet", Severity::Ignore, Some("never")),
            Violation {
                from: "src/other.ts".into(),
                ..violation("elsewhere", Severity::Info, None)
            },
        ];
        let found = diagnostics(&document, &violations, "src/a.ts");
        assert_eq!(
            found.len(),
            2,
            "ignore is not reported, another file's is not"
        );
        assert_eq!(found[0]["message"], "with-fix: Do this");
        assert_eq!(found[0]["code"], "RB-with-fix");
        assert_eq!(found[0]["source"], "rulebearing");
        assert_eq!(found[0]["severity"], 1);
        assert_eq!(
            found[0]["range"]["start"]["line"], 2,
            "the import's line, zero-based"
        );
        assert_eq!(
            found[1]["message"], "no-fix: why",
            "the comment when there is no fix"
        );
        assert_eq!(found[1]["severity"], 2);
        let info = diagnostics(&document, &violations, "src/other.ts");
        assert_eq!(info[0]["severity"], 3);
        assert_eq!(
            info[0]["range"]["start"]["line"], 0,
            "no import recorded: the first line"
        );

        let mut carried = found.clone();
        carried.push(json!({ "source": "other-linter", "message": "x" }));
        let actions = code_actions(&carried);
        assert_eq!(actions.len(), 2, "only this server's diagnostics");
        assert_eq!(actions[0]["title"], "Do this");
        assert_eq!(actions[0]["kind"], "quickfix");
        assert_eq!(actions[0]["command"]["command"], EXPLAIN_COMMAND);
        assert_eq!(actions[0]["command"]["arguments"], json!(["with-fix"]));
        assert_eq!(actions[1]["title"], "why");
        let bare = json!({ "source": "rulebearing", "data": { "rule": "r", "fix": null } });
        assert_eq!(code_actions(&[bare])[0]["title"], "Explain r");
    }
}
