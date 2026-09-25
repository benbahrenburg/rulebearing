//! Conformance gate 2, NetArchTest half: every ported NetArchTest 1.3.2 unit test, evaluated by the
//! element engine over the NetArchTest fixture graphs, must reproduce the verdict NetArchTest itself
//! gives over the same assemblies.
//!
//! - Source: [design § Conformance gate 2](../../../docs/artifacts/design.md#conformance-gate-2-archunitnets-test-assemblies-validate-the-element-rules)
//!   ("NetArchTest's own test project is treated the same way")
//! - Decision: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md) (the
//!   upstream suite is the specification; the unported count only falls)
//! - Plan: [Wave 2, Step 7](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#27-step-7-gate-2-porting-to-completion-2c)
//! - Requirements: [NFR-CONF-02](../../../docs/prd.md#nfr-conf-02), [FR-RULE-03](../../../docs/prd.md#fr-rule-03)
//!
//! Cases live in `conformance/netarchtest/ported/<TestClass>.yaml`, written by
//! `conformance/netarchtest/tools/Port`, which runs each upstream chain with NetArchTest 1.3.2 over
//! the committed fixtures and records the passing and failing types beside the element rule that
//! maps it. Every case is an element rule. The graphs are
//! `conformance/netarchtest/graphs/<Assembly>.json`. `RB_GATE2_REPORT=1` writes the differing
//! cases to `<temp>/rb-gate2-netarchtest-failures.txt`.

mod gate2_common;

use rb_rules::elements::Architecture;
use serde_json::{Value, json};

/// One case's verdict against its expectation: `None` when they agree.
fn check(architecture: &Architecture<'_>, id: &str, case: &Value) -> Option<String> {
    let mut rule = case.get("rule").cloned().unwrap_or(Value::Null);
    if let Value::Object(map) = &mut rule {
        map.insert("name".into(), json!(id));
    }
    let expect = case.get("expect").cloned().unwrap_or(Value::Null);
    match rb_config::elements::parse_elements(&json!([rule])) {
        Ok(rules) => gate2_common::check_element(architecture, id, &rules[0], &expect),
        Err(e) => Some(format!("{id}: the rule does not parse: {e}")),
    }
}

#[test]
fn every_ported_netarchtest_case_reproduces_upstream() -> Result<(), Box<dyn std::error::Error>> {
    let run = gate2_common::run_suite(&gate2_common::suite_root("netarchtest"), check)?;
    gate2_common::report(
        "gate2 netarchtest",
        "rb-gate2-netarchtest-failures.txt",
        &run,
    )
}
