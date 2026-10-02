//! The conformance harness's protocols: `validate` (gate 1 layer 2) and `report` (layer 3).
//!
//! - Protocol: `conformance/dependency-cruiser/harness/shim.mjs`
//! - Decision: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md)
//! - Plan: [Wave 1, Step 8](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-8-rulebearing-validate-for-gate-1-layer-2-1b),
//!   [Step 12](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-12-reporters-1d)
//! - Requirement: [NFR-CONF-01](../../../docs/prd.md#nfr-conf-01)
//!
//! `#report/plugins.mjs` and `report -T plugin:<path>` are answered by [`crate::plugin`], so the
//! plugin fixtures of dependency-cruiser's specs run in Rulebearing's sandbox, from the working
//! directory the harness runs in
//! ([Wave 3, Step 7](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)).

use std::path::Path;

use serde_json::Value;

use crate::{Outcome, RunExit};

/// The module whose exports [`crate::plugin::protocol`] answers.
const PLUGINS_MODULE: &str = "#report/plugins.mjs";

/// `rulebearing validate`: one engine request on stdin, the answer on stdout. A request for a
/// reporter module (`#report/...`) is answered by the reporters, any other by the engine.
pub fn validate(cwd: &Path, stdin: &str) -> Outcome {
    let request = serde_json::from_str::<Value>(stdin).unwrap_or(Value::Null);
    let module = request
        .get("module")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if module == PLUGINS_MODULE {
        let export = request
            .get("export")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let argument = request.pointer("/calls/0/0").filter(|v| !v.is_null());
        return match crate::plugin::protocol(cwd, export, argument) {
            Ok(result) => Outcome::printed(serde_json::json!({ "result": result }).to_string()),
            Err(error) => Outcome::failed(
                RunExit::InvalidConfig,
                format!("rulebearing validate: {error}\n"),
            ),
        };
    }
    let answered = if rb_report::conformance::handles(&module) {
        rb_report::conformance::answer(stdin)
    } else {
        rb_rules::conformance::answer(stdin)
    };
    match answered {
        Ok(reply) => Outcome::printed(reply),
        Err(error) => Outcome::failed(
            RunExit::InvalidConfig,
            format!("rulebearing validate: {error}\n"),
        ),
    }
}

/// `rulebearing report --output-type <type>`: `{ result, options }` on stdin, the reporter's
/// `{ output, exitCode }` on stdout.
pub fn report(cwd: &Path, output_type: &str, stdin: &str) -> Outcome {
    let request: Value = match serde_json::from_str(stdin) {
        Ok(v) => v,
        Err(e) => {
            return Outcome::failed(
                RunExit::InvalidConfig,
                format!("rulebearing report: not a request: {e}\n"),
            );
        }
    };
    let result = request.get("result").cloned().unwrap_or(Value::Null);
    if let Some(name) = rb_config::js::plugin::plugin_name(output_type) {
        // A spec calls the plugin with its own result, as upstream's reporter function is called:
        // no receipt is added.
        let sandbox = crate::plugin::sandbox(cwd);
        let rendered = sandbox
            .resolve(name)
            .and_then(|plugin| sandbox.report(&plugin, &result));
        return match rendered {
            Ok(output) => Outcome::printed(
                serde_json::json!({
                    "output": output.output,
                    "exitCode": crate::plugin::exit_code_json(&output),
                })
                .to_string(),
            ),
            Err(e) => Outcome::failed(RunExit::InvalidConfig, format!("rulebearing report: {e}\n")),
        };
    }
    let section = request.get("options").filter(|o| !o.is_null());
    // `x-dot-webpage`: the answers of the spec's `spawnFunction` when it passes one, else the
    // `dot` on PATH, as upstream's reporter uses (ADR-0053).
    let graphviz = match crate::graphviz::Answers::from_options(section) {
        Some(answers) => rb_report::dot_webpage::GraphvizRunner(std::sync::Arc::new(answers)),
        None => crate::graphviz::system(),
    };
    let options = rb_report::ReportOptions {
        timestamp: "1970-01-01T00:00:00.000".into(),
        graphviz: Some(graphviz),
        ..rb_report::ReportOptions::default()
    };
    match rb_report::render_with(output_type, &result, &options, section) {
        Ok(rendered) => Outcome::printed(
            serde_json::json!({ "output": rendered.output, "exitCode": rendered.exit_code })
                .to_string(),
        ),
        Err(e) => Outcome::failed(RunExit::InvalidConfig, format!("rulebearing report: {e}\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocols_answer_or_fail() {
        let cwd = std::env::temp_dir();
        let ok = validate(
            &cwd,
            r##"{"module":"#graph-utl/compare.mjs","export":"compareSeverities","calls":[["warn","warn"]]}"##,
        );
        assert_eq!(ok.stdout, r#"{"result":0}"#);
        assert_eq!(validate(&cwd, "nope").code, 3);
        let rendered = report(&cwd, "null", r#"{"result":{"summary":{"error":4}}}"#);
        let parsed: Value = serde_json::from_str(&rendered.stdout).unwrap_or(Value::Null);
        assert_eq!(parsed, serde_json::json!({ "exitCode": 4, "output": "" }));
        // `x-dot-webpage` with the answers of a spec's `spawnFunction`: no GraphViz.
        let missing = report(
            &cwd,
            "x-dot-webpage",
            r#"{"result":{},"options":{"spawnFunction":{"version":{"status":1,"stderr":"not found"},"convert":{}}}}"#,
        );
        assert_eq!(missing.code, 3);
        assert!(
            missing.stderr.contains("GraphViz dot, which is required"),
            "{}",
            missing.stderr
        );
        let drawn = report(
            &cwd,
            "x-dot-webpage",
            r#"{"result":{},"options":{"spawnFunction":{"version":{"status":0,"stderr":"dot - graphviz version 9"},"convert":{"status":0,"stdout":"<svg/>"}}}}"#,
        );
        let page: Value = serde_json::from_str(&drawn.stdout).unwrap_or(Value::Null);
        assert_eq!(
            page["output"],
            serde_json::json!(rb_report::dot_webpage::wrap_in_html("<svg/>"))
        );
        let reporter = validate(
            &cwd,
            r##"{"module":"#report/dot/module-utl.mjs","export":"attributizeObject","calls":[[{"a":1}]]}"##,
        );
        assert_eq!(reporter.stdout, r#"{"result":"a=\"1\""}"#);
        assert_eq!(report(&cwd, "err", "{").code, 3);
    }

    #[test]
    fn plugin_requests_run_in_the_sandbox() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("rb-protocol-plugin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git"))?;
        std::fs::write(
            dir.join("count.cjs"),
            "module.exports = (r) => ({ output: String(r.modules.length), exitCode: 7 });",
        )?;
        let not_plugin = validate(
            &dir,
            r##"{"module":"#report/plugins.mjs","export":"getExternalPluginReporter","calls":[["err"]]}"##,
        );
        assert_eq!(not_plugin.stdout, r#"{"result":false}"#);
        let valid = validate(
            &dir,
            r##"{"module":"#report/plugins.mjs","export":"isValidPlugin","calls":[[{"plugin":"./count.cjs"}]]}"##,
        );
        assert_eq!(valid.stdout, r#"{"result":true}"#);
        let missing = validate(
            &dir,
            r##"{"module":"#report/plugins.mjs","export":"getExternalPluginReporter","calls":[["plugin:this-plugin-does-not-exist"]]}"##,
        );
        assert_eq!(missing.code, 3);
        assert!(
            missing
                .stderr
                .contains("Could not find reporter plugin 'this-plugin-does-not-exist'"),
            "{}",
            missing.stderr
        );
        let rendered = report(
            &dir,
            "plugin:./count.cjs",
            r#"{"result":{"modules":[{},{}],"summary":{}}}"#,
        );
        assert_eq!(rendered.stdout, r#"{"output":"2","exitCode":7}"#);
        let refused = report(&dir, "plugin:../../etc/passwd", r#"{"result":{}}"#);
        assert_eq!(refused.code, 3);
        assert!(refused.stderr.contains("the reporter sandbox refused it"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
