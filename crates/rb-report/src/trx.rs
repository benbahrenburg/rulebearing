//! `trx`: the Visual Studio test results format, one unit test per rule, for Azure DevOps'
//! test tab and `dotnet test` tooling.
//!
//! - Contract: [Wave 2 plan § 1.5](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#15-interfaces-and-contracts-this-wave-freezes)
//!   (one test case per rule; the failure message carries the `fix` and the first violations)
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//!   (the receipt as a property; vacuous rules as errors, not failures)
//! - Source: [design § Reporters](../../../docs/artifacts/design.md#reporters)
//! - Decisions: [ADR-0007](../../../docs/adr/0007-vacuous-rules-fail-by-default.md),
//!   [ADR-0015](../../../docs/adr/0015-stable-violation-id.md)
//! - Requirement: [FR-OUT-02](../../../docs/prd.md#fr-out-02)
//!
//! The same cases as [`crate::junit`] ([`crate::catalog::cases`]), in the shape `vstest` writes:
//! `Times`, `Results` (one `UnitTestResult` per rule, outcome `Passed`, `Failed` for an
//! error-severity violation, `Error` for a rule that could not be checked), `TestDefinitions`,
//! `TestEntries`, `TestLists` and `ResultSummary` with its `Counters`. A failure's `Message` is
//! the `fix` and the first violations; its `StackTrace` lists every violation, one per object
//! for an element rule. TRX has no run-level properties, so the receipt is the `ResultSummary`'s
//! standard output, one `name = value` line each. A test is named by the rule's catalogue id (the
//! name, `name#2` for a second rule of that name), and every id is a name-based GUID (SHA-256 of
//! the rule's family and catalogue id), so no two tests share one and two runs are byte-identical; the times are the run's timestamp.
//! Exits 0, as the data reporters do.

use std::fmt::Write as _;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::Rendered;
use crate::catalog::{self, Case, xml};

/// The TRX namespace.
pub const NAMESPACE: &str = "http://microsoft.com/schemas/VisualStudio/TeamTest/2010";
/// The test type `vstest` gives a unit test.
pub const UNIT_TEST_TYPE: &str = "13cdc9d9-ddb5-4fa4-a97d-d965ccfc6d4b";
/// The list `vstest` puts results in when no list is named: "Results Not in a List".
pub const RESULTS_NOT_IN_A_LIST: &str = "8c84fa94-04c1-424b-9868-57a2d4851a1d";
/// The list of every loaded result: "All Loaded Results".
pub const ALL_LOADED_RESULTS: &str = "19431567-8539-422a-85d7-44ee4e166bda";

/// A name-based GUID: the first 16 bytes of SHA-256 over `name`, with the version (5) and
/// variant bits set as RFC 9562 lays them out.
pub fn guid(name: &str) -> String {
    let digest = Sha256::digest(name.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    });
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// A case's outcome.
pub fn outcome(case: &Case) -> &'static str {
    if !case.errors.is_empty() {
        "Error"
    } else if case.failure.is_some() {
        "Failed"
    } else {
        "Passed"
    }
}

struct Ids {
    test: String,
    execution: String,
}

fn ids(case: &Case) -> Ids {
    let key = format!("{}/{}", case.rule.family, case.rule.id);
    Ids {
        test: guid(&format!("rulebearing/trx/test/{key}")),
        execution: guid(&format!("rulebearing/trx/execution/{key}")),
    }
}

fn result_element(out: &mut String, case: &Case, ids: &Ids, time: &str) {
    let _ = write!(
        out,
        "    <UnitTestResult executionId=\"{}\" testId=\"{}\" testName=\"{}\" computerName=\"rulebearing\" duration=\"00:00:00\" startTime=\"{time}\" endTime=\"{time}\" testType=\"{UNIT_TEST_TYPE}\" outcome=\"{}\" testListId=\"{RESULTS_NOT_IN_A_LIST}\" relativeResultsDirectory=\"{}\"",
        ids.execution,
        ids.test,
        xml(&case.rule.id, true),
        outcome(case),
        ids.execution
    );
    let mut messages: Vec<String> = case.errors.iter().map(|(_, m)| m.clone()).collect();
    let mut traces: Vec<String> = Vec::new();
    if let Some((message, detail)) = &case.failure {
        messages.push(message.clone());
        traces.push(detail.clone());
    }
    if messages.is_empty() && case.output.is_empty() {
        out.push_str(" />\n");
        return;
    }
    out.push_str(">\n      <Output>\n");
    if !case.output.is_empty() {
        let _ = writeln!(
            out,
            "        <StdOut>{}</StdOut>",
            xml(&case.output.join("\n"), false)
        );
    }
    if !messages.is_empty() {
        out.push_str("        <ErrorInfo>\n");
        let _ = writeln!(
            out,
            "          <Message>{}</Message>",
            xml(&messages.join("\n"), false)
        );
        if !traces.is_empty() {
            let _ = writeln!(
                out,
                "          <StackTrace>{}</StackTrace>",
                xml(&traces.join("\n"), false)
            );
        }
        out.push_str("        </ErrorInfo>\n");
    }
    out.push_str("      </Output>\n    </UnitTestResult>\n");
}

/// Renders `trx`. `timestamp` is the run's, ISO 8601 without a zone.
pub fn render(result: &Value, timestamp: &str) -> Rendered {
    let cases = catalog::cases(result);
    let time = if timestamp.is_empty() {
        "1970-01-01T00:00:00.000".to_owned()
    } else {
        xml(timestamp, true)
    };
    let run_id = guid(&format!("rulebearing/trx/run/{time}"));
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        out,
        "<TestRun id=\"{run_id}\" name=\"rulebearing {time}\" runUser=\"rulebearing\" xmlns=\"{NAMESPACE}\">"
    );
    let _ = writeln!(
        out,
        "  <Times creation=\"{time}\" queuing=\"{time}\" start=\"{time}\" finish=\"{time}\" />"
    );
    let all: Vec<(&Case, Ids)> = cases.iter().map(|c| (c, ids(c))).collect();
    out.push_str("  <Results>\n");
    for (case, ids) in &all {
        result_element(&mut out, case, ids, &time);
    }
    out.push_str("  </Results>\n  <TestDefinitions>\n");
    for (case, ids) in &all {
        let _ = writeln!(
            out,
            "    <UnitTest name=\"{name}\" storage=\"rulebearing\" id=\"{}\">\n      <Execution id=\"{}\" />\n      <TestMethod codeBase=\"rulebearing\" adapterTypeName=\"executor://rulebearing/v1\" className=\"rulebearing.{}\" name=\"{name}\" />\n    </UnitTest>",
            ids.test,
            ids.execution,
            xml(&case.rule.family, true),
            name = xml(&case.rule.id, true),
        );
    }
    out.push_str("  </TestDefinitions>\n  <TestEntries>\n");
    for (_, ids) in &all {
        let _ = writeln!(
            out,
            "    <TestEntry testId=\"{}\" executionId=\"{}\" testListId=\"{RESULTS_NOT_IN_A_LIST}\" />",
            ids.test, ids.execution
        );
    }
    let _ = writeln!(
        out,
        "  </TestEntries>\n  <TestLists>\n    <TestList name=\"Results Not in a List\" id=\"{RESULTS_NOT_IN_A_LIST}\" />\n    <TestList name=\"All Loaded Results\" id=\"{ALL_LOADED_RESULTS}\" />\n  </TestLists>"
    );
    let count = |o: &str| cases.iter().filter(|c| outcome(c) == o).count();
    let (passed, failed, errors) = (count("Passed"), count("Failed"), count("Error"));
    let summary = if failed + errors == 0 {
        "Completed"
    } else {
        "Failed"
    };
    let total = cases.len();
    let _ = writeln!(
        out,
        "  <ResultSummary outcome=\"{summary}\">\n    <Counters total=\"{total}\" executed=\"{total}\" passed=\"{passed}\" failed=\"{failed}\" error=\"{errors}\" timeout=\"0\" aborted=\"0\" inconclusive=\"0\" passedButRunAborted=\"0\" notRunnable=\"0\" notExecuted=\"0\" disconnected=\"0\" warning=\"0\" completed=\"0\" inProgress=\"0\" pending=\"0\" />"
    );
    let receipt: Vec<String> = catalog::receipt(result)
        .into_iter()
        .map(|(name, value)| format!("{name} = {value}"))
        .collect();
    if !receipt.is_empty() {
        let _ = writeln!(
            out,
            "    <Output>\n      <StdOut>{}</StdOut>\n    </Output>",
            xml(&receipt.join("\n"), false)
        );
    }
    out.push_str("  </ResultSummary>\n</TestRun>\n");
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
    fn guids_are_name_based_and_well_formed() {
        let id = guid("rulebearing/trx/test/forbidden/no-b");
        assert_eq!(id, guid("rulebearing/trx/test/forbidden/no-b"));
        assert_ne!(id, guid("rulebearing/trx/test/forbidden/no-c"));
        let parts: Vec<usize> = id.split('-').map(str::len).collect();
        assert_eq!(parts, [8, 4, 4, 4, 12]);
        assert_eq!(&id[14..15], "5", "version 5");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"), "RFC variant");
        // A fixed vector: the id a TRX consumer sees for this rule must not move between releases.
        assert_eq!(guid(""), "e3b0c442-98fc-5c14-9afb-f4c8996fb924");
    }

    #[test]
    fn outcomes_counters_and_the_receipt() {
        let result = json!({
            "summary": {
                "violations": [
                    { "type": "element", "from": "a.cs", "to": "A", "rule": { "name": "sealed", "severity": "error" }, "id": "RB-1", "fix": "Seal <it>." }
                ],
                "totalCruised": 1,
                "ruleSetUsed": { "forbidden": [{ "name": "ok" }, { "name": "dead" }], "elements": [{ "name": "sealed", "severity": "error" }] },
                "vacuousRules": [{ "name": "dead", "side": "from" }]
            }
        });
        let output = render(&result, "2026-09-21T14:13:20.000").output;
        assert!(output.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<TestRun id=\""));
        assert!(output.contains(&format!("xmlns=\"{NAMESPACE}\"")));
        assert!(output.contains("testName=\"ok\" computerName=\"rulebearing\" duration=\"00:00:00\" startTime=\"2026-09-21T14:13:20.000\""));
        assert!(output.contains("outcome=\"Passed\" testListId"));
        assert!(output.contains("testName=\"dead\""));
        assert!(output.contains("outcome=\"Error\""));
        assert!(output.contains("outcome=\"Failed\""));
        assert!(output.contains("<Message>Seal &lt;it&gt;.\nRB-1 a.cs -&gt; A</Message>"));
        assert!(output.contains("<StackTrace>RB-1 a.cs -&gt; A</StackTrace>"));
        assert!(output.contains("<ResultSummary outcome=\"Failed\">"));
        assert!(
            output.contains("total=\"3\" executed=\"3\" passed=\"1\" failed=\"1\" error=\"1\"")
        );
        assert!(output.contains("<StdOut>totalCruised = 1</StdOut>"));
        assert!(output.contains("className=\"rulebearing.elements\" name=\"sealed\""));
        assert_eq!(output, render(&result, "2026-09-21T14:13:20.000").output);
        let clean = render(
            &json!({ "summary": { "ruleSetUsed": { "forbidden": [{ "name": "ok" }] } } }),
            "",
        );
        assert!(
            clean
                .output
                .contains("<ResultSummary outcome=\"Completed\">")
        );
        assert!(clean.output.contains("start=\"1970-01-01T00:00:00.000\""));
        assert!(!clean.output.contains("<Output>\n      <StdOut>"));
        assert_eq!(clean.exit_code, 0);
    }

    #[test]
    fn rules_sharing_a_name_are_distinct_tests_with_distinct_guids() {
        let result = json!({ "summary": {
            "violations": [
                { "type": "dependency", "from": "a", "to": "b", "rule": { "name": "unnamed", "severity": "error" } }
            ],
            "ruleSetUsed": { "forbidden": [{ "name": "unnamed", "severity": "error" }, { "name": "unnamed", "severity": "error" }] }
        } });
        let output = render(&result, "").output;
        assert!(output.contains("<UnitTest name=\"unnamed\" storage"));
        assert!(output.contains("<UnitTest name=\"unnamed#2\" storage"));
        assert_eq!(
            output.matches("<StackTrace>a -&gt; b</StackTrace>").count(),
            1
        );
        let first = guid("rulebearing/trx/test/forbidden/unnamed");
        let second = guid("rulebearing/trx/test/forbidden/unnamed#2");
        assert_ne!(first, second);
        // Fixed vectors: a TRX consumer tracks a test by these ids across releases.
        assert_eq!(first, "3de65f45-ac60-54ca-813d-27fd2daf2733");
        assert_eq!(second, "f5590fe1-a335-5bcf-b2ec-8b304891d768");
        assert!(output.contains(&format!("testId=\"{first}\"")));
        assert!(output.contains(&format!("testId=\"{second}\"")));
    }
}
