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
//! names the fixture assemblies its architecture loads (per case, or for the whole file), the
//! rule, and the expectation: the passing and failing object sets, an error
//! (`TypeDoesNotExistInArchitecture`), a vacuous selection, or `passes` (`HasNoViolations`). A case's `family` is `element`
//! (the default), `slice`, `diagram` (a diagram rule over `conformance/archunitnet/diagrams/`)
//! or `plantuml` (a diagram parsed on its own). The graphs are
//! `conformance/archunitnet/graphs/<Assembly>.json`.
//! `RB_GATE2_REPORT=1` writes the differing cases to `<temp>/rb-gate2-failures.txt`.

use std::collections::{BTreeMap, BTreeSet};
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

/// One case's verdict against its expectation: `None` when they agree. `family` picks how
/// the case is read: an element rule (the default), a slice rule, a diagram rule, or a
/// `PlantUML` diagram parsed on its own.
fn check(architecture: &Architecture<'_>, id: &str, case: &Value) -> Option<String> {
    let mut rule = case.get("rule").cloned().unwrap_or(Value::Null);
    if let Value::Object(map) = &mut rule {
        map.insert("name".into(), json!(id));
    }
    let expect = case.get("expect").cloned().unwrap_or(Value::Null);
    match case
        .get("family")
        .and_then(Value::as_str)
        .unwrap_or("element")
    {
        "element" => match rb_config::elements::parse_elements(&json!([rule])) {
            Ok(rules) => check_element(architecture, id, &rules[0], &expect),
            Err(e) => Some(format!("{id}: the rule does not parse: {e}")),
        },
        "diagram" => match rb_config::elements::parse_diagrams(&json!([rule])) {
            Ok(rules) => check_element(architecture, id, &rules[0].as_element_rule(), &expect),
            Err(e) => Some(format!("{id}: the rule does not parse: {e}")),
        },
        "slice" => match rb_config::elements::parse_slices(&json!([rule])) {
            Ok(rules) => check_slice(architecture, id, &rules[0], &expect),
            Err(e) => Some(format!("{id}: the rule does not parse: {e}")),
        },
        "plantuml" => check_plantuml(id, case, &expect),
        other => Some(format!("{id}: unknown family {other}")),
    }
}

/// An error expectation: `TypeDoesNotExistInArchitecture` is an unknown object; a `PlantUML`
/// exception is a diagram error whose message names it.
fn error_matches(expected: &str, error: &ElementError) -> bool {
    match error {
        ElementError::UnknownObject { .. } => expected == "TypeDoesNotExistInArchitecture",
        ElementError::Diagram { message, .. } => message.contains(expected),
        _ => false,
    }
}

fn check_element(
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

/// A slice case: `slices` is the number of slices (`GetObjects().Count()`), `passes` whether
/// every condition holds (`HasNoViolations`), `vacuous` an empty slicing, `error` a refused
/// pattern (`ArgumentException`).
fn check_slice(
    architecture: &Architecture<'_>,
    id: &str,
    rule: &rb_config::elements::SliceRule,
    expect: &Value,
) -> Option<String> {
    let outcome = match rb_rules::slices::evaluate(architecture, rule) {
        Ok(outcome) => outcome,
        Err(e) => {
            return (expect.get("error").and_then(Value::as_str) != Some("ArgumentException"))
                .then(|| format!("{id}: {e}"));
        }
    };
    let mut differences = Vec::new();
    if let Some(error) = expect.get("error").and_then(Value::as_str) {
        differences.push(format!("expected {error}, got an outcome"));
    }
    if let Some(count) = expect.get("slices").and_then(Value::as_u64)
        && outcome.slices.len() as u64 != count
    {
        differences.push(format!(
            "expected {count} slices, got {:?}",
            outcome.slices.keys().collect::<Vec<_>>()
        ));
    }
    if let Some(passes) = expect.get("passes").and_then(Value::as_bool)
        && outcome.failures.is_empty() != passes
    {
        differences.push(format!(
            "expected passes={passes}, got failures {:?}",
            outcome.failures
        ));
    }
    if let Some(vacuous) = expect.get("vacuous").and_then(Value::as_bool)
        && outcome.vacuous != vacuous
    {
        differences.push(format!("expected vacuous={vacuous}"));
    }
    (!differences.is_empty()).then(|| format!("{id}: {}", differences.join("; ")))
}

/// A `PlantUML` parse case: the diagram inline (`diagram`) or a file under `diagrams/`
/// (`file`); `components` lists `{name, stereotypes, alias}`, `dependencies` the
/// `[origin, target]` component names, `error` the exception a malformed diagram raises.
fn check_plantuml(id: &str, case: &Value, expect: &Value) -> Option<String> {
    let text = match (
        case.get("diagram").and_then(Value::as_str),
        case.get("file").and_then(Value::as_str),
    ) {
        (Some(text), _) => text.to_owned(),
        (None, Some(file)) => {
            match std::fs::read_to_string(conformance().join("diagrams").join(file)) {
                Ok(text) => text,
                Err(e) => return Some(format!("{id}: {file}: {e}")),
            }
        }
        (None, None) => return Some(format!("{id}: neither diagram nor file")),
    };
    let diagram = match rb_rules::plantuml::parse(&text) {
        Ok(diagram) => diagram,
        Err(message) => {
            return match expect.get("error").and_then(Value::as_str) {
                Some(error) if message.contains(error) => None,
                _ => Some(format!("{id}: {message}")),
            };
        }
    };
    let mut differences = Vec::new();
    if let Some(error) = expect.get("error").and_then(Value::as_str) {
        differences.push(format!("expected {error}, got a diagram"));
    }
    if let Some(components) = expect.get("components") {
        let got: Vec<Value> = diagram
            .components
            .iter()
            .map(|c| {
                let mut v = json!({ "name": c.name, "stereotypes": c.stereotypes });
                if let Some(alias) = &c.alias {
                    v["alias"] = json!(alias);
                }
                v
            })
            .collect();
        if Value::Array(got.clone()) != *components {
            differences.push(format!("components {got:?}"));
        }
    }
    if let Some(dependencies) = expect.get("dependencies") {
        let got: Vec<Value> = diagram
            .dependencies
            .iter()
            .flat_map(|(from, targets)| {
                targets
                    .iter()
                    .map(|to| json!([diagram.components[*from].name, diagram.components[*to].name]))
            })
            .collect();
        let want: BTreeSet<String> = dependencies
            .as_array()
            .into_iter()
            .flatten()
            .map(Value::to_string)
            .collect();
        let have: BTreeSet<String> = got.iter().map(Value::to_string).collect();
        if want != have {
            differences.push(format!("dependencies {have:?}"));
        }
    }
    (!differences.is_empty()).then(|| format!("{id}: {}", differences.join("; ")))
}

/// The architecture a case loads: its own `architecture`, or its file's.
fn assemblies_of(suite: &Value, case: &Value) -> Vec<String> {
    let own = strings(case.get("architecture"));
    let chosen = if own.is_empty() {
        strings(suite.get("architecture"))
    } else {
        own
    };
    chosen.into_iter().collect()
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
                let document = architecture_document(slot.key())?;
                slot.insert(document);
            }
        }
    }
    let architectures: BTreeMap<&Vec<String>, Architecture<'_>> = documents
        .iter()
        .map(|(key, document)| {
            let mut architecture = Architecture::new(document);
            architecture.base = conformance().join("diagrams");
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
