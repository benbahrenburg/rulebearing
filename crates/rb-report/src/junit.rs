//! `junit`: one test case per rule, so a rule failure shows in the test tab of any CI.
//!
//! - Contract: [Wave 2 plan § 1.5](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#15-interfaces-and-contracts-this-wave-freezes)
//!   (one test case per rule; the failure message carries the `fix` and the first violations
//!   with id, from, to and line; the test adapters' message is this text)
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//!   (the receipt as a property; vacuous rules as errors, not failures)
//! - Source: [design § Reporters](../../../docs/artifacts/design.md#reporters)
//! - Decisions: [ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md),
//!   [ADR-0015](../../../docs/adr/0015-stable-violation-id.md)
//! - Requirement: [FR-OUT-02](../../../docs/prd.md#fr-out-02)
//! - Specification: the Jenkins xUnit plugin's `junit-10.xsd`, vendored in `tests/schemas/`,
//!   which `tests/xml_schemas.rs` validates every output against
//!
//! `<testsuites>` holds one `<testsuite name="rulebearing">`, whose `<properties>` are the receipt
//! (`summary.inspected`, flattened, and the counts). Each rule of the run is a `<testcase>` whose
//! `classname` is `rulebearing.<family>` ([`crate::catalog`]). An error-severity violation fails
//! it: `<failure type="error">` with the message the test adapters print (the `fix`, then the
//! first five violations) and every violation in the body, one per object for an element rule. A
//! vacuous rule, an expired rule and a ratchet without a budget are `<error>`s, because the rule
//! could not be checked, not because the code broke it. Warn, info and known findings are listed
//! in `<system-out>` without failing the case. An expired known violation is a case of its own.
//! Exits 0, as the data reporters do.

use std::fmt::Write as _;

use serde_json::Value;

use crate::Rendered;
use crate::catalog::{self, xml};

/// Renders `junit`. `timestamp` is the run's, ISO 8601 without a zone; empty leaves it out.
pub fn render(result: &Value, timestamp: &str) -> Rendered {
    let cases = catalog::cases(result);
    let tests = cases.len();
    let failures = cases.iter().filter(|c| c.failure.is_some()).count();
    let errors = cases.iter().filter(|c| !c.errors.is_empty()).count();
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        out,
        "<testsuites name=\"rulebearing\" tests=\"{tests}\" failures=\"{failures}\" errors=\"{errors}\" time=\"0\">"
    );
    let stamp = if timestamp.is_empty() {
        String::new()
    } else {
        format!(" timestamp=\"{}\"", xml(timestamp, true))
    };
    let _ = writeln!(
        out,
        "  <testsuite name=\"rulebearing\" tests=\"{tests}\" failures=\"{failures}\" errors=\"{errors}\" skipped=\"0\" time=\"0\"{stamp}>"
    );
    out.push_str("    <properties>\n");
    for (name, value) in catalog::receipt(result) {
        let _ = writeln!(
            out,
            "      <property name=\"{}\" value=\"{}\"/>",
            xml(&name, true),
            xml(&value, true)
        );
    }
    out.push_str("    </properties>\n");
    for case in &cases {
        let _ = write!(
            out,
            "    <testcase name=\"{}\" classname=\"rulebearing.{}\" time=\"0\"",
            xml(&case.rule.name, true),
            xml(&case.rule.family, true)
        );
        if case.failure.is_none() && case.errors.is_empty() && case.output.is_empty() {
            out.push_str("/>\n");
            continue;
        }
        out.push_str(">\n");
        if let Some((message, detail)) = &case.failure {
            let _ = writeln!(
                out,
                "      <failure type=\"error\" message=\"{}\">{}</failure>",
                xml(message, true),
                xml(detail, false)
            );
        }
        for (kind, message) in &case.errors {
            let _ = writeln!(
                out,
                "      <error type=\"{}\" message=\"{}\">{}</error>",
                xml(kind, true),
                xml(message, true),
                xml(message, false)
            );
        }
        if !case.output.is_empty() {
            let _ = writeln!(
                out,
                "      <system-out>{}</system-out>",
                xml(&case.output.join("\n"), false)
            );
        }
        out.push_str("    </testcase>\n");
    }
    out.push_str("  </testsuite>\n</testsuites>\n");
    Rendered {
        output: out,
        exit_code: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_rule_per_case_with_failures_errors_and_output() {
        let result = json!({
            "modules": [{ "source": "a.ts", "dependencies": [{ "resolved": "b.ts", "line": 2, "column": 1 }] }],
            "summary": {
                "violations": [
                    { "type": "dependency", "from": "a.ts", "to": "b.ts", "rule": { "name": "no-b", "severity": "error" }, "id": "RB-1" },
                    { "type": "dependency", "from": "c.ts", "to": "b.ts", "rule": { "name": "no-b", "severity": "ignore" }, "id": "RB-2" },
                    { "type": "element", "from": "s.cs", "to": "S<T>", "rule": { "name": "sealed", "severity": "warn" }, "id": "RB-3" }
                ],
                "error": 1, "warn": 1, "info": 0, "ignore": 1, "totalCruised": 2,
                "inspected": { "typescript": { "files": 2, "assemblies": 0, "modules": 2 } },
                "ruleSetUsed": {
                    "forbidden": [{ "name": "no-b", "severity": "error", "fix": "Use \"the\" index & go." }, { "name": "dead" }],
                    "elements": [{ "name": "sealed", "severity": "warn" }]
                },
                "vacuousRules": [{ "name": "dead", "side": "from" }],
                "expired": [{ "name": "RB-9", "expires": "2026-01-01", "kind": "knownViolation" }]
            }
        });
        let rendered = render(&result, "2026-09-21T14:13:20.000");
        assert_eq!(rendered.exit_code, 0);
        let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<testsuites name=\"rulebearing\" tests=\"4\" failures=\"1\" errors=\"2\" time=\"0\">
  <testsuite name=\"rulebearing\" tests=\"4\" failures=\"1\" errors=\"2\" skipped=\"0\" time=\"0\" timestamp=\"2026-09-21T14:13:20.000\">
    <properties>
      <property name=\"inspected.typescript.assemblies\" value=\"0\"/>
      <property name=\"inspected.typescript.files\" value=\"2\"/>
      <property name=\"inspected.typescript.modules\" value=\"2\"/>
      <property name=\"totalCruised\" value=\"2\"/>
      <property name=\"error\" value=\"1\"/>
      <property name=\"warn\" value=\"1\"/>
      <property name=\"info\" value=\"0\"/>
      <property name=\"ignore\" value=\"1\"/>
    </properties>
    <testcase name=\"no-b\" classname=\"rulebearing.forbidden\" time=\"0\">
      <failure type=\"error\" message=\"Use &quot;the&quot; index &amp; go.&#10;RB-1 a.ts -&gt; b.ts (line 2, column 1)\">RB-1 a.ts -&gt; b.ts (line 2, column 1)</failure>
      <system-out>ignore: RB-2 c.ts -&gt; b.ts [known]</system-out>
    </testcase>
    <testcase name=\"dead\" classname=\"rulebearing.forbidden\" time=\"0\">
      <error type=\"vacuous\" message=\"rule `dead` is vacuous: its from side matched nothing, so it checks nothing (ADR-0007)\">rule `dead` is vacuous: its from side matched nothing, so it checks nothing (ADR-0007)</error>
    </testcase>
    <testcase name=\"sealed\" classname=\"rulebearing.elements\" time=\"0\">
      <system-out>warn: RB-3 s.cs -&gt; S&lt;T&gt;</system-out>
    </testcase>
    <testcase name=\"RB-9\" classname=\"rulebearing.knownViolations\" time=\"0\">
      <error type=\"expired\" message=\"knownViolation `RB-9` expired on 2026-01-01; it no longer applies and the run fails\">knownViolation `RB-9` expired on 2026-01-01; it no longer applies and the run fails</error>
    </testcase>
  </testsuite>
</testsuites>
";
        assert_eq!(rendered.output, expected);
    }

    #[test]
    fn a_passing_rule_is_an_empty_case_and_no_timestamp_is_left_out() {
        let result = json!({ "summary": { "violations": [], "ruleSetUsed": { "forbidden": [{ "name": "ok" }] } } });
        let output = render(&result, "").output;
        assert!(
            output
                .contains("<testcase name=\"ok\" classname=\"rulebearing.forbidden\" time=\"0\"/>")
        );
        assert!(!output.contains("timestamp"));
        assert!(output.contains("tests=\"1\" failures=\"0\" errors=\"0\""));
    }
}
