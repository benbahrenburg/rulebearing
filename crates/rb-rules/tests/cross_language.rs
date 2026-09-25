//! The cross-language dependency-rule keys, `to.license` and `to.moreUnstable` over the graphs
//! the .NET and Python extractors write.
//!
//! - Plan: [Wave 2, Step 8](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#28-step-8-cross-language-rule-additions-per-language-dependencytypes-license-moreunstable-2d)
//!   ("a fixture config over the `TestAssembly` graph with one rule per key that must fire")
//! - Source: [design § Dependency rules](../../../docs/artifacts/design.md#dependency-rules-the-whole-of-dependency-cruiser-1820)
//!   (the cross-language additions), [dc coverage § Rules](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#rules)
//!   (`to.license`, `to.moreUnstable`), [dc coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options)
//!   (`metrics`)
//! - Decision: [ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) (the
//!   engine reads graph documents, so the fixtures arrive as the extractors' committed output)
//! - Requirement: [FR-RULE-02](../../../docs/prd.md#fr-rule-02)
//!
//! `conformance/archunitnet/graphs/TestAssembly.json` is what `rb-extract-dotnet` writes for
//! ArchUnitNET's `TestAssembly` (kept equal by that crate's `gate2_graphs` test);
//! `crates/rb-extract-python/tests/fixtures/pkg.expected.json` is the Python extractor's.

use std::path::Path;

use chrono::NaiveDate;
use rb_config::CompatMode;
use rb_config::load::from_canonical;
use rb_model::GraphDocument;
use rb_rules::evaluate::{EvalOptions, Evaluation, evaluate};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The module and code layers of a committed extraction, as a graph document (an extractor's
/// expectation also carries its receipt, which is not part of the document).
fn graph(relative: &str) -> Result<GraphDocument> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    let value: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    Ok(GraphDocument {
        modules: serde_json::from_value(value["modules"].clone())?,
        code: serde_json::from_value(value["code"].clone())?,
        ..GraphDocument::default()
    })
}

fn test_assembly() -> Result<GraphDocument> {
    graph("conformance/archunitnet/graphs/TestAssembly.json")
}

fn run(document: GraphDocument, rules: Value) -> Result<Evaluation> {
    let mut map = serde_json::Map::new();
    map.insert("forbidden".into(), rules);
    let config = from_canonical(map, CompatMode::Native)?;
    Ok(evaluate(
        document,
        &config,
        &EvalOptions {
            today: NaiveDate::from_ymd_opt(2026, 9, 24).unwrap_or_default(),
            ..EvalOptions::default()
        },
    )?)
}

/// `(rule, from, to)` for every violation, with the `TestAssembly/` prefix dropped.
fn found(evaluation: &Evaluation) -> Vec<(String, String, String)> {
    let short = |s: &str| s.trim_start_matches("TestAssembly/").to_owned();
    evaluation
        .violations()
        .iter()
        .map(|v| (v.rule.name.clone(), short(&v.from), short(&v.to)))
        .collect()
}

fn of<'a>(all: &'a [(String, String, String)], rule: &str) -> Vec<(&'a str, &'a str)> {
    all.iter()
        .filter(|(r, _, _)| r == rule)
        .map(|(_, f, t)| (f.as_str(), t.as_str()))
        .collect()
}

/// One rule per cross-language key, each written to fire on `TestAssembly`, and the plan's
/// example over its Domain folder.
fn one_rule_per_key() -> Value {
    json!([
        { "name": "language", "severity": "error",
          "from": { "language": "dotnet", "path": "Record1\\.cs$" },
          "to": { "path": "AbstractRecord\\.cs$" } },
        { "name": "namespace", "severity": "error",
          "from": { "namespace": "^TestAssembly\\.Domain\\.Entities$" },
          "to": { "namespace": "^TestAssembly\\.Domain\\.Marker$" } },
        { "name": "namespace-not", "severity": "error",
          "from": { "namespace": "^TestAssembly\\.Domain\\." },
          "to": { "namespaceNot": ["^TestAssembly\\.Domain\\.Marker$", "^TestAssembly\\.Domain\\.Entities$"] } },
        { "name": "project", "severity": "error",
          "from": { "project": "^TestAssembly\\.dll$", "path": "Slice3Class\\.cs$" },
          "to": { "project": "^TestAssembly\\.dll$" } },
        { "name": "project-not", "severity": "error",
          "from": { "projectNot": "^Other\\.dll$", "path": "/Class1\\.cs$" },
          "to": { "projectNot": "^Other\\.dll$" } },
        { "name": "assembly", "severity": "error",
          "from": { "assembly": "^TestAssembly$", "path": "Customer\\.cs$" },
          "to": { "assembly": "^TestAssembly$" } },
        { "name": "assembly-not", "severity": "error",
          "from": { "assemblyNot": "^Other$", "path": "IndependentOrigin\\.cs$" },
          "to": { "assemblyNot": "^Other$" } },
        { "name": "dependency-kind", "severity": "error",
          "from": { "language": "dotnet" },
          "to": { "dependencyKind": "inherits", "dependencyTypes": ["local"] } },
        { "name": "dependency-kind-not", "severity": "error",
          "from": { "path": "Entity2WithDependencyToEntity1\\.cs$" },
          "to": { "dependencyKindNot": ["field", "implements"], "dependencyTypes": ["local"] } },
        { "name": "entities-do-not-implement-markers", "severity": "error",
          "comment": "The plan's example, over TestAssembly's Domain folder.",
          "from": { "language": "dotnet", "namespace": "^TestAssembly\\.Domain\\.Entities$" },
          "to": { "namespace": "Marker$", "dependencyKind": ["inherits", "implements"] } }
    ])
}

#[test]
fn one_rule_per_key_fires_on_the_test_assembly() -> Result<()> {
    let evaluation = run(test_assembly()?, one_rule_per_key())?;
    let all = found(&evaluation);
    assert_eq!(
        of(&all, "language"),
        [("Record1.cs", "AbstractRecord.cs")],
        "{all:?}"
    );
    assert_eq!(
        of(&all, "namespace"),
        [
            ("Domain/Entities/Entity1.cs", "Domain/Marker/IEntity.cs"),
            (
                "Domain/Entities/Entity2WithDependencyToEntity1.cs",
                "Domain/Marker/IEntity.cs"
            ),
            (
                "Domain/Entities/EntityWithPublicSetters.cs",
                "Domain/Marker/IEntity.cs"
            ),
        ]
    );
    assert_eq!(
        of(&all, "namespace-not"),
        [(
            "Domain/Repository/RepositoryWithDependencyToService.cs",
            "Domain/Services/TestService.cs"
        )],
        "framework targets carry no namespaces, so namespaceNot leaves them out"
    );
    assert_eq!(
        of(&all, "project"),
        [(
            "Slices/Slice3/Slice3Class.cs",
            "Slices/Slice1/Slice1Class.cs"
        )]
    );
    assert_eq!(of(&all, "project-not"), [("Class1.cs", "Class2.cs")]);
    assert_eq!(
        of(&all, "assembly"),
        [
            (
                "PlantUml/Customers/Customer.cs",
                "PlantUml/Addresses/Address.cs"
            ),
            ("PlantUml/Customers/Customer.cs", "PlantUml/Orders/Order.cs"),
        ]
    );
    assert_eq!(
        of(&all, "assembly-not"),
        [(
            "Diagram/NoDependencies/SomeNamespace/IndependentOrigin.cs",
            "Diagram/NoDependencies/SomeNamespace/DependencyWithinNamespace.cs"
        )]
    );
    assert_eq!(
        of(&all, "dependency-kind"),
        [("Record1.cs", "AbstractRecord.cs")]
    );
    assert_eq!(
        of(&all, "dependency-kind-not"),
        [(
            "Domain/Entities/Entity2WithDependencyToEntity1.cs",
            "Domain/Entities/Entity1.cs"
        )]
    );
    assert_eq!(of(&all, "entities-do-not-implement-markers").len(), 3);
    assert!(evaluation.vacuous.is_empty(), "{:?}", evaluation.vacuous);
    Ok(())
}

#[test]
fn the_assembly_is_the_name_not_the_file_and_python_selects_nothing() -> Result<()> {
    let evaluation = run(
        test_assembly()?,
        json!([
            { "name": "assembly-is-not-the-file-name", "severity": "error",
              "from": { "path": "Customer\\.cs$" },
              "to": { "assembly": "\\.dll$" } },
            { "name": "project-is-the-file", "severity": "error",
              "from": { "path": "Customer\\.cs$" },
              "to": { "project": "\\.dll$" } },
            { "name": "no-python-here", "severity": "error",
              "from": { "language": "python" },
              "to": {} }
        ]),
    )?;
    let all = found(&evaluation);
    assert!(of(&all, "assembly-is-not-the-file-name").is_empty());
    assert_eq!(of(&all, "project-is-the-file").len(), 2);
    let vacuous: Vec<(&str, &str)> = evaluation
        .vacuous
        .iter()
        .map(|v| (v.name.as_str(), v.side.as_str()))
        .collect();
    assert_eq!(
        vacuous,
        [("no-python-here", "from")],
        "liveness counts the modules the cross-language keys select"
    );
    let stats = evaluation
        .rule_stats
        .iter()
        .find(|s| s.name == "assembly-is-not-the-file-name")
        .map(|s| s.from_matches);
    assert_eq!(stats, Some(1));
    Ok(())
}

#[test]
fn an_orphan_rule_narrows_by_language_and_namespace() -> Result<()> {
    let evaluation = run(
        graph("crates/rb-extract-python/tests/fixtures/pkg.expected.json")?,
        json!([
            { "name": "python-orphans", "severity": "error",
              "from": { "orphan": true, "language": "python", "namespace": "^app\\." },
              "to": {} },
            { "name": "python-orphans-outside-app", "severity": "error",
              "from": { "orphan": true, "language": "python", "namespaceNot": "^app\\." },
              "to": {} },
            { "name": "dotnet-orphans", "severity": "error",
              "from": { "orphan": true, "language": "dotnet" },
              "to": {} }
        ]),
    )?;
    let all = found(&evaluation);
    assert_eq!(
        of(&all, "python-orphans"),
        [("src/app/broken.py", "src/app/broken.py")]
    );
    assert!(of(&all, "python-orphans-outside-app").is_empty());
    assert!(of(&all, "dotnet-orphans").is_empty());
    let vacuous: Vec<&str> = evaluation.vacuous.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(vacuous, ["dotnet-orphans"]);
    Ok(())
}

#[test]
fn more_unstable_on_a_dotnet_graph() -> Result<()> {
    // Instability is efferent over total coupling, framework edges included: Record1 (1.0)
    // depends on AbstractRecord (0.875); in PlantUml, Address (0.6) depends on ProductCatalog
    // (0.78), and Customer (0.625) and Product (0.67) on Order (0.77).
    let evaluation = run(
        test_assembly()?,
        json!([
            { "name": "towards-instability", "severity": "error",
              "from": { "language": "dotnet", "path": "/PlantUml/" },
              "to": { "moreUnstable": true, "dependencyTypes": ["local"] } },
            { "name": "towards-stability", "severity": "error",
              "from": { "path": "Record1\\.cs$" },
              "to": { "moreUnstable": false, "path": "AbstractRecord\\.cs$" } },
            { "name": "folders-towards-instability", "severity": "error", "scope": "folder",
              "from": { "path": "^TestAssembly/PlantUml/" },
              "to": { "moreUnstable": true } }
        ]),
    )?;
    let modules = &evaluation.document.modules;
    let instability = |source: &str| {
        modules
            .iter()
            .find(|m| m.source == source)
            .and_then(|m| m.instability)
    };
    let all = found(&evaluation);
    for (from, to) in of(&all, "towards-instability") {
        let (a, b) = (
            instability(&format!("TestAssembly/{from}")),
            instability(&format!("TestAssembly/{to}")),
        );
        assert!(
            matches!((a, b), (Some(a), Some(b)) if a < b),
            "{from} {a:?} -> {to} {b:?}"
        );
    }
    assert_eq!(
        of(&all, "towards-instability"),
        [
            (
                "PlantUml/Addresses/Address.cs",
                "PlantUml/Catalog/ProductCatalog.cs"
            ),
            ("PlantUml/Customers/Customer.cs", "PlantUml/Orders/Order.cs"),
            ("PlantUml/Products/Product.cs", "PlantUml/Orders/Order.cs"),
        ]
    );
    assert_eq!(
        of(&all, "towards-stability"),
        [("Record1.cs", "AbstractRecord.cs")]
    );
    let folders = of(&all, "folders-towards-instability");
    assert_eq!(
        folders,
        [
            ("PlantUml/Addresses", "PlantUml/Catalog"),
            ("PlantUml/Customers", "PlantUml/Orders"),
            ("PlantUml/Products", "PlantUml/Orders"),
        ]
    );
    let folder_instability = |name: &str| {
        evaluation
            .document
            .folders
            .iter()
            .flatten()
            .find(|f| f.name == name)
            .and_then(|f| f.instability)
    };
    for (from, to) in folders {
        let (a, b) = (
            folder_instability(&format!("TestAssembly/{from}")),
            folder_instability(&format!("TestAssembly/{to}")),
        );
        assert!(
            matches!((a, b), (Some(a), Some(b)) if a < b),
            "{from} {a:?} -> {to} {b:?}"
        );
    }
    Ok(())
}

#[test]
fn license_on_the_python_graph() -> Result<()> {
    let evaluation = run(
        graph("crates/rb-extract-python/tests/fixtures/pkg.expected.json")?,
        json!([
            { "name": "no-mit", "severity": "error",
              "from": { "language": "python" },
              "to": { "license": "^MIT$" } },
            { "name": "only-apache", "severity": "error",
              "from": { "path": "^src/app/" },
              "to": { "licenseNot": "^Apache-2\\.0$" } },
            { "name": "no-gpl", "severity": "error",
              "from": {},
              "to": { "license": "GPL" } }
        ]),
    )?;
    let all = found(&evaluation);
    let site = [
        ("src/app/core.py", "fancylib"),
        ("src/app/core.py", "fancylib.sub"),
    ];
    assert_eq!(of(&all, "no-mit"), site);
    assert_eq!(
        of(&all, "only-apache"),
        site,
        "licenseNot needs a licence, as upstream's does"
    );
    assert!(of(&all, "no-gpl").is_empty());
    Ok(())
}

#[test]
fn an_assembly_over_python_modules_is_refused_not_silently_false() -> Result<()> {
    let python = || graph("crates/rb-extract-python/tests/fixtures/pkg.expected.json");
    let refused = run(
        python()?,
        json!([{ "name": "no-assembly", "from": {}, "to": { "assemblyNot": "x" } }]),
    );
    let message = refused.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(
        message.contains("rule `no-assembly`: `to.assemblyNot`")
            && message.contains("python")
            && message.contains("add `to.language`"),
        "{message}"
    );
    // The same rule scoped away from Python, and a key Python records, run.
    let scoped = run(
        python()?,
        json!([
            { "name": "scoped", "from": {}, "to": { "language": "dotnet", "assemblyNot": "x" } },
            { "name": "recorded", "from": { "namespace": "^app\\.cli$" }, "to": { "project": "app" } }
        ]),
    )?;
    assert!(of(&found(&scoped), "scoped").is_empty());
    assert!(!of(&found(&scoped), "recorded").is_empty());
    Ok(())
}
