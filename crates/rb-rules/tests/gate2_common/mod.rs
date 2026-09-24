//! What the two halves of conformance gate 2 share: reading a suite's ported cases, joining the
//! fixture graphs each case loads, and checking an element rule's verdict against the case.
//!
//! - Source: [design § Conformance gate 2](../../../../docs/artifacts/design.md#conformance-gate-2-archunitnets-test-assemblies-validate-the-element-rules)
//! - Decision: [ADR-0009](../../../../docs/adr/0009-conformance-suites-as-specification.md)
//! - Plan: [Wave 2, Step 7](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#27-step-7-gate-2-porting-to-completion-2c)
//!
//! A suite is a directory under `conformance/` holding `ported/<TestClass>.yaml`, `ported.json`
//! and `graphs/<Assembly>.json`: `conformance/archunitnet` (`tests/gate2.rs`) and
//! `conformance/netarchtest` (`tests/gate2_netarchtest.rs`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rb_model::GraphDocument;
use rb_rules::elements::{Architecture, ElementError, evaluate};
use serde_json::Value;

/// The suite's directory: `conformance/<name>`.
pub fn suite_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance")
        .join(name)
}

/// The architecture a case loads: the suite's fixture graphs for `assemblies`, joined.
pub fn architecture_document(
    root: &Path,
    assemblies: &[String],
) -> Result<GraphDocument, Box<dyn std::error::Error>> {
    let mut document = GraphDocument::default();
    let mut code = rb_model::CodeLayer::default();
    for assembly in assemblies {
        let path = root.join("graphs").join(format!("{assembly}.json"));
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

/// A list of strings, or nothing.
pub fn strings(value: Option<&Value>) -> BTreeSet<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}

/// An error expectation: `TypeDoesNotExistInArchitecture` is an unknown object; a `PlantUML`
/// exception is a diagram error whose message names it.
pub fn error_matches(expected: &str, error: &ElementError) -> bool {
    match error {
        ElementError::UnknownObject { .. } => expected == "TypeDoesNotExistInArchitecture",
        ElementError::Diagram { message, .. } => message.contains(expected),
        _ => false,
    }
}

/// One element rule's verdict against its expectation: `None` when they agree. The expectation
/// is the passing and failing object sets, an error, a vacuous selection, or `passes`
/// (`HasNoViolations`).
pub fn check_element(
    architecture: &Architecture<'_>,
    id: &str,
    rule: &rb_config::elements::ElementRule,
    expect: &Value,
) -> Option<String> {
    let outcome = evaluate(architecture, rule);
    if let Some(error) = expect.get("error").and_then(Value::as_str) {
        return match outcome {
            Err(e) if error_matches(error, &e) => None,
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
    // `HasNoViolations`: every result passes and a positive result was required and found.
    if let Some(passes) = expect.get("passes").and_then(Value::as_bool) {
        return (outcome.holds() != passes).then(|| {
            format!(
                "{id}: expected passes={passes}, got {:?} (vacuous {}, existence failed {})",
                outcome.results, outcome.vacuous, outcome.existence_failed
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

/// The architecture a case loads: its own `architecture`, or its file's.
pub fn assemblies_of(suite: &Value, case: &Value) -> Vec<String> {
    let own = strings(case.get("architecture"));
    let chosen = if own.is_empty() {
        strings(suite.get("architecture"))
    } else {
        own
    };
    chosen.into_iter().collect()
}

/// What running a suite found: how many cases, and each case that differs from upstream.
pub struct SuiteRun {
    /// The cases evaluated.
    pub total: u64,
    /// One line per differing case.
    pub failures: Vec<String>,
}

/// Evaluates every case under `<root>/ported/*.yaml` with `check`, over one architecture per
/// distinct assembly set (its diagram folder `<root>/diagrams`), and asserts that `ported.json`
/// counts them.
pub fn run_suite(
    root: &Path,
    check: impl Fn(&Architecture<'_>, &str, &Value) -> Option<String>,
) -> Result<SuiteRun, Box<dyn std::error::Error>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join("ported"))
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "yaml"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut suites = Vec::new();
    for file in &files {
        let suite: Value = serde_yaml::from_str(&std::fs::read_to_string(file)?)?;
        let name = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        suites.push((name, suite));
    }
    let empty = Vec::new();
    let cases = |suite: &Value| -> Vec<Value> {
        suite
            .get("cases")
            .and_then(Value::as_array)
            .unwrap_or(&empty)
            .clone()
    };
    // One document per distinct assembly set, then one architecture over each.
    let mut documents: BTreeMap<Vec<String>, GraphDocument> = BTreeMap::new();
    for (_, suite) in &suites {
        for case in cases(suite) {
            let assemblies = assemblies_of(suite, &case);
            if let std::collections::btree_map::Entry::Vacant(slot) = documents.entry(assemblies) {
                let document = architecture_document(root, slot.key())?;
                slot.insert(document);
            }
        }
    }
    let architectures: BTreeMap<&Vec<String>, Architecture<'_>> = documents
        .iter()
        .map(|(key, document)| {
            let mut architecture = Architecture::new(document);
            architecture.base = root.join("diagrams");
            (key, architecture)
        })
        .collect();
    let mut total = 0_u64;
    let mut failures = Vec::new();
    for (name, suite) in &suites {
        for case in cases(suite) {
            total += 1;
            let id = case.get("id").and_then(Value::as_str).unwrap_or("?");
            let assemblies = assemblies_of(suite, &case);
            let Some(architecture) = architectures.get(&assemblies) else {
                failures.push(format!("{name}.{id}: no architecture for {assemblies:?}"));
                continue;
            };
            if let Some(failure) = check(architecture, &format!("{name}.{id}"), &case) {
                failures.push(failure);
            }
        }
    }
    let ported: Value = serde_json::from_str(&std::fs::read_to_string(root.join("ported.json"))?)?;
    assert_eq!(
        ported.get("ported").and_then(Value::as_u64),
        Some(total),
        "{}: ported.json's count must equal the cases under ported/",
        root.display()
    );
    Ok(SuiteRun { total, failures })
}

/// Prints the tally, writes the differing cases to `<temp>/<report>` under `RB_GATE2_REPORT`,
/// and fails when any case differs.
pub fn report(label: &str, report: &str, run: &SuiteRun) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{label}: {} of {} ported cases reproduce upstream",
        run.total - run.failures.len() as u64,
        run.total
    );
    if std::env::var_os("RB_GATE2_REPORT").is_some() {
        std::fs::write(std::env::temp_dir().join(report), run.failures.join("\n"))?;
    }
    assert!(
        run.failures.is_empty(),
        "{} cases differ from upstream:\n{}",
        run.failures.len(),
        run.failures.join("\n")
    );
    Ok(())
}
