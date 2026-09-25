//! The crate dependency direction, asserted from `cargo metadata`.
//!
//! - Decision: [ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) (the
//!   table and rules 1 and 2)
//! - Plan: [Wave 0, Step 2](../../../docs/plans/pending/0000-wave-0-spike.md#step-2-cargo-workspace-and-the-ten-crates-0a)
//!   item 4
//! - Requirement: [FR-CORE-01](../../../docs/prd.md#fr-core-01)
//!
//! This is the stand-in for the repository's own `rulebearing.yaml`, which wave 1 enforces with
//! the tool itself. It reads only normal (non-dev, non-build) dependencies: `rb-model`'s build
//! dependency on `xtask` runs the documentation link check at compile time and links nothing
//! into the library ([ADR-0023](../../../docs/adr/0023-documentation-link-and-lint-gates.md)).

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::process::Command;

/// Each workspace crate and the workspace crates it may depend on, from ADR-0010's table.
const ALLOWED: &[(&str, &[&str])] = &[
    ("rb-model", &[]),
    ("rb-config", &["rb-model"]),
    ("rb-rules", &["rb-model", "rb-config"]),
    ("rb-extract-ts", &["rb-model"]),
    ("rb-extract-dotnet", &["rb-model"]),
    ("rb-extract-python", &["rb-model"]),
    ("rb-ingest", &["rb-model"]),
    ("rb-report", &["rb-model", "rb-rules"]),
    (
        "rb-cli",
        &[
            "rb-model",
            "rb-config",
            "rb-rules",
            "rb-extract-ts",
            "rb-extract-dotnet",
            "rb-extract-python",
            "rb-ingest",
            "rb-report",
        ],
    ),
    ("rb-node", &["rb-cli"]),
];

/// Normal dependencies on other workspace members, per workspace member.
fn workspace_edges() -> Result<BTreeMap<String, BTreeSet<String>>, Box<dyn Error>> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()?;
    assert!(output.status.success(), "cargo metadata failed");
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let packages = metadata["packages"].as_array().cloned().unwrap_or_default();
    let members: BTreeSet<String> = packages
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_owned))
        .collect();
    let mut edges = BTreeMap::new();
    for package in &packages {
        let Some(name) = package["name"].as_str() else {
            continue;
        };
        let deps: BTreeSet<String> = package["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|d| d["kind"].is_null())
            .filter_map(|d| d["name"].as_str())
            .filter(|d| members.contains(*d))
            .map(str::to_owned)
            .collect();
        edges.insert(name.to_owned(), deps);
    }
    Ok(edges)
}

#[test]
fn every_crate_depends_only_on_what_adr_0010_allows() -> Result<(), Box<dyn Error>> {
    let edges = workspace_edges()?;
    for (name, allowed) in ALLOWED {
        let actual = edges.get(*name).cloned().unwrap_or_default();
        let allowed: BTreeSet<String> = allowed.iter().map(|s| (*s).to_owned()).collect();
        let extra: Vec<&String> = actual.difference(&allowed).collect();
        assert!(
            extra.is_empty(),
            "{name} depends on {extra:?}, which ADR-0010 does not allow"
        );
    }
    Ok(())
}

#[test]
fn the_table_names_every_crate_in_the_workspace() -> Result<(), Box<dyn Error>> {
    let edges = workspace_edges()?;
    let listed: BTreeSet<&str> = ALLOWED.iter().map(|(n, _)| *n).collect();
    let unlisted: Vec<&String> = edges
        .keys()
        .filter(|name| name.starts_with("rb-") && !listed.contains(name.as_str()))
        .collect();
    assert!(
        unlisted.is_empty(),
        "{unlisted:?} are not in ADR-0010's table; add them there first"
    );
    Ok(())
}

#[test]
fn extractors_never_reach_the_engine_or_the_config() -> Result<(), Box<dyn Error>> {
    // Rule 2 of ADR-0010, stated on its own so a failure names the rule.
    let edges = workspace_edges()?;
    for extractor in ["rb-extract-ts", "rb-extract-dotnet", "rb-extract-python"] {
        let deps = edges.get(extractor).cloned().unwrap_or_default();
        for forbidden in ["rb-config", "rb-rules", "rb-report", "rb-cli"] {
            assert!(
                !deps.contains(forbidden),
                "{extractor} depends on {forbidden} (ADR-0010 rule 2)"
            );
        }
    }
    Ok(())
}
