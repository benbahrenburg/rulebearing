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
//! (`TypeDoesNotExistInArchitecture`), a vacuous selection, or `passes` (`HasNoViolations`). A
//! case's `family` is `element` (the default), `slice`, `diagram` (a diagram rule over `conformance/archunitnet/diagrams/`),
//! `plantuml` (a diagram parsed on its own) or `association` (a diagram associated with one type
//! of the architecture, as `ClassDiagramAssociation` does). The graphs are
//! `conformance/archunitnet/graphs/<Assembly>.json`.
//! `RB_GATE2_REPORT=1` writes the differing cases to `<temp>/rb-gate2-failures.txt`. Reading the
//! cases, joining the graphs and checking an element rule are shared with the NetArchTest half
//! (`gate2_netarchtest.rs`) in `gate2_common/`.

mod gate2_common;

use std::collections::BTreeSet;
use std::path::PathBuf;

use gate2_common::{check_element, strings};
use rb_rules::elements::Architecture;
use serde_json::{Value, json};

fn conformance() -> PathBuf {
    gate2_common::suite_root("archunitnet")
}

/// One case's verdict against its expectation: `None` when they agree. `family` picks how
/// the case is read: an element rule (the default), a slice rule, a diagram rule, a
/// `PlantUML` diagram parsed on its own, or a diagram associated with a type.
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
        "association" => check_association(architecture, id, case, &expect),
        other => Some(format!("{id}: unknown family {other}")),
    }
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

/// A case's diagram text: inline (`diagram`) or a file under `diagrams/` (`file`).
fn diagram_text(id: &str, case: &Value) -> Result<String, String> {
    match (
        case.get("diagram").and_then(Value::as_str),
        case.get("file").and_then(Value::as_str),
    ) {
        (Some(text), _) => Ok(text.to_owned()),
        (None, Some(file)) => std::fs::read_to_string(conformance().join("diagrams").join(file))
            .map_err(|e| format!("{id}: {file}: {e}")),
        (None, None) => Err(format!("{id}: neither diagram nor file")),
    }
}

/// An error expectation on a diagram: `error` names the exception, `message` is the whole
/// message (`Assert.Equal`), `messageContains` parts of it (`Assert.Contains`).
fn check_diagram_error(
    id: &str,
    expect: &Value,
    error: &rb_rules::plantuml::DiagramError,
) -> Option<String> {
    let Some(name) = expect.get("error").and_then(Value::as_str) else {
        return Some(format!("{id}: {error}"));
    };
    let mut differences = Vec::new();
    if error.exception.name() != name {
        differences.push(format!("expected {name}, got {error}"));
    }
    if let Some(message) = expect.get("message").and_then(Value::as_str)
        && error.message != message
    {
        differences.push(format!("message {:?}", error.message));
    }
    for part in strings(expect.get("messageContains")) {
        if !error.message.contains(&part) {
            differences.push(format!("message {:?} lacks {part:?}", error.message));
        }
    }
    (!differences.is_empty()).then(|| format!("{id}: {}", differences.join("; ")))
}

/// A `PlantUML` parse case: the diagram inline (`diagram`) or a file under `diagrams/`
/// (`file`); `components` lists `{name, stereotypes, alias}` in order, `dependencies` the
/// `[origin, target]` component names as a set, `dependenciesOf` a component's targets in the
/// order the diagram draws them, `error` the exception a malformed diagram raises.
fn check_plantuml(id: &str, case: &Value, expect: &Value) -> Option<String> {
    let text = match diagram_text(id, case) {
        Ok(text) => text,
        Err(e) => return Some(e),
    };
    let diagram = match rb_rules::plantuml::parse(&text) {
        Ok(diagram) => diagram,
        Err(error) => return check_diagram_error(id, expect, &error),
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
    let name = |index: &usize| diagram.components[*index].name.clone();
    if let Some(dependencies) = expect.get("dependencies") {
        let got: Vec<Value> = diagram
            .dependencies
            .iter()
            .flat_map(|(from, targets)| targets.iter().map(|to| json!([name(from), name(to)])))
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
    if let Some(Value::Object(of)) = expect.get("dependenciesOf") {
        for (component, want) in of {
            let Some(index) = diagram.components.iter().position(|c| &c.name == component) else {
                differences.push(format!("no component {component}"));
                continue;
            };
            let got: Vec<String> = diagram.dependencies_of(index).iter().map(name).collect();
            if json!(got) != *want {
                differences.push(format!("{component} depends on {got:?}"));
            }
        }
    }
    (!differences.is_empty()).then(|| format!("{id}: {}", differences.join("; ")))
}

/// A `ClassDiagramAssociation` case: the diagram (`diagram` or `file`) associated, then `ask`
/// about `object` (a type's full name in the case's architecture): `namespaceIdentifiers` or
/// `targetNamespaceIdentifiers` (expect `value`, a list compared as a set), `contains` (expect
/// `value`, a boolean); or an `error` from associating or asking.
fn check_association(
    architecture: &Architecture<'_>,
    id: &str,
    case: &Value,
    expect: &Value,
) -> Option<String> {
    use rb_rules::plantuml::{Association, name_of, namespace_of, parse};
    let text = match diagram_text(id, case) {
        Ok(text) => text,
        Err(e) => return Some(e),
    };
    let association = match parse(&text).and_then(Association::new) {
        Ok(association) => association,
        Err(error) => return check_diagram_error(id, expect, &error),
    };
    let object = case
        .get("object")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (name, namespace) = (
        name_of(architecture, object),
        namespace_of(architecture, object),
    );
    let answer = match case.get("ask").and_then(Value::as_str) {
        Some("namespaceIdentifiers") => association
            .namespace_identifiers_of(&name, &namespace)
            .map(|v| json!(v.iter().collect::<BTreeSet<_>>())),
        Some("targetNamespaceIdentifiers") => association
            .target_namespace_identifiers(&name, &namespace)
            .map(|v| json!(v.iter().collect::<BTreeSet<_>>())),
        Some("contains") => association.contains(&namespace).map(|b| json!(b)),
        other => return Some(format!("{id}: unknown ask {other:?}")),
    };
    match answer {
        Err(error) => check_diagram_error(id, expect, &error),
        Ok(_) if expect.get("error").is_some() => {
            Some(format!("{id}: expected {}, got an answer", expect["error"]))
        }
        Ok(got) => {
            let want = match expect.get("value") {
                Some(Value::Array(items)) => json!(
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<BTreeSet<_>>()
                ),
                Some(other) => other.clone(),
                None => Value::Null,
            };
            (got != want).then(|| format!("{id}: got {got}"))
        }
    }
}

#[test]
fn every_ported_case_reproduces_upstream() -> Result<(), Box<dyn std::error::Error>> {
    let run = gate2_common::run_suite(&conformance(), check)?;
    gate2_common::report("gate2", "rb-gate2-failures.txt", &run)
}
