//! `rulebearing hooks install --claude-code`: the three Claude Code hooks, merged into
//! `.claude/settings.json`.
//!
//! - Contract: [Wave 1 plan § 1.5](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#15-interfaces-and-contracts-frozen-by-this-wave)
//!   (`SessionStart`, `PreToolUse` on Edit and Write, `Stop`)
//! - Source: [design § Hooks, test runners, an MCP server, an LSP](../../../../docs/artifacts/design.md#hooks-test-runners-an-mcp-server-an-lsp)
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Plan: [Wave 1, Step 15](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-15-hooks-install---claude-code-summary---format-agent-impact-attest---require-comment-token-1e)
//! - Requirement: [FR-CLI-03](../../../../docs/prd.md#fr-cli-03)
//!
//! The file is merged, never overwritten: other settings and other hooks stay, and installing twice
//! adds nothing. `impact` reads the file being edited from the hook's JSON on stdin.

use clap::{Args, Subcommand};
use serde_json::{Map, Value, json};

use crate::context::Context;
use crate::{Outcome, RunExit};

/// `hooks`.
#[derive(Debug, Clone, Subcommand)]
pub enum HooksCommand {
    /// Install the hooks
    Install(InstallArgs),
}

/// `hooks install`.
#[derive(Debug, Clone, Args)]
pub struct InstallArgs {
    /// Install Claude Code's hooks into .claude/settings.json
    #[arg(long)]
    pub claude_code: bool,
}

/// The session brief.
pub const SESSION_START: &str = "rulebearing summary --format agent";
/// Before an edit: what the file is subject to.
pub const PRE_TOOL_USE: &str = "rulebearing impact --from-hook";
/// Before the turn ends: the findings, for the agent (wave 3 narrows it to `--affected HEAD`).
pub const STOP: &str = "rulebearing cruise --output-type agent";

fn command(command: &str) -> Value {
    json!({ "type": "command", "command": command })
}

/// Adds `entry` to `hooks[event]` unless an entry running the same command is already there.
fn merge_event(hooks: &mut Map<String, Value>, event: &str, entry: Value) {
    let wanted = entry["hooks"][0]["command"].clone();
    let list = hooks.entry(event.to_owned()).or_insert_with(|| json!([]));
    let Value::Array(list) = list else { return };
    let present = list.iter().any(|e| {
        e.get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|h| h.iter().any(|x| x.get("command") == Some(&wanted)))
    });
    if !present {
        list.push(entry);
    }
}

/// Merges the three hooks into a settings object.
pub fn merged(settings: Value) -> Value {
    let mut settings = match settings {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let hooks = settings.entry("hooks").or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    if let Value::Object(hooks) = hooks {
        merge_event(
            hooks,
            "SessionStart",
            json!({ "hooks": [command(SESSION_START)] }),
        );
        merge_event(
            hooks,
            "PreToolUse",
            json!({ "matcher": "Edit|Write", "hooks": [command(PRE_TOOL_USE)] }),
        );
        merge_event(hooks, "Stop", json!({ "hooks": [command(STOP)] }));
    }
    Value::Object(settings)
}

/// Runs `hooks`.
pub fn run(ctx: &mut Context<'_>, command: &HooksCommand) -> Outcome {
    let HooksCommand::Install(args) = command;
    if !args.claude_code {
        return Outcome::failed(
            RunExit::InvalidConfig,
            "rulebearing hooks install: name the agent, --claude-code\n",
        );
    }
    let path = ctx.resolve(".claude/settings.json");
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                return Outcome::failed(
                    RunExit::InvalidConfig,
                    format!(
                        "rulebearing hooks: {} is not JSON: {e}; fix it and run again\n",
                        path.display()
                    ),
                );
            }
        },
        Err(_) => json!({}),
    };
    let settings = merged(existing);
    let mut text = serde_json::to_string_pretty(&settings).unwrap_or_default();
    text.push('\n');
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return Outcome::failed(RunExit::Untrustworthy, format!("rulebearing hooks: {e}\n"));
    }
    match std::fs::write(&path, text) {
        Ok(()) => Outcome::printed(format!(
            "installed in {}:\n  SessionStart  {SESSION_START}\n  PreToolUse    {PRE_TOOL_USE}  (Edit, Write)\n  Stop          {STOP}\n",
            path.display()
        )),
        Err(e) => Outcome::failed(
            RunExit::Untrustworthy,
            format!("rulebearing hooks: cannot write {}: {e}\n", path.display()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merging_is_idempotent_and_keeps_other_settings() {
        let existing = json!({ "model": "x", "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "other" }] }] } });
        let once = merged(existing);
        let twice = merged(once.clone());
        assert_eq!(once, twice);
        assert_eq!(once["model"], "x");
        assert_eq!(once["hooks"]["Stop"].as_array().map(Vec::len), Some(2));
        assert_eq!(once["hooks"]["PreToolUse"][0]["matcher"], "Edit|Write");
        assert_eq!(
            once["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            SESSION_START
        );
        assert_eq!(
            merged(json!([]))["hooks"]["Stop"][0]["hooks"][0]["command"],
            STOP
        );
        assert_eq!(
            merged(json!({ "hooks": 3 }))["hooks"]["Stop"][0]["hooks"][0]["command"],
            STOP
        );
    }
}
