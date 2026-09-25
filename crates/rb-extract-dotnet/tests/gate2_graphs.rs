//! The graphs conformance gate 2 evaluates: each committed `ArchUnitNET` fixture assembly
//! extracted, stored as JSON so `rb-rules` reads them without depending on an extractor.
//!
//! - Plan: [Wave 2, Step 7](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#27-step-7-gate-2-porting-to-completion-2c)
//! - Decisions: [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md),
//!   [ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) (the engine
//!   depends on the document, never on an extractor)
//!
//! `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-extract-dotnet --test gate2_graphs` rewrites
//! `conformance/archunitnet/graphs/<Assembly>.json`; the test fails when a committed graph differs
//! from what the extractor now produces.

use std::path::{Path, PathBuf};

use rb_extract_dotnet::DotnetExtractor;
use rb_model::{DotnetOptions, Extractor, GraphDocument};

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/archunitnet")
}

#[test]
fn every_fixture_graph_is_current() -> Result<(), Box<dyn std::error::Error>> {
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
    assert!(names.len() >= 11, "{names:?}");
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
        "stale gate 2 graphs {stale:?}; regenerate with RB_UPDATE_SNAPSHOTS=1 and review the diff"
    );
    Ok(())
}
