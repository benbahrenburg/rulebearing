//! `xtask`: repository automation for Rulebearing.
//!
//! - Decision: [ADR-0023](../../docs/adr/0023-documentation-link-and-lint-gates.md)
//! - Architecture: [`docs/architecture.md#verification-strategy`](../../docs/architecture.md#verification-strategy)
//! - Plan: [Wave 0, sub-wave 0A](../../docs/plans/pending/0000-wave-0-spike.md)
//! - Requirements: [NFR-DOC-01](../../docs/prd.md#nfr-doc-01), [NFR-QUAL-02](../../docs/prd.md#nfr-qual-02)
//!
//! Two jobs: [`doclinks`] checks that every relative link between documents and from a doc
//! comment resolves, and the binary in `main.rs` is the one lint entry point for all four
//! languages. The library is compiled by `crates/rb-model/build.rs`, so the link check runs on
//! every compile as well as on every lint.

pub mod doclinks;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Directories the walkers never descend into: build output, dependencies and vendored checkouts.
pub const SKIP_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".git",
    ".graph",
    "checkouts",
    "upstream",
    "dist",
    "obj",
    "bin",
    ".venv",
    "__pycache__",
];

/// Walks `dir` recursively, appending files whose extension is in `extensions`.
///
/// Entries are visited in sorted order so two runs produce the same report, and [`SKIP_DIRS`]
/// are never entered.
///
/// # Errors
/// Returns the underlying error when a directory cannot be listed.
pub fn walk(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name.as_ref()) {
                walk(&path, extensions, out)?;
            }
        } else if has_extension(&path, extensions) {
            out.push(path);
        }
    }
    Ok(())
}

/// Files directly inside `dir` whose extension is in `extensions`, without descending.
///
/// # Errors
/// Returns the underlying error when the directory cannot be listed.
pub fn walk_shallow(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        if path.is_file() && has_extension(&path, extensions) {
            out.push(path);
        }
    }
    Ok(())
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| extensions.contains(&ext))
}

/// True when the repository holds at least one file with one of `extensions` under any of `dirs`.
///
/// The lint entry point uses this to decide whether a language's linter applies yet: the
/// TypeScript, C# and Python trees are created wave by wave, and a linter with nothing to lint
/// is reported as not applicable rather than as a pass.
///
/// # Errors
/// Returns the underlying error when a directory cannot be listed.
pub fn any_file(root: &Path, dirs: &[&str], extensions: &[&str]) -> io::Result<bool> {
    let mut found = Vec::new();
    for dir in dirs {
        walk(&root.join(dir), extensions, &mut found)?;
        if !found.is_empty() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The repository root, derived from this crate's manifest directory.
#[must_use]
pub fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map_or(manifest.clone(), Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_root_holds_the_workspace_manifest() {
        assert!(repo_root().join("Cargo.toml").is_file());
        assert!(repo_root().join("docs").is_dir());
    }

    #[test]
    fn walk_finds_markdown_and_skips_build_output() -> io::Result<()> {
        let root = repo_root();
        let mut found = Vec::new();
        walk(&root.join("docs"), &["md"], &mut found)?;
        assert!(found.iter().any(|p| p.ends_with("architecture.md")));
        assert!(
            !found
                .iter()
                .any(|p| p.components().any(|c| c.as_os_str() == "target"))
        );
        Ok(())
    }

    #[test]
    fn walk_shallow_does_not_descend() -> io::Result<()> {
        let root = repo_root();
        let mut found = Vec::new();
        walk_shallow(&root, &["md"], &mut found)?;
        assert!(found.iter().any(|p| p.ends_with("README.md")));
        assert!(!found.iter().any(|p| p.ends_with("architecture.md")));
        Ok(())
    }

    #[test]
    fn walk_shallow_ignores_a_directory_named_like_a_file() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("rb-walk-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("decoy.md"))?;
        fs::write(root.join("real.md"), "# real\n")?;
        let mut found = Vec::new();
        walk_shallow(&root, &["md"], &mut found)?;
        assert_eq!(found.len(), 1, "a directory called decoy.md is not a file");
        assert!(found[0].ends_with("real.md"));
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn walk_on_a_missing_directory_is_empty() -> io::Result<()> {
        let mut found = Vec::new();
        walk(&repo_root().join("no-such-dir"), &["md"], &mut found)?;
        assert!(found.is_empty());
        Ok(())
    }

    #[test]
    fn every_language_keeps_its_linter_configuration() {
        // The TypeScript, Python and C# trees arrive wave by wave, so their linters report
        // "not applicable" until then. This test is what stops the configuration being deleted
        // or renamed in the meantime (docs/adr/0023-documentation-link-and-lint-gates.md).
        let root = repo_root();
        for file in [
            "rustfmt.toml",          // rust: formatting
            "deny.toml",             // rust: licences and advisories
            "eslint.config.mjs",     // typescript: lint
            ".prettierrc.json",      // typescript: formatting
            "tsconfig.json",         // typescript: the project the type-aware rules read
            "tsconfig.base.json",    // typescript: shared compiler settings
            "package.json",          // typescript: the linters themselves
            "pyproject.toml",        // python: ruff, mypy, pytest
            "Directory.Build.props", // c#: analyzers, warnings as errors, coverage floor
            ".editorconfig",         // c#: style severities
            ".cargo/config.toml",    // the cargo lint aliases
        ] {
            assert!(
                root.join(file).is_file(),
                "missing linter configuration: {file}"
            );
        }
    }

    #[test]
    fn python_configuration_names_both_linters() -> io::Result<()> {
        let text = fs::read_to_string(repo_root().join("pyproject.toml"))?;
        assert!(text.contains("[tool.ruff]"), "ruff configuration missing");
        assert!(text.contains("[tool.mypy]"), "mypy configuration missing");
        assert!(
            text.contains("--cov-fail-under=70"),
            "the 70% coverage floor is not set"
        );
        Ok(())
    }

    #[test]
    fn any_file_reports_rust_sources_and_not_fictional_ones() -> io::Result<()> {
        let root = repo_root();
        assert!(any_file(&root, &["crates"], &["rs"])?);
        assert!(!any_file(&root, &["crates"], &["zzz"])?);
        Ok(())
    }
}
