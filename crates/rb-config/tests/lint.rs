//! `config lint`: one fixture per finding class, under `tests/lint/`.
//!
//! - Plan: [Wave 1, Step 4](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-4-config-convert-config-expand-config-lint-shorthands-1a)
//!   ("One fixture per finding under `rb-config/tests/lint/`")
//! - Requirement: [FR-CFG-05](../../../docs/prd.md#fr-cfg-05); `type-only-on-dotnet` is
//!   [FR-RULE-02](../../../docs/prd.md#fr-rule-02)

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
        "type-only-on-dotnet",
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

#[test]
fn type_only_on_dotnet_is_the_only_finding_of_its_fixture() -> Result<(), Box<dyn Error>> {
    assert_eq!(
        codes("type-only-on-dotnet.yaml", true)?,
        ["type-only-on-dotnet"]
    );
    let config = load(
        &fixture("type-only-on-dotnet.yaml"),
        &LoadOptions::default(),
    )?;
    assert!(
        config
            .warnings
            .iter()
            .any(|w| w.rule.as_deref() == Some("no-type-imports-into-domain")
                && w.message
                    .contains("`from.language` limits the rule to .NET")),
        "the loader warns as well: {:?}",
        config.warnings
    );
    Ok(())
}
