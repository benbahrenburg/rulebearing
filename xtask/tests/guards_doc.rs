//! The `guards-doc` check over whole trees: the fixtures under `tests/guards_doc/`.
//!
//! - Plan: [plan 0005, Step 5](../../docs/plans/pending/0005-guard-catalogue.md#step-5-the-guards-doc-check-in-xtask-5b)
//! - Requirement: [NFR-DOC-01](../../docs/prd.md#nfr-doc-01)
//!
//! Each folder is a small repository root: a page at `docs/guards.md` and one fixture at
//! `cookbook/guards/sample/`. The last test runs the check over this repository's own page.

use std::path::{Path, PathBuf};

use xtask::guards_doc::{Report, check};
use xtask::repo_root;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn tree(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/guards_doc")
        .join(name)
}

fn findings(report: &Report) -> Vec<String> {
    report.findings.iter().map(ToString::to_string).collect()
}

#[test]
fn a_matching_page_passes() -> Result {
    let report = check(&tree("matching"))?;
    assert_eq!(findings(&report), Vec::<String>::new());
    assert!(report.is_clean());
    // The whole file, and one rule of it.
    assert_eq!(report.blocks, 2);
    Ok(())
}

#[test]
fn a_mismatching_page_names_the_slug_and_the_first_differing_line() -> Result {
    let report = check(&tree("mismatching"))?;
    assert_eq!(
        findings(&report),
        [
            "docs/guards.md:9: guard=sample: differs from the fixture: the block has \"        severity: warm\", the fixture has \"        severity: warn\""
        ]
    );
    Ok(())
}

#[test]
fn an_unknown_slug_is_named() -> Result {
    let report = check(&tree("unknown-slug"))?;
    assert_eq!(
        findings(&report),
        ["docs/guards.md:3: guard=absent: no fixture at cookbook/guards/absent/rulebearing.yaml"]
    );
    Ok(())
}

#[test]
fn an_unknown_rule_is_named() -> Result {
    let report = check(&tree("unknown-rule"))?;
    assert_eq!(
        findings(&report),
        [
            "docs/guards.md:3: guard=sample#absent: cookbook/guards/sample/rulebearing.yaml has no rule named `absent`"
        ]
    );
    Ok(())
}

/// A fixture no block copies fails, so a page that keeps some blocks and loses others cannot pass.
/// The folder without a `rulebearing.yaml` is not a fixture.
#[test]
fn a_fixture_with_no_block_is_named() -> Result {
    let report = check(&tree("uncovered"))?;
    assert_eq!(
        findings(&report),
        ["docs/guards.md:1: guard=second: no block copies cookbook/guards/second/rulebearing.yaml"]
    );
    Ok(())
}

#[test]
fn a_missing_page_is_an_error_and_not_a_pass() {
    assert!(check(&tree("no-such-tree")).is_err());
}

#[test]
fn a_missing_fixtures_folder_is_an_error_and_not_a_pass() {
    assert!(check(&tree("no-fixtures")).is_err());
}

/// This repository's page, as `cargo xtask lint` checks it, and it is not vacuous.
#[test]
fn this_repository_page_matches_its_fixtures() -> Result {
    let report = check(&repo_root())?;
    assert_eq!(findings(&report), Vec::<String>::new());
    assert!(report.blocks > 0);
    assert!(report.slugs.contains("no-circular"));
    Ok(())
}
