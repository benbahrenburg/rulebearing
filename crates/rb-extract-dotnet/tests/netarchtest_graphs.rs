//! The graphs the NetArchTest half of conformance gate 2 evaluates: each committed NetArchTest
//! fixture assembly extracted, stored as JSON so `rb-rules` reads them without depending on an
//! extractor.
//!
//! - Plan: [Wave 2, Step 7](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#27-step-7-gate-2-porting-to-completion-2c)
//! - Decisions: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md),
//!   [ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) (the engine
//!   depends on the document, never on an extractor)
//!
//! `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-extract-dotnet --test netarchtest_graphs` rewrites
//! `conformance/netarchtest/graphs/<Assembly>.json`; the test fails when a committed graph differs
//! from what the extractor now produces.

use std::path::{Path, PathBuf};

use rb_extract_dotnet::DotnetExtractor;
use rb_model::{DotnetOptions, Extractor, GraphDocument};

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/netarchtest")
}

#[test]
fn every_netarchtest_fixture_graph_is_current() -> Result<(), Box<dyn std::error::Error>> {
    let fixtures = conformance().join("fixtures");
    let graphs = conformance().join("graphs");
    std::fs::create_dir_all(&graphs)?;
    let mut names: Vec<String> = std::fs::read_dir(&fixtures)?
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_str()
                .and_then(|n| n.strip_suffix(".dll"))
                .map(str::to_owned)
        })
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "NetArchTest.CrossAssemblyTest.A",
            "NetArchTest.CrossAssemblyTest.B",
            "NetArchTest.TestStructure"
        ]
    );
    let mut stale = Vec::new();
    for name in &names {
        let options = DotnetOptions {
            assemblies: Some(vec![format!("{name}.dll")]),
            ..DotnetOptions::default()
        };
        let extraction = DotnetExtractor.extract(std::slice::from_ref(&fixtures), &options)?;
        let document = GraphDocument {
            modules: extraction.modules,
            code: extraction.code,
            ..GraphDocument::default()
        };
        let text = format!("{}\n", serde_json::to_string_pretty(&document)?);
        let path = graphs.join(format!("{name}.json"));
        if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
            std::fs::write(&path, &text)?;
        }
        // Compared as JSON: a workspace build turns on serde_json's `preserve_order`, which
        // changes key order but not content.
        let committed: Option<serde_json::Value> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
        if committed != serde_json::from_str(&text).ok() {
            stale.push(name.clone());
        }
    }
    assert!(
        stale.is_empty(),
        "stale NetArchTest gate 2 graphs {stale:?}; regenerate with RB_UPDATE_SNAPSHOTS=1 and review the diff"
    );
    Ok(())
}

/// `ArchUnitNET`'s phase 8 moves a generic parameter's constraint types to the type or member that
/// declares the parameter; NetArchTest's dependency search finds them there too.
#[test]
fn generic_parameter_constraints_are_signature_dependencies()
-> Result<(), Box<dyn std::error::Error>> {
    let fixtures = conformance().join("fixtures");
    let options = DotnetOptions {
        assemblies: Some(vec!["NetArchTest.TestStructure.dll".to_owned()]),
        ..DotnetOptions::default()
    };
    let extraction = DotnetExtractor.extract(std::slice::from_ref(&fixtures), &options)?;
    let code = extraction.code.ok_or("no code layer")?;
    let example = "NetArchTest.TestStructure.Dependencies.Examples.ExampleDependency";
    let location = "NetArchTest.TestStructure.Dependencies.Search.DependencyLocation";
    let on_example = |deps: &[rb_model::ElementDependency]| {
        deps.iter()
            .any(|d| d.target == example && d.kind == "signature")
    };
    let class = code
        .types
        .iter()
        .find(|t| t.full_name == format!("{location}.GenericConstraintClass`1"))
        .ok_or("GenericConstraintClass`1 missing")?;
    assert!(on_example(&class.dependencies), "{:?}", class.dependencies);
    let method = code
        .members
        .iter()
        .find(|m| {
            m.declaring_type == format!("{location}.GenericConstraintMethod")
                && m.name == "Method(T)"
        })
        .ok_or("GenericConstraintMethod.Method missing")?;
    assert!(
        on_example(&method.dependencies),
        "{:?}",
        method.dependencies
    );
    let declaring = code
        .types
        .iter()
        .find(|t| t.full_name == format!("{location}.GenericConstraintMethod"))
        .ok_or("GenericConstraintMethod missing")?;
    assert!(
        on_example(&declaring.dependencies),
        "member dependencies roll up to the type"
    );
    let unconstrained = code
        .types
        .iter()
        .find(|t| t.full_name == "NetArchTest.TestStructure.Generic.GenericType`1")
        .ok_or("GenericType`1 missing")?;
    assert!(
        unconstrained
            .dependencies
            .iter()
            .all(|d| d.kind != "signature"),
        "an unconstrained parameter adds nothing: {:?}",
        unconstrained.dependencies
    );
    Ok(())
}
