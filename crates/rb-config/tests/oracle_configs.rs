//! Every dependency-cruiser configuration in the test-bed manifest loads without edits.
//!
//! - Plan: [Wave 1, Step 1](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-1-config-model-and-the-two-front-ends-1a)
//!   ("Done when every manifest config loads without edits")
//! - Decisions: [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md),
//!   [ADR-0006](../../../docs/adr/0006-embedded-quickjs-config-evaluator.md),
//!   [ADR-0027](../../../docs/adr/0027-pure-path-and-url-modules-in-the-config-sandbox.md)
//! - Requirement: [FR-CFG-01](../../../docs/prd.md#fr-cfg-01)
//!
//! The configurations are not committed (a test bed is read-only); `scripts/fetch-oracle-configs.sh`
//! downloads them at their pinned commits into `target/oracle-configs/`. Without them the test
//! prints a note and passes, unless `RB_ORACLE_CONFIGS=required`, which CI sets after fetching.
//! `invertase/react-native-firebase` reads and writes the filesystem, so the sandbox must refuse
//! it with a message naming `--config-via-node`; every other row must load in the sandbox.

use std::error::Error;
use std::path::PathBuf;

use rb_config::{LoadOptions, load};

/// The row whose configuration needs Node (ADR-0027, Context).
const NEEDS_NODE: &str = "invertase/react-native-firebase";

fn rows() -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testbeds/manifest.yaml");
    let text = std::fs::read_to_string(manifest)?;
    let value: serde_yaml::Value = serde_yaml::from_str(&text)?;
    let mut out = Vec::new();
    for row in value["rows"].as_sequence().into_iter().flatten() {
        if row["tool"].as_str() == Some("dependency-cruiser")
            && let (Some(repo), Some(config)) = (row["repo"].as_str(), row["config"].as_str())
        {
            out.push((repo.to_owned(), config.to_owned()));
        }
    }
    Ok(out)
}

#[test]
fn every_manifest_config_loads() -> Result<(), Box<dyn Error>> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/oracle-configs");
    let required = std::env::var("RB_ORACLE_CONFIGS").as_deref() == Ok("required");
    let rows = rows()?;
    assert!(
        rows.len() >= 10,
        "the manifest lists {} dependency-cruiser rows",
        rows.len()
    );
    let mut loaded = 0;
    for (repo, config) in &rows {
        let path = base.join(repo.replace('/', "__")).join(config);
        if !path.exists() {
            assert!(
                !required,
                "{} is missing; run scripts/fetch-oracle-configs.sh",
                path.display()
            );
            eprintln!("oracle_configs: {repo} not fetched; run scripts/fetch-oracle-configs.sh");
            continue;
        }
        let result = load(&path, &LoadOptions::default());
        if repo == NEEDS_NODE {
            let message = result.err().map(|e| e.to_string()).unwrap_or_default();
            assert!(message.contains("--config-via-node"), "{repo}: {message}");
        } else {
            let config = result.map_err(|e| format!("{repo}: {e}"))?;
            let rules = config.rules.dependencies.forbidden.len()
                + config.rules.dependencies.allowed.len()
                + config.rules.dependencies.required.len();
            assert!(rules > 0, "{repo} loaded no rules");
            eprintln!(
                "oracle_configs: {repo} loaded, {rules} rules, {} warning(s)",
                config.warnings.len()
            );
        }
        loaded += 1;
    }
    eprintln!("oracle_configs: {loaded} of {} rows checked", rows.len());
    Ok(())
}
