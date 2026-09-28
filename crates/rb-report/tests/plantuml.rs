//! The `plantuml` reporter over the committed `TestAssembly` graph: one diagram per `from` form
//! and per generation option, each compared byte for byte with a committed expected `.puml`
//! under `tests/fixtures/plantuml/`.
//!
//! - Plan: [Wave 3, Step 9](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//!   ("one fixture per generation option, compared against a committed expected `.puml`")
//! - Coverage: [`ArchUnitNET` § `PlantUML`](../../../docs/artifacts/archunitnet-0.13.4-coverage.md#plantuml)
//! - Decision: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md)
//! - Requirement: [FR-OUT-02](../../../docs/prd.md#fr-out-02)
//!
//! The forms that are `ArchUnitNET`'s text (types, packages, C4) are proven against `ArchUnitNET`
//! itself by gate 2 (`crates/rb-rules/tests/gate2.rs`, `plantuml_export.rs`); these fixtures pin
//! the reporter's output so a change to it is a reviewable diff. `RB_UPDATE_SNAPSHOTS=1`
//! rewrites them.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/plantuml")
        .join(format!("{name}.puml"))
}

fn graph() -> Result<Value, Box<dyn std::error::Error>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/archunitnet/graphs/TestAssembly.json");
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

#[test]
fn each_form_and_generation_option_writes_its_committed_diagram()
-> Result<(), Box<dyn std::error::Error>> {
    let graph = graph()?;
    let slices = "TestAssembly.PlantUml.(*)";
    let cases = [
        ("namespaces", json!({ "from": "namespaces" })),
        ("slices", json!({ "from": "slices", "Matching": slices })),
        ("types", json!({ "from": "types" })),
        ("folders", json!({ "from": "folders" })),
        (
            "LimitDependencies",
            json!({ "from": "namespaces", "LimitDependencies": true }),
        ),
        (
            "C4Style",
            json!({ "from": "slices", "MatchingWithPackages": "TestAssembly.(*).(*)", "C4Style": true }),
        ),
        (
            "FocusOn",
            json!({ "from": "namespaces", "FocusOn": "^TestAssembly\\.Slices\\." }),
        ),
        (
            "IncludeDependenciesToOther",
            json!({ "from": "slices", "Matching": slices, "IncludeDependenciesToOther": true }),
        ),
        (
            "DependencyFilters",
            json!({ "from": "namespaces", "DependencyFilters": [
                "IgnoreDependenciesToChildrenAndParents", "^TestAssembly\\.Domain\\."
            ] }),
        ),
    ];
    let update = std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some();
    let mut differ = Vec::new();
    for (name, section) in cases {
        let options = rb_report::ReportOptions::default();
        let rendered = rb_report::render_with("plantuml", &graph, &options, Some(&section))?;
        assert_eq!(rendered.exit_code, 0, "{name}");
        let again = rb_report::render_with("plantuml", &graph, &options, Some(&section))?;
        assert_eq!(rendered.output, again.output, "{name}: deterministic");
        let path = fixture(name);
        if update {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &rendered.output)?;
            continue;
        }
        if std::fs::read_to_string(&path)? != rendered.output {
            differ.push(name);
        }
    }
    assert!(
        differ.is_empty(),
        "{differ:?} changed; if that was intended, regenerate with RB_UPDATE_SNAPSHOTS=1"
    );
    Ok(())
}

#[test]
fn the_flag_wins_over_the_section() -> Result<(), Box<dyn std::error::Error>> {
    let graph = graph()?;
    let options = rb_report::ReportOptions {
        plantuml_from: Some("types".into()),
        ..rb_report::ReportOptions::default()
    };
    let section = json!({ "from": "namespaces" });
    let rendered = rb_report::render_with("plantuml", &graph, &options, Some(&section))?;
    assert_eq!(rendered.output, std::fs::read_to_string(fixture("types"))?);
    Ok(())
}
