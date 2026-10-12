//! Declared edges through every reporter: each output type renders the committed result, whose
//! services are joined by declared edges, to its committed expected file, and the same result with
//! the marks taken off renders to what the reporters wrote before declared edges were marked.
//!
//! - Requirement: [FR-REACH-03](../../../docs/prd.md#fr-reach-03) ("distinguishable from a
//!   detected edge in every reporter")
//! - Decisions: [ADR-0078](../../../docs/adr/0078-fleet-and-declared-edges.md),
//!   [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md),
//!   [ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md)
//! - Plan: [plan 0004, Step 12](../../../docs/plans/pending/0004-wave-4-reach.md#24-steps-for-sub-wave-4d-fleet-and-declared-edges)
//! - Reference: [reporters.md § Declared edges](../../../docs/reporters.md#declared-edges)
//!
//! `fixtures/declared/result.json` is a fleet's result in small: a web app, an api and a worker,
//! joined by declared `http`, `queue` and `webhook` edges, with a finding on a declared edge, one
//! on an edge both declared and detected (which carries no mark), a cycle and a reachability path
//! through declared edges. `expected/<type>.txt` is each reporter's output for it;
//! `RB_UPDATE_SNAPSHOTS=1` rewrites those. `expected/unmarked/<type>.txt` is each reporter's output
//! for the same result without the marks (every edge detected), written by the reporters as they
//! were before any reporter read the marks (commit `c68a6d0f`): the proof that a result without
//! declared edges renders byte for byte as before. They are never regenerated.
//! `x-dot-webpage` is `dot`'s program drawn by GraphViz, so `dot` stands for it.

use std::path::{Path, PathBuf};

use rb_report::{OUTPUT_TYPES, ReportOptions, render};
use serde_json::Value;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The reporters that print no edge and no violation, so they cannot carry a mark.
const NO_EDGES: &[&str] = &["null", "metrics"];

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/declared")
}

fn result() -> Result<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(
        dir().join("result.json"),
    )?)?)
}

/// The result with every mark taken off: each declared dependency as a detected one would be.
fn unmarked(mut result: Value) -> Value {
    if let Some(Value::Array(modules)) = result.get_mut("modules") {
        for module in modules {
            if let Some(Value::Array(dependencies)) = module.get_mut("dependencies") {
                for dependency in dependencies {
                    if let Value::Object(map) = dependency {
                        let declared = map.remove("declared").is_some();
                        map.remove("declaredKind");
                        map.remove("comment");
                        if declared {
                            map.remove("dependencyKind");
                        }
                    }
                }
            }
        }
    }
    result
}

fn options() -> ReportOptions {
    ReportOptions {
        timestamp: "2026-10-12T00:00:00.000".into(),
        ..ReportOptions::default()
    }
}

fn types() -> impl Iterator<Item = &'static str> {
    OUTPUT_TYPES
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| *name != "x-dot-webpage")
}

fn rendered(output_type: &str, result: &Value) -> Result<String> {
    Ok(render(output_type, result, &options())?.output)
}

fn compare(path: &Path, actual: &str, update: bool) -> Result {
    if update && std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap_or(path))?;
        std::fs::write(path, actual)?;
    }
    let expected = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    assert_eq!(
        actual,
        expected,
        "{} changed; regenerate it with RB_UPDATE_SNAPSHOTS=1 only if that was intended",
        path.display()
    );
    Ok(())
}

#[test]
fn every_reporter_that_prints_an_edge_marks_a_declared_one() -> Result {
    let result = result()?;
    let plain = unmarked(result.clone());
    let mut compared = 0;
    for output_type in types() {
        let marked = rendered(output_type, &result)?;
        assert_eq!(
            marked,
            rendered(output_type, &result)?,
            "{output_type}: deterministic"
        );
        compare(
            &dir().join("expected").join(format!("{output_type}.txt")),
            &marked,
            true,
        )?;
        let without = rendered(output_type, &plain)?;
        if NO_EDGES.contains(&output_type) {
            assert_eq!(marked, without, "{output_type} prints no edge");
        } else {
            assert_ne!(
                marked, without,
                "{output_type} does not mark a declared edge"
            );
            assert!(marked.contains("declared"), "{output_type}");
        }
        compared += 1;
    }
    assert_eq!(compared, OUTPUT_TYPES.len() - 1);
    Ok(())
}

#[test]
fn without_the_marks_every_reporter_writes_what_it_wrote_before() -> Result {
    let plain = unmarked(result()?);
    for output_type in types() {
        let output = rendered(output_type, &plain)?;
        assert!(
            !output.contains("declared"),
            "{output_type} marks an edge nothing declared"
        );
        compare(
            &dir()
                .join("expected/unmarked")
                .join(format!("{output_type}.txt")),
            &output,
            false,
        )?;
    }
    Ok(())
}

#[test]
fn a_finding_on_an_edge_both_declared_and_detected_carries_no_mark() -> Result {
    let err = rendered("err", &result()?)?;
    assert!(
        err.contains("web/src/app.ts → api/src/Orders.cs (declared: http)\n"),
        "{err}"
    );
    assert!(
        err.contains("web/src/legacy.ts → api/src/Store.cs\n"),
        "{err}"
    );
    Ok(())
}
