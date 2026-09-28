//! `-T plugin:<path>`: a reporter written in JavaScript, run in the configuration sandbox.
//!
//! - Architecture: [architecture § Security posture](../../../docs/architecture.md#security-posture),
//!   [§ Outputs and CI contract](../../../docs/architecture.md#outputs-and-ci-contract)
//! - Decisions: [ADR-0006](../../../docs/adr/0006-embedded-quickjs-config-evaluator.md),
//!   [ADR-0008](../../../docs/adr/0008-exit-code-contract.md),
//!   [ADR-0030](../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md),
//!   [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//! - Plan: [Wave 3, Step 7](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//! - Requirements: [FR-OUT-01](../../../docs/prd.md#fr-out-01), [NFR-SEC-01](../../../docs/prd.md#nfr-sec-01)
//!
//! The sandbox itself is `rb_config::js::plugin`; this module is the orchestration `cruise`,
//! `fmt` and the conformance protocol share. It lives in `rb-cli`, not in `rb-report`, because a
//! plugin needs what only the command line has: the working directory relative paths start from,
//! the repository root the sandbox is confined to, and the decision on the exit code. `rb-report`
//! stays a set of pure functions from a result to text, and gains no edge to `rb-config`.
//!
//! Three decisions:
//!
//! - **Exit code.** dependency-cruiser's command line exits with whatever `exitCode` the reporter
//!   returns, a plugin's included (`src/cli/index.mjs`); ADR-0030 makes the violation part of the
//!   exit code the reporter's. So a plugin's `exitCode` is the count, `--exit-code-mode strict`
//!   shifts it like any other, and 2 and 3 keep their meaning whatever the plugin says. `fmt`
//!   without `--exit-code` exits 0, as for every reporter. An `exitCode` that is not a whole
//!   number of zero or more cannot be a process exit code and is refused.
//! - **Errors are exit 3.** A plugin is code the command line or the configuration names, like a
//!   JavaScript configuration, whose every evaluation error is exit 3 ([`rb_config::js::JsError`]):
//!   a missing plugin, an invalid one, a sandbox refusal, a time or memory overrun and a plugin
//!   that throws all mean the configured reporter cannot run, and each message names the plugin
//!   and the fix.
//! - **Receipt.** The result a plugin receives carries `summary.plugins: [path]`, the module
//!   relative to the repository root, so what rendered a report is on the record
//!   ([plan § 1.7](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#17-quality-attributes)).
//!   `--strict-schema` hands the plugin dependency-cruiser's shape instead, with every addition,
//!   the receipt included, stripped.

use std::path::Path;

use rb_config::js::Limits;
use rb_config::js::plugin::{Plugin, PluginError, Sandbox};
use serde_json::{Value, json};

/// Why a plugin report could not be made. Every variant is exit 3.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Failure {
    /// The plugin could not be found, loaded, checked or called.
    #[error(transparent)]
    Plugin(#[from] PluginError),
    /// The plugin's `exitCode` is not a process exit count.
    #[error(
        "plugin:{name} returned exitCode {code}, which is not a whole number of zero or more; fix the plugin to return the count it gates on"
    )]
    ExitCode {
        /// The name after `plugin:`.
        name: String,
        /// What it returned.
        code: f64,
    },
}

/// The sandbox a plugin runs in from `cwd`: confined to the repository `cwd` belongs to.
pub fn sandbox(cwd: &Path) -> Sandbox {
    Sandbox::new(
        &rb_config::load::repository_root(cwd),
        cwd,
        Limits::plugin(),
    )
}

/// The plugin's module relative to the repository root, `/`-separated: the receipt's entry.
pub fn receipt_path(sandbox: &Sandbox, plugin: &Plugin) -> String {
    plugin
        .file()
        .strip_prefix(sandbox.root())
        .unwrap_or(plugin.file())
        .to_string_lossy()
        .replace('\\', "/")
}

/// Renders `result` through the plugin named `name` (the text after `plugin:`), first recording
/// it in `summary.plugins`, or stripping every addition under `strict_schema`.
///
/// # Errors
/// [`Failure`]; the caller exits 3.
pub fn render(
    cwd: &Path,
    name: &str,
    result: &mut Value,
    strict_schema: bool,
) -> Result<rb_report::Rendered, Failure> {
    let sandbox = sandbox(cwd);
    let plugin = sandbox.resolve(name)?;
    if strict_schema {
        rb_report::json::strip(result);
    } else if let Some(summary) = result.get_mut("summary").and_then(Value::as_object_mut) {
        summary.insert("plugins".into(), json!([receipt_path(&sandbox, &plugin)]));
    }
    let output = sandbox.report(&plugin, result)?;
    let exit_code = output.whole_exit_code().ok_or(Failure::ExitCode {
        name: name.to_owned(),
        code: output.exit_code,
    })?;
    Ok(rb_report::Rendered {
        output: output.output,
        exit_code,
    })
}

/// The conformance protocol's answer for `#report/plugins.mjs`: `getExternalPluginReporter` loads
/// and checks the plugin an output type names (`false` when it names none), and `isValidPlugin`
/// checks a module the harness hands over by its `{ "plugin": <file URL> }` handle (`false` for
/// anything else, as upstream answers for a value that is not a function). The value is the
/// `result` of the reply.
///
/// # Errors
/// [`PluginError`] as upstream throws it, or a message for an export the module does not have.
pub fn protocol(cwd: &Path, export: &str, argument: Option<&Value>) -> Result<Value, String> {
    let sandbox = sandbox(cwd);
    match export {
        "getExternalPluginReporter" => {
            let Some(name) = argument
                .and_then(Value::as_str)
                .and_then(rb_config::js::plugin::plugin_name)
            else {
                return Ok(Value::Bool(false));
            };
            let plugin = sandbox.resolve(name).map_err(|e| e.to_string())?;
            sandbox.check(&plugin).map_err(|e| e.to_string())?;
            Ok(json!({ "plugin": receipt_path(&sandbox, &plugin) }))
        }
        "isValidPlugin" => {
            let Some(name) = argument
                .and_then(|a| a.get("plugin"))
                .and_then(Value::as_str)
            else {
                return Ok(Value::Bool(false));
            };
            let plugin = sandbox.resolve(name).map_err(|e| e.to_string())?;
            sandbox
                .is_valid(&plugin)
                .map(Value::Bool)
                .map_err(|e| e.to_string())
        }
        other => Err(format!(
            "#report/plugins.mjs has no export `{other}`; it exports getExternalPluginReporter and isValidPlugin"
        )),
    }
}

/// A plugin's `exitCode` as the protocol returns it: the number, whole when it is one.
pub fn exit_code_json(output: &rb_config::js::plugin::PluginOutput) -> Value {
    output
        .whole_exit_code()
        .map_or_else(|| json!(output.exit_code), |code| json!(code))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_config::js::plugin::PluginOutput;

    struct Repo(std::path::PathBuf);

    impl Repo {
        fn new(files: &[(&str, &str)]) -> std::io::Result<Self> {
            static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("rb-cli-plugin-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(path.join(".git"))?;
            for (name, text) in files {
                let file = path.join(name);
                if let Some(parent) = file.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(file, text)?;
            }
            Ok(Self(path.canonicalize()?))
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const ECHO: &str = "module.exports = (r) => ({ output: JSON.stringify(r.summary.plugins ?? null) + ':' + Object.keys(r.summary).join(','), exitCode: r.summary.error });";

    #[test]
    fn render_records_the_receipt_or_strips_it() -> Result<(), Box<dyn std::error::Error>> {
        let repo = Repo::new(&[
            ("tools/echo.cjs", ECHO),
            (
                "tools/fraction.cjs",
                "module.exports = (r) => ({ output: '', exitCode: r.modules ? 0.5 : 0 });",
            ),
        ])?;
        let result = json!({ "modules": [], "summary": { "error": 3, "inspected": {} } });
        let nested = repo.0.join("tools");
        let mut value = result.clone();
        let rendered = render(&nested, "./echo.cjs", &mut value, false)?;
        assert_eq!(
            rendered,
            rb_report::Rendered {
                output: r#"["tools/echo.cjs"]:error,inspected,plugins"#.into(),
                exit_code: 3
            }
        );
        let mut strict = result.clone();
        let rendered = render(&nested, "./echo.cjs", &mut strict, true)?;
        assert_eq!(rendered.output, "null:error");
        let mut again = result;
        let fraction = render(&repo.0, "./tools/fraction.cjs", &mut again, false);
        assert_eq!(
            fraction,
            Err(Failure::ExitCode {
                name: "./tools/fraction.cjs".into(),
                code: 0.5
            })
        );
        assert!(
            fraction
                .err()
                .map(|e| e.to_string())
                .is_some_and(|m| m.contains("not a whole number"))
        );
        let missing = render(&repo.0, "./nope.cjs", &mut json!({}), false);
        assert!(matches!(
            missing,
            Err(Failure::Plugin(PluginError::NotFound { .. }))
        ));
        Ok(())
    }

    #[test]
    fn the_protocol_answers_as_upstreams_plugins_module() -> Result<(), Box<dyn std::error::Error>>
    {
        let repo = Repo::new(&[
            (
                "ok.cjs",
                "module.exports = () => ({ output: '', exitCode: 0 });",
            ),
            ("bad.cjs", "module.exports = () => ({ output: '' });"),
        ])?;
        let cwd = repo.0.as_path();
        let ask = |export: &str, argument: Value| protocol(cwd, export, Some(&argument));
        assert_eq!(
            ask("getExternalPluginReporter", json!("err")),
            Ok(json!(false))
        );
        assert_eq!(
            protocol(cwd, "getExternalPluginReporter", None),
            Ok(json!(false))
        );
        assert_eq!(
            ask("getExternalPluginReporter", json!("plugin:./ok.cjs")),
            Ok(json!({ "plugin": "ok.cjs" }))
        );
        let bad = ask("getExternalPluginReporter", json!("plugin:./bad.cjs"));
        assert!(
            bad.as_ref()
                .is_err_and(|m| m.starts_with("./bad.cjs is not a valid plugin")),
            "{bad:?}"
        );
        let url = format!("file://{}", repo.0.join("bad.cjs").to_string_lossy());
        assert_eq!(
            ask("isValidPlugin", json!({ "plugin": url })),
            Ok(json!(false))
        );
        assert_eq!(
            ask("isValidPlugin", json!({ "plugin": "./ok.cjs" })),
            Ok(json!(true))
        );
        assert_eq!(
            ask("isValidPlugin", json!("not a handle")),
            Ok(json!(false))
        );
        assert!(ask("isValidPlugin", json!({ "plugin": "./gone.cjs" })).is_err());
        assert!(ask("getPluginReporter", json!(null)).is_err_and(|m| m.contains("no export")));
        Ok(())
    }

    #[test]
    fn protocol_exit_codes_keep_their_number() {
        let output = |exit_code: f64| PluginOutput {
            output: String::new(),
            exit_code,
        };
        assert_eq!(exit_code_json(&output(0.0)).to_string(), "0");
        assert_eq!(exit_code_json(&output(42.0)).to_string(), "42");
        assert_eq!(exit_code_json(&output(1.5)).to_string(), "1.5");
        assert_eq!(exit_code_json(&output(f64::NAN)), Value::Null);
    }
}
