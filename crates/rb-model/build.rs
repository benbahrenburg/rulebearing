//! Runs the documentation link check on every compile.
//!
//! Every crate in the workspace depends on `rb-model`, so building or testing anything runs this
//! once. The check is the same code `cargo xtask check-links` and the `lint` job run, and it
//! fails the build when a relative link between documents, or from a doc comment, does not
//! resolve. See:
//!
//! - Decision: `docs/adr/0023-documentation-link-and-lint-gates.md`
//! - Rule it enforces: `docs/adr/0001-record-architecture-decisions.md`, "link everything"
//! - Requirement: `docs/prd.md#nfr-doc-01`
//!
//! Set `RB_SKIP_DOC_LINK_CHECK=1` to compile without it, for example while moving a large
//! document; continuous integration never sets it, so the gate still holds on the pull request.

use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .map_or_else(|| manifest.clone(), Path::to_path_buf);

    // Re-run only when something the check reads has changed.
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=RB_SKIP_DOC_LINK_CHECK");
    // On a CI server links into the local-only plans and design are skipped (ADR-0039).
    println!("cargo::rerun-if-env-changed=CI");
    for dir in xtask::doclinks::MARKDOWN_ROOTS
        .iter()
        .chain(xtask::doclinks::RUST_ROOTS)
    {
        let watched = root.join(dir);
        if watched.is_dir() {
            println!("cargo::rerun-if-changed={}", watched.display());
        }
    }
    for file in ["README.md", "CLAUDE.md", "rulebearing.yaml"] {
        let watched = root.join(file);
        if watched.is_file() {
            println!("cargo::rerun-if-changed={}", watched.display());
        }
    }

    if std::env::var_os("RB_SKIP_DOC_LINK_CHECK").is_some() {
        println!("cargo::warning=documentation link check skipped (RB_SKIP_DOC_LINK_CHECK is set)");
        return;
    }

    match xtask::doclinks::check(&root) {
        Ok(report) if report.is_clean() => {}
        Ok(report) => {
            for broken in &report.broken {
                println!("cargo::error={broken}");
            }
            println!(
                "cargo::error=documentation link check: {} of {} links do not resolve; run `cargo xtask check-links`",
                report.broken.len(),
                report.checked
            );
            std::process::exit(1);
        }
        Err(error) => {
            println!("cargo::error=documentation link check could not run: {error}");
            std::process::exit(1);
        }
    }
}
