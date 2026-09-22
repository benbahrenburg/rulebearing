//! `rb-extract-ts`: the TypeScript and JavaScript extractor over `oxc_parser` and `oxc_resolver`.
//!
//! - Architecture: [`docs/architecture.md#extractors`](../../../docs/architecture.md#extractors)
//! - Decisions: [ADR-0012](../../../docs/adr/0012-oxc-for-typescript.md),
//!   [ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md)
//! - Plans: [Wave 0, Spike A](../../../docs/plans/pending/0000-wave-0-spike.md),
//!   [Wave 1, sub-wave 1C](../../../docs/plans/pending/0001-wave-1-typescript-parity.md)
//! - Requirements: [FR-EXT-TS-01](../../../docs/prd.md#fr-ext-ts-01) to [FR-EXT-TS-05](../../../docs/prd.md#fr-ext-ts-05)
//! - Specification: dependency-cruiser's 546 `test/extract` fixtures
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//!
//! Rule of the boundary: this crate reads source files and writes `rb_model` types only. It
//! must not depend on `rb-config` or `rb-rules`.

/// File extensions this extractor owns, from
/// [coverage § Extraction and resolution](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution).
pub const EXTENSIONS: &[&str] = &["js", "mjs", "cjs", "jsx", "ts", "tsx", "mts", "cts"];

/// Extensions handled by the Node sidecar rather than natively
/// ([ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md)).
pub const SIDECAR_EXTENSIONS: &[&str] = &["coffee", "litcoffee", "ls", "cjsx", "csx"];

/// Whether a path is one this extractor parses natively.
pub fn owns(path: &str) -> bool {
    extension(path).is_some_and(|e| EXTENSIONS.contains(&e))
}

/// Whether a path needs the sidecar.
pub fn needs_sidecar(path: &str) -> bool {
    extension(path).is_some_and(|e| SIDECAR_EXTENSIONS.contains(&e))
}

fn extension(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next()?;
    let (_, ext) = name.rsplit_once('.')?;
    Some(ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owns_typescript_and_javascript() {
        assert!(owns("src/a.ts"));
        assert!(owns("src/a.d.ts"));
        assert!(owns("src/a.mjs"));
        assert!(!owns("src/a.py"));
        assert!(!owns("README"));
    }

    #[test]
    fn sidecar_languages_are_separate() {
        assert!(needs_sidecar("lib/x.coffee"));
        assert!(!owns("lib/x.coffee"));
        assert!(!needs_sidecar("lib/x.ts"));
    }
}
