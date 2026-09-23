//! `config lint`: one fixture per finding class, under `tests/lint/`.
//!
//! - Plan: [Wave 1, Step 4](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-4-config-convert-config-expand-config-lint-shorthands-1a)
//!   ("One fixture per finding under `rb-config/tests/lint/`")
//! - Requirement: [FR-CFG-05](../../../docs/prd.md#fr-cfg-05)

use std::error::Error;
use std::path::PathBuf;

use rb_config::lint::{LintOptions, lint};
use rb_config::{LoadOptions, load};
use rb_model::GraphDocument;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/lint")
        .join(name)
}

fn codes(file: &str, require_token: bool) -> Result<Vec<&'static str>, Box<dyn Error>> {
    let config = load(&fixture(file), &LoadOptions::default())?;
    let graph: GraphDocument =
        serde_json::from_str(&std::fs::read_to_string(fixture("graph.json"))?)?;
    let findings = lint(
        &config,
        Some(&graph),
        LintOptions {
            require_comment_token: require_token,
        },
    );
    Ok(findings.iter().map(|f| f.code).collect())
}

#[test]
fn each_fixture_raises_its_finding() -> Result<(), Box<dyn Error>> {
    for code in [
        "no-fix",
        "fix-restates-name",
        "shadowed",
        "overlapping-allowed",
        "allowed-admits-everything",
        "missing-decision-token",
        "never-matches",
        "severity-below-error",
    ] {
        let found = codes(&format!("{code}.yaml"), true)?;
        assert!(found.contains(&code), "{code}.yaml gave {found:?}");
    }
    Ok(())
}

#[test]
fn a_clean_config_has_no_findings() -> Result<(), Box<dyn Error>> {
    assert_eq!(codes("clean.yaml", true)?, Vec::<&str>::new());
    Ok(())
}

#[test]
fn tokens_are_only_required_when_asked() -> Result<(), Box<dyn Error>> {
    assert!(!codes("missing-decision-token.yaml", false)?.contains(&"missing-decision-token"));
    Ok(())
}

#[test]
fn graph_findings_need_a_graph() -> Result<(), Box<dyn Error>> {
    let config = load(&fixture("never-matches.yaml"), &LoadOptions::default())?;
    assert!(lint(&config, None, LintOptions::default()).is_empty());
    Ok(())
}
