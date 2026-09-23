//! The conformance harness's protocols: `validate` (gate 1 layer 2) and `report` (layer 3).
//!
//! - Protocol: `conformance/dependency-cruiser/harness/shim.mjs`
//! - Decision: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md)
//! - Plan: [Wave 1, Step 8](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-8-rulebearing-validate-for-gate-1-layer-2-1b),
//!   [Step 12](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-12-reporters-1d)
//! - Requirement: [NFR-CONF-01](../../../docs/prd.md#nfr-conf-01)

use serde_json::Value;

use crate::{Outcome, RunExit};

/// `rulebearing validate`: one engine request on stdin, the answer on stdout.
pub fn validate(stdin: &str) -> Outcome {
    match rb_rules::conformance::answer(stdin) {
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
    let options = rb_report::ReportOptions {
        timestamp: "1970-01-01T00:00:00.000".into(),
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
        assert_eq!(report("dot", r#"{"result":{}}"#).code, 3);
        assert_eq!(report("err", "{").code, 3);
    }
}
