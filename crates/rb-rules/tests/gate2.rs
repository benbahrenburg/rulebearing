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
//! `plantuml` (a diagram parsed on its own), `plantuml-export` (a diagram `PlantUmlFileBuilder` writes,
//! compared with upstream's text or with `ArchUnitNET`'s own output under `diagrams/generated/`), `association` (a diagram associated with one type
//! of the architecture, as `ClassDiagramAssociation` does) or `baseline` (an element or slice rule
//! frozen against a known-violations file under `conformance/archunitnet/baselines/`, as
//! `FreezingArchRule` is against its violation store). The graphs are
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
        "plantuml-export" => check_plantuml_export(architecture, id, case, &expect),
        "baseline" => check_baseline(id, case, &expect),
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

/// A `FreezingArchRule` case: the rule (`ruleFamily` `element` or `slice`) under the name the
/// store knows it by (`baselineRule`), with `knownViolations` read from `baselines/<file>` by the
/// reader `--ignore-known` uses, evaluated by the engine `cruise` runs over the case's
/// `architecture`. `passes` is whether no finding is left at severity `error`: every current
/// violation is a stored one, which is `Freeze(rule).Check(architecture)` not throwing.
fn check_baseline(id: &str, case: &Value, expect: &Value) -> Option<String> {
    let mut rule = case.get("rule").cloned().unwrap_or(Value::Null);
    let name = case
        .get("baselineRule")
        .and_then(Value::as_str)
        .unwrap_or(id);
    if let Value::Object(map) = &mut rule {
        map.insert("name".into(), json!(name));
    }
    let family = match case.get("ruleFamily").and_then(Value::as_str) {
        Some("element") => "elements",
        Some("slice") => "slices",
        other => {
            return Some(format!(
                "{id}: ruleFamily {other:?} is not element or slice"
            ));
        }
    };
    let mut canonical = serde_json::Map::new();
    canonical.insert(family.into(), json!([rule]));
    let mut config = match rb_config::load::from_canonical(canonical, rb_config::CompatMode::Native)
    {
        Ok(config) => config,
        Err(e) => return Some(format!("{id}: the rule does not parse: {e}")),
    };
    let file = case
        .get("knownViolations")
        .and_then(Value::as_str)
        .unwrap_or_default();
    config.known_violations =
        match rb_config::load::known_violations_file(&conformance().join("baselines").join(file)) {
            Ok(entries) => entries,
            Err(e) => return Some(format!("{id}: {e}")),
        };
    let assemblies = strings(case.get("architecture"))
        .into_iter()
        .collect::<Vec<_>>();
    let document = match gate2_common::architecture_document(&conformance(), &assemblies) {
        Ok(document) => document,
        Err(e) => return Some(format!("{id}: {e}")),
    };
    let options = rb_rules::EvalOptions {
        liveness: false,
        ..rb_rules::EvalOptions::default()
    };
    let evaluation = match rb_rules::evaluate(document, &config, &options) {
        Ok(evaluation) => evaluation,
        Err(e) => return Some(format!("{id}: {e}")),
    };
    let passes = evaluation.error_count() == 0;
    let want = expect.get("passes").and_then(Value::as_bool);
    (want != Some(passes)).then(|| {
        let left: Vec<String> = evaluation
            .violations()
            .iter()
            .filter(|v| v.rule.severity == rb_model::Severity::Error)
            .map(|v| format!("{} -> {}", v.from, v.to))
            .collect();
        format!("{id}: expected passes={want:?}, got {passes}; not known: {left:?}")
    })
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

/// One element of a `from: elements` build: `{dependency: [origin, target, DependencyType]}` or
/// `{class: name}`.
fn export_element(value: &Value) -> Result<rb_rules::plantuml_export::Element, String> {
    use rb_rules::plantuml_export::{Dependency, DependencyType, Element};
    if let Some(name) = value.get("class").and_then(Value::as_str) {
        return Element::class(name).map_err(|e| e.to_string());
    }
    let parts: Vec<&str> = value
        .get("dependency")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let [origin, target, kind] = parts[..] else {
        return Err(format!("unknown element {value}"));
    };
    let kind = match kind {
        "OneToOne" => DependencyType::OneToOne,
        "OneToMany" => DependencyType::OneToMany,
        "OneToPackage" => DependencyType::OneToPackage,
        "PackageToOne" => DependencyType::PackageToOne,
        "PackageToPackage" => DependencyType::PackageToPackage,
        "OneToOneIfSameParentNamespace" => DependencyType::OneToOneIfSameParentNamespace,
        "PackageToPackageIfSameParentNamespace" => {
            DependencyType::PackageToPackageIfSameParentNamespace
        }
        "OneToOneCompact" => DependencyType::OneToOneCompact,
        "Circle" => DependencyType::Circle,
        "NoDependency" => DependencyType::NoDependency,
        other => return Err(format!("unknown DependencyType {other}")),
    };
    Dependency::new(origin, target, kind)
        .map(Element::Dependency)
        .map_err(|e| e.to_string())
}

/// Every constructor `HandleIllegalComponentNamesTest` calls, with `name` in each position: the
/// exceptions they raise, one per constructor that raised one.
fn illegal_name_errors(name: &str) -> Vec<rb_rules::plantuml_export::ExportException> {
    use rb_rules::plantuml_export::{Dependency, DependencyType, Element, SliceNode};
    [
        Dependency::new(name, "a", DependencyType::OneToOne).err(),
        Dependency::new("a", name, DependencyType::OneToOne).err(),
        Element::class(name).err(),
        Element::interface(name).err(),
        SliceNode::new(name, None, None).err(),
        Element::namespace(name).err(),
    ]
    .into_iter()
    .flatten()
    .map(|e| e.exception)
    .collect()
}

/// A `PlantUmlFileBuilder` case: `rule.from` is `types` (the loaded types in ordinal order,
/// `take` the first N or `only` those named), `namespaces`, `slices` (`Matching` or
/// `MatchingWithPackages`), `elements` (custom elements) or `names` (each checked against every
/// constructor), with the generation options by their `ArchUnitNET` names. `expect.text` is the
/// diagram upstream asserts, `expect.file` a diagram under `diagrams/generated/` that
/// `ArchUnitNET` itself wrote for the same selection, `expect.error` the exception.
fn check_plantuml_export(
    architecture: &Architecture<'_>,
    id: &str,
    case: &Value,
    expect: &Value,
) -> Option<String> {
    let build = case.get("rule").cloned().unwrap_or(Value::Null);
    if build.get("from").and_then(Value::as_str) == Some("names") {
        return check_illegal_names(id, &build, expect);
    }
    let diagram = match export_builder(architecture, id, &build) {
        Ok(diagram) => diagram,
        Err(e) => return Some(e),
    };
    match diagram.and_then(|b| b.render()) {
        Ok(text) => compare_diagram(id, &text, expect),
        Err(error) => (expect.get("error").and_then(Value::as_str) != Some(error.exception.name()))
            .then(|| format!("{id}: {error}")),
    }
}

/// `HandleIllegalComponentNamesTest`: every name in `rule.names` refused by all six
/// constructors with the expected exception.
fn check_illegal_names(id: &str, build: &Value, expect: &Value) -> Option<String> {
    use rb_rules::plantuml_export::ExportException;
    let names = strings(build.get("names"));
    let want = expect.get("error").and_then(Value::as_str);
    let wrong: Vec<String> = names
        .iter()
        .filter(|name| {
            let errors = illegal_name_errors(name);
            errors.len() != 6
                || errors
                    .iter()
                    .any(|e| Some(e.name()) != want || *e != ExportException::IllegalComponentName)
        })
        .map(|name| format!("{name:?}"))
        .collect();
    (names.len() != 8 || !wrong.is_empty())
        .then(|| format!("{id}: not refused by every constructor: {wrong:?}"))
}

/// The builder a case draws with: `Err` when the case itself is malformed.
fn export_builder(
    architecture: &Architecture<'_>,
    id: &str,
    build: &Value,
) -> Result<
    Result<rb_rules::plantuml_export::Builder, rb_rules::plantuml_export::ExportError>,
    String,
> {
    use rb_rules::plantuml_export::{
        Builder, GenerationOptions, export_namespaces, export_slices, export_types,
    };
    let flag = |key: &str| build.get(key).and_then(Value::as_bool).unwrap_or(false);
    let options = GenerationOptions {
        include_dependencies_to_other: flag("IncludeDependenciesToOther"),
        limit_dependencies: flag("LimitDependencies"),
        c4_style: flag("C4Style"),
        ..GenerationOptions::default()
    };
    match build.get("from").and_then(Value::as_str) {
        Some("types") => {
            let mut types = export_types(architecture);
            let only = strings(build.get("only"));
            if !only.is_empty() {
                types.retain(|t| only.contains(&t.full_name));
            }
            if let Some(take) = build.get("take").and_then(Value::as_u64) {
                types.truncate(usize::try_from(take).unwrap_or(usize::MAX));
            }
            Ok(Builder::new().with_types(&types, &options))
        }
        Some("namespaces") => {
            Ok(Builder::new().with_slices(&export_namespaces(architecture), &options))
        }
        Some("slices") => {
            let (pattern, packages) = match (
                build.get("Matching").and_then(Value::as_str),
                build.get("MatchingWithPackages").and_then(Value::as_str),
            ) {
                (Some(pattern), None) => (pattern, false),
                (None, Some(pattern)) => (pattern, true),
                _ => return Err(format!("{id}: one of Matching and MatchingWithPackages")),
            };
            let slicing = rb_rules::slices::slicing(architecture, pattern, packages, id)
                .map_err(|e| format!("{id}: {e}"))?;
            Ok(Builder::new().with_slices(&export_slices(&slicing), &options))
        }
        Some("elements") => {
            let elements = build
                .get("elements")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(export_element)
                .collect::<Result<Vec<_>, String>>()
                .map_err(|e| format!("{id}: {e}"))?;
            Ok(Ok(Builder::new().with_elements(elements)))
        }
        other => Err(format!("{id}: unknown rule.from {other:?}")),
    }
}

/// The diagram against `expect.text`, or `expect.file` under `diagrams/generated/`, with the
/// `graphDiffers` lines (drawn differently by the committed graph than by `ArchUnitNET`'s
/// loader, each of which must occur) taken out of both sides first.
fn compare_diagram(id: &str, text: &str, expect: &Value) -> Option<String> {
    let want = match (
        expect.get("text").and_then(Value::as_str),
        expect.get("file").and_then(Value::as_str),
    ) {
        (Some(text), _) => text.to_owned(),
        (None, Some(file)) => {
            match std::fs::read_to_string(conformance().join("diagrams/generated").join(file)) {
                Ok(text) => text,
                Err(e) => return Some(format!("{id}: {file}: {e}")),
            }
        }
        (None, None) => return Some(format!("{id}: expect neither text nor file")),
    };
    let differs = expect.get("graphDiffers");
    let (extra, missing) = (
        strings(differs.and_then(|d| d.get("extra"))),
        strings(differs.and_then(|d| d.get("missing"))),
    );
    let without = |text: &str, lines: &BTreeSet<String>| -> (String, usize) {
        let mut found = 0;
        let mut kept = String::new();
        for line in text.split_inclusive('\n') {
            if lines.contains(line.trim_end_matches('\n')) {
                found += 1;
            } else {
                kept.push_str(line);
            }
        }
        (kept, found)
    };
    let (text, extra_found) = without(text, &extra);
    let (want, missing_found) = without(&want, &missing);
    if extra_found != extra.len() || missing_found != missing.len() {
        return Some(format!(
            "{id}: graphDiffers is stale: {extra_found} of {} extra and {missing_found} of {} missing lines occur",
            extra.len(),
            missing.len()
        ));
    }
    if text.is_empty() || text != want {
        let first = text
            .lines()
            .zip(want.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(text.lines().count().min(want.lines().count()));
        return Some(format!(
            "{id}: the diagram differs from line {} ({:?} against {:?})",
            first + 1,
            text.lines().nth(first),
            want.lines().nth(first)
        ));
    }
    None
}

#[test]
fn every_ported_case_reproduces_upstream() -> Result<(), Box<dyn std::error::Error>> {
    let run = gate2_common::run_suite(&conformance(), check)?;
    gate2_common::report("gate2", "rb-gate2-failures.txt", &run)
}
