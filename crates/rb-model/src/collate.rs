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

/// The CLDR root order of ASCII: punctuation and symbols (which differ from code point order),
/// then digits, then letters, case folded.
const ORDER: &str = "\t\n\r _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789abcdefghijklmnopqrstuvwxyz";

/// The primary weight of a character: the root order's class (0 for ASCII in [`ORDER`], 1 for
/// anything else) and the position within it (for anything else, its code point).
fn primary(c: char) -> (u8, u32) {
    let folded = c.to_ascii_lowercase();
    ORDER
        .chars()
        .position(|o| o == folded)
        .map_or((1, u32::from(c)), |at| {
            (0, u32::try_from(at).unwrap_or(u32::MAX))
        })
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
    fn weights_order_punctuation_digits_letters_then_the_rest() {
        // Each class in order, and each class in order within itself: punctuation by the root
        // table, digits 0 to 9, letters a to z without case, then anything else by code point.
        let chain = [
            "$", "0", "1", "5", "9", "a", "B", "m", "Z", "\u{e9}", "\u{ff}", "\u{4e00}",
        ];
        for pair in chain.windows(2) {
            assert_eq!(
                compare(pair[0], pair[1]),
                Ordering::Less,
                "{} < {}",
                pair[0],
                pair[1]
            );
            assert_eq!(
                compare(pair[1], pair[0]),
                Ordering::Greater,
                "{} > {}",
                pair[1],
                pair[0]
            );
        }
        assert!(primary('~') < primary('0') && primary('9') < primary('a'));
        assert_eq!(
            compare("1b", "5a"),
            Ordering::Less,
            "digits weigh before the next character"
        );
        assert_eq!(primary('A'), primary('a'));
        assert_eq!(compare("abc", "abc"), Ordering::Equal);
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
