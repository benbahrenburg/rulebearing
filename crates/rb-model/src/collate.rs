//! String order as JavaScript's `localeCompare` gives it, which is how dependency-cruiser sorts a
//! module's dependencies and its violations.
//!
//! - Plans: [Wave 0, Step 8](../../../docs/plans/pending/0000-wave-0-spike.md#step-8-spike-a-rb-extract-ts-0c)
//!   (the extractor's dependency order), [Wave 1, Step 7](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-7-liveness-severity-ids-receipts-expires-ratchets-1b)
//!   (the engine's violation order); it lives here so both share one implementation
//! - Specification: dependency-cruiser 18.2.0 `extract-dependencies.mjs` (`compareDeps` uses
//!   `String.prototype.localeCompare`), which follows the Unicode root collation (CLDR/DUCET)
//! - Requirement: [FR-CORE-07](../../../docs/prd.md#fr-core-07) (deterministic output)
//!
//! Root collation is not byte order: punctuation and symbols sort before digits, digits before
//! letters, and letters compare without case first (`a` < `B` < `c`), with lower case before upper
//! case only as a tie-break. Specifiers and paths are ASCII in practice, so the ASCII table of the
//! root collation is written out here; any other character falls back to its code point after
//! every ASCII one, which keeps the order total and deterministic.

use std::cmp::Ordering;

/// The primary weight of a character: its position in the root order, case folded.
fn primary(c: char) -> u32 {
    // The CLDR root order of ASCII punctuation, which differs from code point order.
    const ORDER: &[char] = &[
        '\t', '\n', '\r', ' ', '_', '-', ',', ';', ':', '!', '?', '.', '\'', '"', '(', ')', '[',
        ']', '{', '}', '@', '*', '/', '\\', '&', '#', '%', '`', '^', '+', '<', '=', '>', '|', '~',
        '$',
    ];
    if let Some(at) = ORDER.iter().position(|p| *p == c) {
        return u32::try_from(at).unwrap_or(0);
    }
    let base = u32::try_from(ORDER.len()).unwrap_or(0);
    if c.is_ascii_digit() {
        return base + (c as u32 - '0' as u32);
    }
    if c.is_ascii_alphabetic() {
        return base + 10 + (c.to_ascii_lowercase() as u32 - 'a' as u32);
    }
    0x1000 + c as u32
}

/// `a.localeCompare(b)` for the strings dependency-cruiser sorts.
pub fn compare(a: &str, b: &str) -> Ordering {
    let primary_order = a.chars().map(primary).cmp(b.chars().map(primary));
    if primary_order != Ordering::Equal {
        return primary_order;
    }
    // Tertiary: lower case before upper case, position by position.
    let case = |c: char| u8::from(c.is_ascii_uppercase());
    a.chars()
        .map(case)
        .cmp(b.chars().map(case))
        .then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut items: Vec<&str>) -> Vec<&str> {
        items.sort_by(|a, b| compare(a, b));
        items
    }

    #[test]
    fn matches_node_locale_compare() {
        // Orders checked against `[...].sort((a, b) => a.localeCompare(b))` in Node 24.
        assert_eq!(sorted(vec!["b", "A", "a", "B"]), ["a", "A", "b", "B"]);
        assert_eq!(
            sorted(vec!["zeta", "Alpha", "beta"]),
            ["Alpha", "beta", "zeta"]
        );
        assert_eq!(
            sorted(vec!["a1", "a_", "a-", "a."]),
            ["a_", "a-", "a.", "a1"]
        );
        assert_eq!(
            sorted(vec!["fs cjs false", "./a es6 false", "../b es6 false"]),
            ["../b es6 false", "./a es6 false", "fs cjs false"]
        );
        assert_eq!(
            sorted(vec!["@scope/x", "#hash", "~tilde", "$dollar"]),
            ["@scope/x", "#hash", "~tilde", "$dollar"]
        );
        assert_eq!(compare("same", "same"), Ordering::Equal);
        assert_eq!(
            sorted(vec![
                "./a", "./A", "./b", "../x", ".", "..", "a/b", "a-b", "a.b", "a_b"
            ]),
            [
                ".", "..", "../x", "./a", "./A", "./b", "a_b", "a-b", "a.b", "a/b"
            ]
        );
    }

    #[test]
    fn shorter_prefix_sorts_first_and_non_ascii_sorts_last() {
        assert_eq!(compare("abc", "abcd"), Ordering::Less);
        assert_eq!(compare("é", "z"), Ordering::Greater);
    }
}
