//! The conformance harness's protocols: `validate` (gate 1 layer 2) and `report` (layer 3).
//!
//! - Protocol: `conformance/dependency-cruiser/harness/shim.mjs`
//! - Decision: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md)
//! - Plan: [Wave 1, Step 8](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-8-rulebearing-validate-for-gate-1-layer-2-1b),
//!   [Step 12](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-12-reporters-1d)
//! - Requirement: [NFR-CONF-01](../../../docs/prd.md#nfr-conf-01)

use serde_json::Value;

use crate::{Outcome, RunExit};

/// `rulebearing validate`: one engine request on stdin, the answer on stdout. A request for a
/// reporter module (`#report/...`) is answered by the reporters, any other by the engine.
pub fn validate(stdin: &str) -> Outcome {
    let module = serde_json::from_str::<Value>(stdin)
        .ok()
        .and_then(|r| r.get("module").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default();
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
pub fn report(output_type: &str, stdin: &str) -> Outcome {
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
        let ok = validate(
            r##"{"module":"#graph-utl/compare.mjs","export":"compareSeverities","calls":[["warn","warn"]]}"##,
        );
        assert_eq!(ok.stdout, r#"{"result":0}"#);
        assert_eq!(validate("nope").code, 3);
        let rendered = report("null", r#"{"result":{"summary":{"error":4}}}"#);
        let parsed: Value = serde_json::from_str(&rendered.stdout).unwrap_or(Value::Null);
        assert_eq!(parsed, serde_json::json!({ "exitCode": 4, "output": "" }));
        // `x-dot-webpage` with the answers of a spec's `spawnFunction`: no GraphViz.
        let missing = report(
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
            "x-dot-webpage",
            r#"{"result":{},"options":{"spawnFunction":{"version":{"status":0,"stderr":"dot - graphviz version 9"},"convert":{"status":0,"stdout":"<svg/>"}}}}"#,
        );
        let page: Value = serde_json::from_str(&drawn.stdout).unwrap_or(Value::Null);
        assert_eq!(
            page["output"],
            serde_json::json!(rb_report::dot_webpage::wrap_in_html("<svg/>"))
        );
        let reporter = validate(
            r##"{"module":"#report/dot/module-utl.mjs","export":"attributizeObject","calls":[[{"a":1}]]}"##,
        );
        assert_eq!(reporter.stdout, r#"{"result":"a=\"1\""}"#);
        assert_eq!(report("err", "{").code, 3);
    }
}
