//! The `PlantUML` builder against `ArchUnitNET`'s own output: one diagram per generation option
//! and slice form, each drawn by `ArchUnitNET` 0.13.4 over the committed `ArchUnitNETTests`
//! fixture and committed under `conformance/archunitnet/diagrams/generated/Oracle.*.puml` by
//! `conformance/archunitnet/tools/PlantUmlOracle`.
//!
//! - Coverage: [`ArchUnitNET` § `PlantUML`](../../../docs/artifacts/archunitnet-0.13.4-coverage.md#plantuml)
//! - Plan: [Wave 3, Step 9](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//! - Decision: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md)
//! - Requirement: [FR-OUT-02](../../../docs/prd.md#fr-out-02)
//!
//! The upstream tests themselves are ported as gate 2 cases (`tests/gate2.rs`, family
//! `plantuml-export`); these cover the options and slice forms those tests do not draw.

use std::path::{Path, PathBuf};

use rb_model::GraphDocument;
use rb_rules::elements::Architecture;
use rb_rules::plantuml_export::{
    Builder, GenerationOptions, export_slices, export_types,
    ignore_dependencies_to_children_and_parents,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/archunitnet")
}

fn document() -> Result<GraphDocument, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(root().join("graphs/ArchUnitNETTests.json"))?;
    Ok(serde_json::from_str(&text)?)
}

fn oracle(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(std::fs::read_to_string(
        root().join(format!("diagrams/generated/Oracle.{name}.puml")),
    )?)
}

fn slices(
    architecture: &Architecture<'_>,
    pattern: &str,
    packages: bool,
    options: &GenerationOptions<'_>,
) -> Result<String, Box<dyn std::error::Error>> {
    let slicing = rb_rules::slices::slicing(architecture, pattern, packages, "oracle")?;
    Ok(Builder::new()
        .with_slices(&export_slices(&slicing), options)?
        .render()?)
}

#[test]
fn slices_draw_as_archunitnet_draws_them() -> Result<(), Box<dyn std::error::Error>> {
    let document = document()?;
    let architecture = Architecture::new(&document);
    let limit = GenerationOptions {
        limit_dependencies: true,
        ..GenerationOptions::default()
    };
    let c4 = GenerationOptions {
        c4_style: true,
        ..GenerationOptions::default()
    };
    let default = GenerationOptions::default();
    for (name, pattern, packages, options) in [
        (
            "SlicesLimitDependencies",
            "ArchUnitNETTests.(*)",
            false,
            &limit,
        ),
        (
            "SlicesTwoAsterisks",
            "ArchUnitNETTests.(*).(*)",
            false,
            &default,
        ),
        (
            "SlicesDoubleAsterisk",
            "ArchUnitNETTests.(**)",
            false,
            &default,
        ),
        (
            "SlicesWithPackages",
            "ArchUnitNETTests.(*).(*).(*)",
            true,
            &default,
        ),
        (
            "SlicesWithPackagesLimitDependencies",
            "ArchUnitNETTests.(*).(*).(*)",
            true,
            &limit,
        ),
        (
            "SlicesWithPackagesC4Style",
            "ArchUnitNETTests.(*).(*)",
            true,
            &c4,
        ),
    ] {
        let drawn = slices(&architecture, pattern, packages, options)?;
        assert_eq!(drawn, oracle(name)?, "{name}");
        // Deterministic: a second run writes the same bytes.
        assert_eq!(drawn, slices(&architecture, pattern, packages, options)?);
    }
    Ok(())
}

#[test]
fn dependency_filters_draw_as_archunitnet_draws_them() -> Result<(), Box<dyn std::error::Error>> {
    let document = document()?;
    let architecture = Architecture::new(&document);
    let mut types = export_types(&architecture);
    types.truncate(100);
    let parents = |o: &str, t: &str| ignore_dependencies_to_children_and_parents(o, t);
    let filtered = GenerationOptions {
        dependency_filter: Some(&parents),
        ..GenerationOptions::default()
    };
    assert_eq!(
        Builder::new().with_types(&types, &filtered)?.render()?,
        oracle("TypesIgnoreDependenciesToChildrenAndParents")?
    );
    // `DependencyFilters.FocusOn(types)`: exactly one end of the arrow is a focused type.
    let focused = |name: &str| name.starts_with("ArchUnitNETTests.Dependencies.");
    let focus = |o: &str, t: &str| focused(o) != focused(t);
    let focusing = GenerationOptions {
        dependency_filter: Some(&focus),
        ..GenerationOptions::default()
    };
    assert_eq!(
        Builder::new().with_types(&types, &focusing)?.render()?,
        oracle("TypesFocusOn")?
    );
    Ok(())
}
