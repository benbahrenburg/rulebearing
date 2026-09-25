//! The element, slice and capability rules over TypeScript and Python code: the mappings the
//! capability table states, proven over the extractors' own fixture packages.
//!
//! - Plan: [Wave 2, Step 5](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#25-step-5-the-element-rule-engine-and-the-capability-table-2c)
//!   ("the TypeScript and Python mappings over the fixture packages") and
//!   [Step 6](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#26-step-6-slice-and-diagram-rules-2c)
//!   ("a TypeScript path-pattern slice ...; a Python dotted-pattern slice")
//! - Decisions: [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md) (an
//!   unanswerable key is an error), [ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md)
//!   (the engine reads graph documents, so the fixtures arrive as the extractors' committed
//!   expectations)
//! - Requirement: [FR-RULE-03](../../../docs/prd.md#fr-rule-03)
//!
//! The documents are `crates/rb-extract-ts/tests/fixtures/codelayer.expected.json` and
//! `crates/rb-extract-python/tests/fixtures/pkg.expected.json`, which those crates' tests keep
//! equal to what the extractors produce.

use std::path::Path;

use rb_config::elements::{parse_elements, parse_slices};
use rb_model::GraphDocument;
use rb_rules::elements::{Architecture, ElementError, Outcome, evaluate};
use rb_rules::slices::{self, SliceOutcome};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The code layer of one extractor's committed expectation, as a graph document.
fn document(expectation: &str) -> Result<GraphDocument> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(expectation);
    let value: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    Ok(GraphDocument {
        modules: serde_json::from_value(value["modules"].clone())?,
        code: Some(serde_json::from_value(value["code"].clone())?),
        ..GraphDocument::default()
    })
}

fn typescript() -> Result<GraphDocument> {
    document("rb-extract-ts/tests/fixtures/codelayer.expected.json")
}

fn python() -> Result<GraphDocument> {
    document("rb-extract-python/tests/fixtures/pkg.expected.json")
}

fn element(document: &GraphDocument, rule: Value) -> std::result::Result<Outcome, ElementError> {
    let mut rule = rule;
    rule["name"] = json!("r");
    let rules = parse_elements(&json!([rule])).map_err(|e| ElementError::Pattern {
        rule: "r".into(),
        pattern: e.to_string(),
    })?;
    evaluate(&Architecture::new(document), &rules[0])
}

fn slice(document: &GraphDocument, matching: &str, should: &str) -> Result<SliceOutcome> {
    let rules = parse_slices(&json!([{ "name": "s", "matching": matching, "should": should }]))?;
    Ok(slices::evaluate(&Architecture::new(document), &rules[0])?)
}

/// The selected objects, and which of them pass.
fn verdicts(outcome: &Outcome) -> (Vec<&str>, Vec<&str>) {
    let selected = outcome.results.iter().map(|r| r.object.as_str()).collect();
    let passing = outcome
        .results
        .iter()
        .filter(|r| r.passed)
        .map(|r| r.object.as_str())
        .collect();
    (selected, passing)
}

#[test]
fn typescript_visibility_reads_export() -> Result<()> {
    let ts = typescript()?;
    let outcome = element(
        &ts,
        json!({ "select": { "kind": "class", "where": { "resideInNamespace": "src/model/base.ts" } }, "should": { "bePublic": true } }),
    )?;
    assert_eq!(
        verdicts(&outcome),
        (
            vec!["src/model/base.ts#Internal", "src/model/base.ts#Record"],
            vec!["src/model/base.ts#Record"]
        ),
        "an exported class is public, any other internal to its module"
    );
    Ok(())
}

#[test]
fn typescript_attributes_read_decorators() -> Result<()> {
    let ts = typescript()?;
    let outcome = element(
        &ts,
        json!({ "select": { "kind": "class", "where": { "haveAnyAttributes": ["src/model/decorators.ts#Entity"] } }, "should": { "exist": true } }),
    )?;
    assert_eq!(
        verdicts(&outcome).0,
        ["src/model/base.ts#Record", "src/ui/widget.tsx#Widget"]
    );
    Ok(())
}

#[test]
fn typescript_cannot_answer_sealed_unless_the_rule_is_scoped() -> Result<()> {
    let ts = typescript()?;
    let refused = element(
        &ts,
        json!({ "select": { "kind": "class" }, "should": { "beSealed": true } }),
    );
    assert!(
        matches!(&refused, Err(ElementError::Unanswerable { key, language, .. }) if key == "beSealed" && language == "typescript"),
        "{refused:?}"
    );
    let scoped = element(
        &ts,
        json!({ "select": { "kind": "class", "language": "dotnet" }, "should": { "beSealed": true } }),
    )?;
    assert!(
        scoped.vacuous,
        "no .NET class in the fixture: vacuous, not unanswerable"
    );
    Ok(())
}

#[test]
fn typescript_slices_by_path() -> Result<()> {
    let ts = typescript()?;
    let independent = slice(&ts, "src/(**)//", "notDependOnEachOther")?;
    assert_eq!(
        independent.slices.keys().collect::<Vec<_>>(),
        ["legacy", "model", "ui"],
        "`(**)//` names a slice by the first folder; src/main.ts has none"
    );
    assert!(
        !independent.failures.is_empty(),
        "legacy extends ui's Store, ui's Widget extends model's Record"
    );
    let acyclic = slice(&ts, "src/(**)//", "beFreeOfCycles")?;
    assert!(acyclic.failures.is_empty(), "{:?}", acyclic.failures);
    Ok(())
}

#[test]
fn python_visibility_reads_the_underscore() -> Result<()> {
    let py = python()?;
    let outcome = element(
        &py,
        json!({ "select": { "kind": "class", "where": { "resideInNamespace": "app.shapes" } }, "should": { "notBePrivate": true } }),
    )?;
    let (selected, passing) = verdicts(&outcome);
    assert!(selected.contains(&"app.shapes._Private"));
    assert!(!passing.contains(&"app.shapes._Private"));
    assert!(passing.contains(&"app.shapes.Point"));
    Ok(())
}

#[test]
fn python_immutability_reads_frozen_dataclasses() -> Result<()> {
    let py = python()?;
    let outcome = element(
        &py,
        json!({ "select": { "kind": "class", "where": { "haveAnyAttributes": ["dataclasses.dataclass"] } }, "should": { "beImmutable": true } }),
    )?;
    assert_eq!(
        verdicts(&outcome),
        (
            vec!["app.shapes.Loose", "app.shapes.Point"],
            vec!["app.shapes.Point"]
        )
    );
    Ok(())
}

#[test]
fn python_cannot_answer_method_body_questions() -> Result<()> {
    let py = python()?;
    let refused = element(
        &py,
        json!({ "select": { "kind": "method" }, "should": { "beCalledBy": ["app.core.Engine"] } }),
    );
    assert!(
        matches!(&refused, Err(ElementError::Unanswerable { language, .. }) if language == "python"),
        "{refused:?}"
    );
    Ok(())
}

#[test]
fn python_slices_by_dotted_module() -> Result<()> {
    let py = python()?;
    let modules = slice(&py, "app.(*)", "notDependOnEachOther")?;
    assert_eq!(
        modules.slices.keys().collect::<Vec<_>>(),
        [
            "broken",
            "cli",
            "core",
            "plugins.greet",
            "shapes",
            "sub",
            "sub.deep",
            "sub.sibling",
            "util"
        ],
        "a Python slice holds modules, named by the dotted remainder"
    );
    assert!(!modules.failures.is_empty(), "cli imports util");
    let first = slice(&py, "app.(**)..", "beFreeOfCycles")?;
    assert_eq!(first.slices.keys().collect::<Vec<_>>(), ["plugins", "sub"]);
    assert!(
        first.failures.is_empty(),
        "plugins.greet imports sub.sibling, nothing goes back"
    );
    Ok(())
}

#[test]
fn python_sibling_packages_are_one_slice_each_with_segments() -> Result<()> {
    let py = python()?;
    let rules = parse_slices(&json!([{
        "name": "acyclic-siblings", "matching": "app.(*)", "segments": 1, "should": "beFreeOfCycles"
    }]))?;
    let outcome = slices::evaluate(&Architecture::new(&py), &rules[0])?;
    assert_eq!(
        outcome.slices.keys().collect::<Vec<_>>(),
        ["broken", "cli", "core", "plugins", "shapes", "sub", "util"],
        "`sub`, `sub.deep` and `sub.sibling` are the one sibling `sub`"
    );
    assert_eq!(outcome.failures.len(), 1, "{:?}", outcome.failures);
    assert_eq!(
        outcome.failures[0].slices,
        ["core", "plugins", "sub"],
        "core imports plugins.greet, which imports sub.sibling, and sub.deep imports core: the \
         cycle import-linter's acyclic_siblings finds"
    );
    Ok(())
}

#[test]
fn typescript_modules_reside_in_their_path_and_depend_on_their_imports() -> Result<()> {
    let ts = typescript()?;
    let select = json!({ "kind": "module", "language": "typescript",
        "where": { "resideInNamespaceMatching": "^src/model/" } });
    let apart = element(
        &ts,
        json!({ "select": select, "should": { "notDependOnAny": ["src/ui/store.ts"] } }),
    )?;
    let model = [
        "src/model/base.ts",
        "src/model/decorators.ts",
        "src/model/index.ts",
    ];
    assert_eq!(verdicts(&apart), (model.to_vec(), model.to_vec()));
    let decorated = element(
        &ts,
        json!({ "select": select, "should": { "dependOnAny": ["src/model/decorators.ts"] } }),
    )?;
    assert_eq!(
        verdicts(&decorated).1,
        ["src/model/base.ts", "src/model/index.ts"]
    );
    // `react` does not resolve, so only the edge to the model is judged.
    let only = element(
        &ts,
        json!({ "select": { "kind": "module", "where": { "haveFullName": "src/ui/widget.tsx" } },
                "should": { "onlyDependOn": ["src/model/index.ts"] } }),
    )?;
    assert_eq!(verdicts(&only).1, ["src/ui/widget.tsx"]);
    let main = element(
        &ts,
        json!({ "select": { "kind": "module", "where": { "haveName": "main.ts" } },
                "should": { "onlyDependOn": ["src/ui/store.ts"] } }),
    )?;
    assert_eq!(verdicts(&main), (vec!["src/main.ts"], vec![]));
    Ok(())
}

#[test]
fn python_modules_reside_in_their_dotted_name() -> Result<()> {
    let py = python()?;
    let outcome = element(
        &py,
        json!({ "select": { "kind": "module", "language": "python",
                            "where": { "resideInNamespaceMatching": "^app\\.sub" } },
                "should": { "notDependOnAny": ["src/app/core.py"] } }),
    )?;
    assert_eq!(
        verdicts(&outcome),
        (
            vec![
                "src/app/sub/__init__.py",
                "src/app/sub/deep.py",
                "src/app/sub/sibling.py"
            ],
            vec!["src/app/sub/__init__.py", "src/app/sub/sibling.py"]
        )
    );
    let exact = element(
        &py,
        json!({ "select": { "kind": "module", "where": { "resideInNamespace": "app.util" } },
                "should": { "exist": true } }),
    )?;
    assert_eq!(verdicts(&exact).0, ["src/app/util.py"]);
    Ok(())
}

#[test]
fn a_module_has_no_visibility_attributes_or_type_shape() -> Result<()> {
    let ts = typescript()?;
    for (key, value) in [
        ("bePublic", json!(true)),
        (
            "haveAnyAttributes",
            json!(["src/model/decorators.ts#Entity"]),
        ),
        ("beSealed", json!(true)),
        ("beVirtual", json!(true)),
    ] {
        let refused = element(
            &ts,
            json!({ "select": { "kind": "module" }, "should": { key: value } }),
        );
        assert!(
            matches!(&refused, Err(ElementError::Inapplicable { key: k, kind, .. }) if k == key && kind == "module"),
            "{key}: {refused:?}"
        );
    }
    Ok(())
}
