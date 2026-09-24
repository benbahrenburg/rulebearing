//! Conformance gate 2: every ported `ArchUnitNET` 0.13.4 test case, evaluated by the element
//! engine over the fixture assemblies' graphs, must reproduce upstream's recorded verdicts.
//!
//! - Source: [design § Conformance gate 2](../../../docs/artifacts/design.md#conformance-gate-2-archunitnets-test-assemblies-validate-the-element-rules)
//! - Decision: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md) (the
//!   upstream suite is the specification; the unported count only falls)
//! - Plan: [Wave 2, Step 7](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#27-step-7-gate-2-porting-to-completion-2c)
//! - Requirements: [NFR-CONF-02](../../../docs/prd.md#nfr-conf-02), [FR-RULE-03](../../../docs/prd.md#fr-rule-03)
//!
//! Cases live in `conformance/archunitnet/ported/<TestClass>.yaml`, written by
//! `conformance/archunitnet/tools/port.py` from upstream's test sources and Verify snapshots. Each
//! names the fixture assemblies its architecture loads, the rule, and the expectation: the
//! passing and failing object sets, an error (`TypeDoesNotExistInArchitecture`), or a vacuous
//! selection. The graphs are `conformance/archunitnet/graphs/<Assembly>.json`.
//! `RB_GATE2_REPORT=1` writes the differing cases to `<temp>/rb-gate2-failures.txt`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rb_model::GraphDocument;
use rb_rules::elements::{Architecture, ElementError, evaluate};
use serde_json::{Value, json};

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/archunitnet")
}

/// The architecture a case file loads: its fixture graphs joined.
fn architecture_document(
    assemblies: &[String],
) -> Result<GraphDocument, Box<dyn std::error::Error>> {
    let mut document = GraphDocument::default();
    let mut code = rb_model::CodeLayer::default();
    for assembly in assemblies {
        let path = conformance()
            .join("graphs")
            .join(format!("{assembly}.json"));
        let graph: GraphDocument = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        document.modules.extend(graph.modules);
        if let Some(layer) = graph.code {
            code.merge(layer);
        }
    }
    code.normalise();
    document.code = Some(code);
    Ok(document)
}

fn strings(value: Option<&Value>) -> BTreeSet<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}

/// One case's verdict against its expectation: `None` when they agree.
fn check(architecture: &Architecture<'_>, id: &str, case: &Value) -> Option<String> {
    let mut rule = case.get("rule").cloned().unwrap_or(Value::Null);
    if let Value::Object(map) = &mut rule {
        map.insert("name".into(), json!(id));
    }
    let rules = match rb_config::elements::parse_elements(&json!([rule])) {
        Ok(rules) => rules,
        Err(e) => return Some(format!("{id}: the rule does not parse: {e}")),
    };
    let expect = case.get("expect").cloned().unwrap_or(Value::Null);
    let outcome = evaluate(architecture, &rules[0]);
    if let Some(error) = expect.get("error").and_then(Value::as_str) {
        return match outcome {
            Err(ElementError::UnknownObject { .. })
                if error == "TypeDoesNotExistInArchitecture" =>
            {
                None
            }
            Err(other) => Some(format!("{id}: expected {error}, got the error {other}")),
            Ok(outcome) => Some(format!(
                "{id}: expected {error}, got {} results",
                outcome.results.len()
            )),
        };
    }
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(e) => return Some(format!("{id}: {e}")),
    };
    if expect.get("vacuous").and_then(Value::as_bool) == Some(true) {
        return (!outcome.vacuous).then(|| {
            format!(
                "{id}: expected a vacuous selection, got {} objects",
                outcome.results.len()
            )
        });
    }
    let (want_pass, want_fail) = (strings(expect.get("pass")), strings(expect.get("fail")));
    let got_pass: BTreeSet<String> = outcome
        .results
        .iter()
        .filter(|r| r.passed)
        .map(|r| r.object.clone())
        .collect();
    let got_fail: BTreeSet<String> = outcome
        .results
        .iter()
        .filter(|r| !r.passed)
        .map(|r| r.object.clone())
        .collect();
    if want_pass == got_pass && want_fail == got_fail {
        return None;
    }
    let diff = |want: &BTreeSet<String>, got: &BTreeSet<String>| {
        let missing: Vec<&String> = want.difference(got).collect();
        let extra: Vec<&String> = got.difference(want).collect();
        format!("missing {missing:?} extra {extra:?}")
    };
    Some(format!(
        "{id}: pass {}; fail {}",
        diff(&want_pass, &got_pass),
        diff(&want_fail, &got_fail)
    ))
}

#[test]
fn every_ported_case_reproduces_upstream() -> Result<(), Box<dyn std::error::Error>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(conformance().join("ported"))
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "yaml"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut total = 0_u64;
    let mut failures = Vec::new();
    for file in &files {
        let suite: Value = serde_yaml::from_str(&std::fs::read_to_string(file)?)?;
        let assemblies: Vec<String> = strings(suite.get("architecture")).into_iter().collect();
        let document = architecture_document(&assemblies)?;
        let architecture = Architecture::new(&document);
        let name = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        for case in suite
            .get("cases")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            total += 1;
            let id = case.get("id").and_then(Value::as_str).unwrap_or("?");
            if let Some(failure) = check(&architecture, &format!("{name}.{id}"), case) {
                failures.push(failure);
            }
        }
    }
    println!(
        "gate2: {} of {total} ported cases reproduce upstream",
        total - failures.len() as u64
    );
    if std::env::var_os("RB_GATE2_REPORT").is_some() {
        std::fs::write(
            std::env::temp_dir().join("rb-gate2-failures.txt"),
            failures.join("\n"),
        )?;
    }
    let ported: Value =
        serde_json::from_str(&std::fs::read_to_string(conformance().join("ported.json"))?)?;
    assert_eq!(
        ported.get("ported").and_then(Value::as_u64),
        Some(total),
        "ported.json's count must equal the cases under ported/"
    );
    assert!(
        failures.is_empty(),
        "{} cases differ from upstream:\n{}",
        failures.len(),
        failures.join("\n")
    );
    Ok(())
}
