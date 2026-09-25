//! The stable violation id, `RB-` plus eight hex characters
//! ([ADR-0015](../../../docs/adr/0015-stable-violation-id.md)).
//!
//! The hash covers the rule name, the `from` path, the `to` path and the dependency kind, in that
//! order, separated by `\n`. Line numbers are excluded so the id survives an unrelated edit above
//! the import. Changing this function invalidates every baseline in the wild, which is why the
//! fixed-vector test below exists ([FR-CORE-04](../../../docs/prd.md#fr-core-04)).

use sha2::{Digest, Sha256};

/// Prefix of every violation id.
pub const PREFIX: &str = "RB-";

/// Computes the stable id for a violation.
///
/// `dependency_kind` is the additive edge kind (`import`, `inherits`, ...); an element-rule
/// violation passes the empty string.
///
/// The id is what a reviewer cites, what SARIF fingerprints and what a baseline is keyed on, so
/// it must not move when an unrelated edit shifts the import down a line:
///
/// ```
/// use rb_model::violation_id::violation_id;
///
/// let id = violation_id(
///     "no-cross-app-imports",
///     "apps/web/src/x.ts",
///     "apps/worker/src/y.ts",
///     "import",
/// );
/// assert_eq!(id, "RB-a85578a3");
/// assert!(id.starts_with("RB-"));
/// ```
///
/// The four fields are joined with newlines before hashing, so a rule name or a path containing
/// a newline could collide with a different violation. Neither the configuration schema nor any
/// filesystem allows one, and the fixed-vector test below pins the scheme.
pub fn violation_id(rule: &str, from: &str, to: &str, dependency_kind: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(rule.as_bytes());
    hasher.update(b"\n");
    hasher.update(from.as_bytes());
    hasher.update(b"\n");
    hasher.update(to.as_bytes());
    hasher.update(b"\n");
    hasher.update(dependency_kind.as_bytes());
    let digest = hasher.finalize();
    let mut id = String::with_capacity(PREFIX.len() + 8);
    id.push_str(PREFIX);
    for byte in &digest[..4] {
        use std::fmt::Write as _;
        // Writing to a String cannot fail.
        let _ = write!(id, "{byte:02x}");
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_prefix_and_eight_hex_chars() {
        let id = violation_id(
            "no-cross-app-imports",
            "apps/web/src/x.ts",
            "apps/worker/src/y.ts",
            "import",
        );
        assert!(id.starts_with(PREFIX));
        assert_eq!(id.len(), PREFIX.len() + 8);
        assert!(id[PREFIX.len()..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn is_deterministic_and_order_sensitive() {
        let a = violation_id("r", "a", "b", "import");
        let b = violation_id("r", "a", "b", "import");
        let c = violation_id("r", "b", "a", "import");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn fixed_vector_guards_baselines() {
        // The first four bytes of SHA-256("r\na\nb\nimport"); recorded here so that any change to the
        // hashing scheme is a deliberate, reviewed break of every baseline.
        let id = violation_id("r", "a", "b", "import");
        assert_eq!(id, FIXED_VECTOR);
    }

    // Frozen on 2026-09-20; see the test above.
    const FIXED_VECTOR: &str = "RB-e362b09e";
}
