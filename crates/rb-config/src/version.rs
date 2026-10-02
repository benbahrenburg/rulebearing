//! Release versions as the rule lifecycle fields and the snapshots name them, and their order.
//!
//! - Source: [design § The architect's hat](../../../docs/artifacts/design.md#the-architects-hat-across-repos-and-across-time)
//!   (`since`, `deprecated`, `replacedBy`; `snapshot` committed per release)
//! - Plan: [Wave 3, Steps 12 and 13](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#23-steps-for-sub-wave-3c-presets-lifecycle-fields-snapshot-and-changelog)
//! - Requirement: [FR-CLI-07](../../../docs/prd.md#fr-cli-07)
//!
//! A version is a string and is never refused for its form: a repository may release as
//! `1.2.0`, `v1.2.0`, `2026.09` or `r42`. When a version is semver (`MAJOR.MINOR.PATCH`, an
//! optional leading `v`, an optional `-prerelease` and `+build`), it is compared as semver: a
//! prerelease comes before its release and build metadata is ignored. A list of versions that
//! are all semver is ordered that way; a list with any other version is ordered naturally
//! (runs of digits by their value, everything else character by character), which keeps
//! `1.10` after `1.9` and orders date-like versions by date. Equal-ranking versions are ordered
//! by their text, so an order is always total and the same on every run.

use std::cmp::Ordering;

/// One prerelease identifier: numeric identifiers rank below alphanumeric ones.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Identifier {
    Numeric(u64),
    Text(String),
}

/// A parsed semver version; build metadata is dropped, as semver's precedence ignores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Semver {
    /// `MAJOR`.
    pub major: u64,
    /// `MINOR`.
    pub minor: u64,
    /// `PATCH`.
    pub patch: u64,
    prerelease: Vec<Identifier>,
}

impl Semver {
    /// Whether the version has a prerelease part.
    pub fn is_prerelease(&self) -> bool {
        !self.prerelease.is_empty()
    }
}

impl PartialOrd for Semver {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Semver {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.is_prerelease(), other.is_prerelease()) {
                (false, false) => Ordering::Equal,
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (true, true) => self.prerelease.cmp(&other.prerelease),
            })
    }
}

fn number(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn identifier(text: &str) -> Option<Identifier> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return None;
    }
    Some(number(text).map_or_else(|| Identifier::Text(text.to_owned()), Identifier::Numeric))
}

/// Parses `text` as semver, with an optional leading `v` or `V`; `None` when it is not one.
pub fn parse(text: &str) -> Option<Semver> {
    let text = text
        .strip_prefix('v')
        .or_else(|| text.strip_prefix('V'))
        .unwrap_or(text);
    let text = match text.split_once('+') {
        Some((version, build)) => {
            build
                .split('.')
                .try_for_each(|b| identifier(b).map(|_| ()))?;
            version
        }
        None => text,
    };
    let (core, prerelease) = match text.split_once('-') {
        Some((core, pre)) => (core, pre.split('.').map(identifier).collect::<Option<_>>()?),
        None => (text, Vec::new()),
    };
    let mut parts = core.split('.');
    let (major, minor, patch) = (
        number(parts.next()?)?,
        number(parts.next()?)?,
        number(parts.next()?)?,
    );
    if parts.next().is_some() {
        return None;
    }
    Some(Semver {
        major,
        minor,
        patch,
        prerelease,
    })
}

/// A run of digits or of other characters.
#[derive(Debug, PartialEq, Eq)]
enum Run<'a> {
    Digits(&'a str),
    Text(&'a str),
}

fn runs(text: &str) -> Vec<Run<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    for i in 1..=bytes.len() {
        if i == bytes.len() || bytes[i].is_ascii_digit() != bytes[start].is_ascii_digit() {
            let run = &text[start..i];
            out.push(if bytes[start].is_ascii_digit() {
                Run::Digits(run)
            } else {
                Run::Text(run)
            });
            start = i;
        }
    }
    out
}

/// Two digit runs by value: leading zeros dropped, then the longer is larger.
fn digits(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// The natural order: runs of digits by their value, other runs by their text, a digit run
/// before a text run, and a prefix before what extends it.
pub fn natural(a: &str, b: &str) -> Ordering {
    let (a_runs, b_runs) = (runs(a), runs(b));
    for (x, y) in a_runs.iter().zip(&b_runs) {
        let order = match (x, y) {
            (Run::Digits(x), Run::Digits(y)) => digits(x, y),
            (Run::Text(x), Run::Text(y)) => x.cmp(y),
            (Run::Digits(_), Run::Text(_)) => Ordering::Less,
            (Run::Text(_), Run::Digits(_)) => Ordering::Greater,
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a_runs.len().cmp(&b_runs.len())
}

/// Whether `a` and `b` name the same release, and which is later: semver precedence when both
/// are semver (so `v1.1.0`, `1.1.0` and `1.1.0+build` are equal), else the natural order. Unlike
/// [`compare`] it breaks no tie by the text, so it is the comparison for "is this release in a
/// range"; [`compare`] and [`sort`] are for ordering a list.
pub fn precedence(a: &str, b: &str) -> Ordering {
    match (parse(a), parse(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        _ => natural(a, b),
    }
}

/// Orders `a` and `b` as semver when both are, else naturally; ties by the text.
///
/// Use [`sort`] for a list: this comparison mixes two orders and is not transitive across a
/// list that mixes semver and other versions.
pub fn compare(a: &str, b: &str) -> Ordering {
    match (parse(a), parse(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        _ => natural(a, b),
    }
    .then_with(|| a.cmp(b))
}

/// Sorts `versions` oldest first: by semver when every one is semver, otherwise naturally;
/// ties by the text.
pub fn sort<T: AsRef<str>>(versions: &mut [T]) {
    if versions.iter().all(|v| parse(v.as_ref()).is_some()) {
        versions.sort_by(|a, b| compare(a.as_ref(), b.as_ref()));
    } else {
        versions.sort_by(|a, b| {
            natural(a.as_ref(), b.as_ref()).then_with(|| a.as_ref().cmp(b.as_ref()))
        });
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn semver_is_parsed_with_its_parts() {
        let v = parse("v1.2.3-rc.1+build.5");
        assert_eq!(
            v.as_ref().map(|v| (v.major, v.minor, v.patch)),
            Some((1, 2, 3))
        );
        assert_eq!(v.as_ref().map(Semver::is_prerelease), Some(true));
        assert_eq!(parse("V0.0.0").map(|v| v.is_prerelease()), Some(false));
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "v",
            "1.2.3-",
            "1.2.3-a..b",
            "1.2.3+",
            "1.2.3-a_b",
            "a.b.c",
            "1.2.-3",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn semver_precedence_is_the_specification_s() {
        // semver.org, item 11.
        let ordered = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.1.0",
            "1.10.0",
            "2.0.0",
        ];
        for pair in ordered.windows(2) {
            assert_eq!(compare(pair[0], pair[1]), Ordering::Less, "{pair:?}");
            assert_eq!(compare(pair[1], pair[0]), Ordering::Greater, "{pair:?}");
        }
        assert_eq!(
            compare("1.0.0+a", "1.0.0+b"),
            Ordering::Less,
            "ties by text"
        );
        assert_eq!(parse("1.0.0+a").cmp(&parse("1.0.0+b")), Ordering::Equal);
        assert_eq!(
            compare("v1.0.0", "1.0.0"),
            Ordering::Greater,
            "ties by text"
        );
    }

    #[test]
    fn precedence_ignores_the_spelling_of_a_release() {
        assert_eq!(precedence("v1.1.0", "1.1.0"), Ordering::Equal);
        assert_eq!(precedence("1.1.0+b", "V1.1.0"), Ordering::Equal);
        assert_eq!(precedence("v1.1.0", "1.0.0"), Ordering::Greater);
        assert_eq!(precedence("1.1.0-rc.1", "v1.1.0"), Ordering::Less);
        assert_eq!(precedence("2026.9", "2026.10"), Ordering::Less);
        assert_eq!(precedence("r7", "r007"), Ordering::Equal);
        assert_ne!(
            compare("v1.1.0", "1.1.0"),
            Ordering::Equal,
            "compare still ties by text"
        );
    }

    #[test]
    fn other_versions_are_ordered_naturally() {
        let ordered = ["2026.9", "2026.10", "2026.10a", "r9", "r10", "r010x"];
        for pair in ordered.windows(2) {
            assert_eq!(natural(pair[0], pair[1]), Ordering::Less, "{pair:?}");
        }
        assert_eq!(natural("r007", "r7"), Ordering::Equal);
        assert_eq!(natural("", ""), Ordering::Equal);
        assert_eq!(natural("", "1"), Ordering::Less);
        assert_eq!(natural("a", "1"), Ordering::Greater);
        assert_eq!(compare("1.2", "1.10"), Ordering::Less);
    }

    #[test]
    fn a_list_is_sorted_by_one_order() {
        let mut semver = vec!["1.10.0", "v1.9.0", "1.0.0", "1.10.0-rc.1"];
        sort(&mut semver);
        assert_eq!(semver, ["1.0.0", "v1.9.0", "1.10.0-rc.1", "1.10.0"]);
        // One version is not semver, so the list is ordered naturally: `1.10.0-rc.1` extends
        // `1.10.0` and follows it.
        let mut mixed = vec!["1.10.0", "1.10.0-rc.1", "1.9", "1.0.0"];
        sort(&mut mixed);
        assert_eq!(mixed, ["1.0.0", "1.9", "1.10.0", "1.10.0-rc.1"]);
        let mut empty: Vec<String> = Vec::new();
        sort(&mut empty);
        assert!(empty.is_empty());
    }

    proptest! {
        #[test]
        fn semver_triples_round_trip(major in 0u64..1000, minor in 0u64..1000, patch in 0u64..1000, v in any::<bool>()) {
            let text = format!("{}{major}.{minor}.{patch}", if v { "v" } else { "" });
            let parsed = parse(&text);
            prop_assert_eq!(parsed.map(|p| (p.major, p.minor, p.patch)), Some((major, minor, patch)));
        }

        #[test]
        fn compare_is_a_consistent_order(a in "[v0-9a-z.+-]{0,8}", b in "[v0-9a-z.+-]{0,8}") {
            prop_assert_eq!(compare(&a, &b), compare(&b, &a).reverse());
            prop_assert_eq!(compare(&a, &a), Ordering::Equal);
            prop_assert_eq!(compare(&a, &b) == Ordering::Equal, a == b);
        }

        #[test]
        fn sorting_is_idempotent_and_order_independent(mut list in proptest::collection::vec("[v0-9a-z.-]{0,6}", 0..8)) {
            let mut reversed = list.clone();
            reversed.reverse();
            sort(&mut list);
            sort(&mut reversed);
            prop_assert_eq!(&list, &reversed);
            let again = { let mut l = list.clone(); sort(&mut l); l };
            prop_assert_eq!(list, again);
        }
    }
}
