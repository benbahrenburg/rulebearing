//! `rb-extract-python`: the Python extractor over `ruff_python_parser`.
//!
//! - Architecture: [`docs/architecture.md#extractors`](../../../docs/architecture.md#extractors)
//! - Decision: [ADR-0013](../../../docs/adr/0013-ruff-parser-for-python.md)
//! - Plan: [Wave 2, sub-wave 2B](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md)
//! - Requirements: [FR-EXT-PY-01](../../../docs/prd.md#fr-ext-py-01), [FR-EXT-PY-02](../../../docs/prd.md#fr-ext-py-02)
//! - Specification: import-linter's contracts and tests on the Python oracle repositories
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//!
//! Rule of the boundary: this crate reads `.py` files and writes `rb_model` types only.

/// The `dependencyTypes` vocabulary for Python, from
/// [design § One engine, three languages](../../../docs/artifacts/design.md#one-engine-three-languages-one-monorepo).
pub const DEPENDENCY_TYPES: &[&str] = &[
    "local",
    "stdlib",
    "site",
    "type-only",
    "dynamic",
    "unresolved",
];

/// Resolves a relative import (`from ..a import b`) against the importing module's dotted
/// package, per [design § What each extractor has to get right](../../../docs/artifacts/design.md#what-each-extractor-has-to-get-right).
///
/// Returns `None` when the import climbs above the top-level package.
pub fn resolve_relative(package: &str, level: usize, name: &str) -> Option<String> {
    let mut parts: Vec<&str> = if package.is_empty() {
        vec![]
    } else {
        package.split('.').collect()
    };
    // level 1 = the current package; each further level climbs one package.
    if level == 0 {
        return Some(name.to_owned());
    }
    for _ in 1..level {
        parts.pop()?;
    }
    if !name.is_empty() {
        parts.extend(name.split('.'));
    }
    Some(parts.join("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_current_and_parent_packages() {
        assert_eq!(
            resolve_relative("pkg.sub", 1, "mod").as_deref(),
            Some("pkg.sub.mod")
        );
        assert_eq!(
            resolve_relative("pkg.sub", 2, "mod").as_deref(),
            Some("pkg.mod")
        );
        assert_eq!(resolve_relative("pkg.sub", 2, "").as_deref(), Some("pkg"));
        assert_eq!(
            resolve_relative("pkg", 0, "os.path").as_deref(),
            Some("os.path")
        );
    }

    #[test]
    fn refuses_to_climb_above_the_root() {
        assert_eq!(resolve_relative("pkg", 3, "x"), None);
    }
}
