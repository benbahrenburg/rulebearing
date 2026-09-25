//! `--config-via-node`: the explicit way out of the sandbox.
//!
//! - Decision: [ADR-0006](../../../../docs/adr/0006-embedded-quickjs-config-evaluator.md)
//! - Plan: [Wave 1, Step 2](../../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-2-the-quickjs-evaluator-1a)
//! - Requirement: [FR-CFG-03](../../../../docs/prd.md#fr-cfg-03)
//!
//! A configuration that needs Node (one that reads the filesystem, say) is evaluated by a local
//! `node`, which imports the file and prints the evaluated object as JSON. The run records
//! `viaNode: true` in `optionsUsed`, so a report says the configuration was not hermetic.

use std::path::Path;
use std::process::Command;

use super::JsError;

/// The script `node` runs: import the file by URL and print its default export as JSON.
const SCRIPT: &str = r#"
const { pathToFileURL } = await import("node:url");
const loaded = await import(pathToFileURL(process.argv[1]).href);
const value = loaded.default ?? loaded;
process.stdout.write(JSON.stringify(value));
"#;

/// The script `node` runs for a webpack configuration: import it, apply
/// [`super::WEBPACK_PICK`] with the `env` and `arguments` given as JSON, print the `resolve` block.
fn webpack_script() -> String {
    format!(
        r#"
const {{ pathToFileURL }} = await import("node:url");
const loaded = await import(pathToFileURL(process.argv[1]).href);
const pick = {pick};
const value = pick(loaded.default ?? loaded, JSON.parse(process.argv[2]), JSON.parse(process.argv[3]));
process.stdout.write(JSON.stringify(value));
"#,
        pick = super::WEBPACK_PICK
    )
}

/// Evaluates the webpack configuration at `entry` with the local Node and returns its `resolve`
/// block, as [`super::evaluate_webpack`] does in the sandbox.
///
/// # Errors
/// See [`evaluate`].
pub fn evaluate_webpack(
    entry: &Path,
    call: super::WebpackCall<'_>,
) -> Result<serde_json::Value, JsError> {
    let node = std::env::var("RULEBEARING_NODE").unwrap_or_else(|_| "node".to_owned());
    let json = |v: &serde_json::Value| serde_json::to_string(v).unwrap_or_else(|_| "null".into());
    run(
        &node,
        entry,
        &webpack_script(),
        &[json(call.env), json(call.arguments)],
    )
}

/// Evaluates `entry` with the `node` found on the path (or `$RULEBEARING_NODE`).
///
/// # Errors
/// [`JsError::Thrown`] when Node is missing, exits non-zero, or prints something that is not
/// JSON.
pub fn evaluate(entry: &Path) -> Result<serde_json::Value, JsError> {
    let node = std::env::var("RULEBEARING_NODE").unwrap_or_else(|_| "node".to_owned());
    evaluate_with(&node, entry)
}

/// [`evaluate`] with a named Node executable.
///
/// # Errors
/// See [`evaluate`].
pub fn evaluate_with(node: &str, entry: &Path) -> Result<serde_json::Value, JsError> {
    run(node, entry, SCRIPT, &[])
}

/// Runs `script` with `node`, `entry` and `extra` as its arguments, and reads its output as JSON.
fn run(
    node: &str,
    entry: &Path,
    script: &str,
    extra: &[String],
) -> Result<serde_json::Value, JsError> {
    let thrown = |message: String| JsError::Thrown {
        file: entry.to_path_buf(),
        message,
    };
    let dir = entry.parent().unwrap_or(Path::new("."));
    let output = Command::new(node)
        .arg("--input-type=module")
        .arg("-e")
        .arg(script)
        .arg(entry)
        .args(extra)
        .current_dir(dir)
        .output()
        .map_err(|e| thrown(format!("--config-via-node could not start `{node}`: {e}")))?;
    if !output.status.success() {
        return Err(thrown(format!(
            "--config-via-node: node exited {}: {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| thrown(format!("--config-via-node printed invalid JSON: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_node_is_named() {
        let error = evaluate_with("rulebearing-no-such-node", Path::new("/x/c.cjs"))
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(error.contains("could not start"), "{error}");
    }

    #[test]
    fn a_failing_config_reports_node_s_message() -> Result<(), Box<dyn std::error::Error>> {
        if Command::new("node").arg("--version").output().is_err() {
            return Ok(());
        }
        let dir = std::env::temp_dir().join(format!("rb-via-node-fail-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let file = dir.join("c.cjs");
        std::fs::write(&file, "throw new Error('nope');")?;
        let error = evaluate(&file)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(error.contains("nope"), "{error}");
        Ok(())
    }

    #[test]
    fn node_evaluates_a_webpack_config_when_present() -> Result<(), Box<dyn std::error::Error>> {
        if Command::new("node").arg("--version").output().is_err() {
            return Ok(());
        }
        let dir = std::env::temp_dir().join(format!("rb-via-node-webpack-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let file = dir.join("webpack.config.cjs");
        std::fs::write(
            &file,
            "const path = require('node:path'); module.exports = (env, argv) => ({ resolve: { alias: { '@': path.join('/r', env.dir) }, extensions: [argv.mode] } });",
        )?;
        let env = serde_json::json!({ "dir": "src" });
        let arguments = serde_json::json!({ "mode": ".ts" });
        let value = evaluate_webpack(
            &file,
            super::super::WebpackCall {
                env: &env,
                arguments: &arguments,
            },
        )?;
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            value,
            serde_json::json!({ "alias": { "@": "/r/src" }, "extensions": [".ts"] })
        );
        Ok(())
    }

    #[test]
    fn node_evaluates_a_config_when_present() -> Result<(), Box<dyn std::error::Error>> {
        if Command::new("node").arg("--version").output().is_err() {
            return Ok(());
        }
        let dir = std::env::temp_dir().join(format!("rb-via-node-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let file = dir.join("c.cjs");
        std::fs::write(
            &file,
            "const fs = require('fs'); module.exports = { forbidden: [], options: { n: fs.existsSync(__filename) } };",
        )?;
        let value = evaluate(&file)?;
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(value["options"]["n"], true);
        Ok(())
    }
}
