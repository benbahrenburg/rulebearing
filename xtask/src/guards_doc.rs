//! The `guards-doc` check: every YAML block in `docs/guards.md` is a byte copy of its fixture.
//!
//! - Plan: [plan 0005, Step 5](../../docs/plans/pending/0005-guard-catalogue.md#step-5-the-guards-doc-check-in-xtask-5b)
//! - Contract: [plan 0005 § 1.5](../../docs/plans/pending/0005-guard-catalogue.md#15-interfaces-and-contracts-frozen-by-this-plan)
//! - Decisions: [ADR-0023](../../docs/adr/0023-documentation-link-and-lint-gates.md),
//!   [ADR-0024](../../docs/adr/0024-test-quality-gates.md)
//! - Requirement: [NFR-DOC-01](../../docs/prd.md#nfr-doc-01)
//!
//! A fence with the info string `yaml guard=<slug>` is a copy of
//! `cookbook/guards/<slug>/rulebearing.yaml`. One with `yaml guard=<slug>#<rule>` is a copy of that
//! one rule, taken from the file line by line and never through a YAML round trip, so the bytes
//! compared are the author's.
//!
//! The check cannot pass by having nothing to compare. A missing page is an error, a page with no
//! `guard=` block is a finding, a YAML block without `guard=` is a finding, and so is a fixture no
//! block copies. It runs in `cargo xtask lint` and in the `docs-links` job, not in `cargo build`,
//! because a stale example is a documentation defect and not a compile-time one.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

/// The page the check reads, relative to the repository root.
pub const PAGE: &str = "docs/guards.md";

/// The folder holding one directory per fixture, relative to the repository root.
pub const FIXTURES: &str = "cookbook/guards";

/// The file in a fixture directory that a block copies.
pub const RECIPE: &str = "rulebearing.yaml";

/// One thing wrong with the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The 1-based line of the page.
    pub line: usize,
    /// The block's `guard=` value, or the fixture's slug; empty when there is neither.
    pub guard: String,
    /// What is wrong.
    pub message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{PAGE}:{}: guard={}: {}",
            self.line, self.guard, self.message
        )
    }
}

/// What the check found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The `guard=` blocks compared.
    pub blocks: usize,
    /// The slugs those blocks name.
    pub slugs: BTreeSet<String>,
    /// Everything wrong with the page.
    pub findings: Vec<Finding>,
}

impl Report {
    /// True when the page has blocks and nothing is wrong with them.
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

/// A fenced block of the page.
struct Fence<'a> {
    /// The 1-based line of the opening fence.
    line: usize,
    /// The info string.
    info: &'a str,
    /// The lines between the fences, with their line endings.
    body: String,
    /// False when the page ended before the closing fence.
    closed: bool,
}

/// A line's fence character, its length and what follows it, when the line can be a fence.
fn fence_marker(line: &str) -> Option<(char, usize, &str)> {
    let text = line.trim_end_matches(['\r', '\n']);
    let unindented = text.trim_start_matches(' ');
    if text.len() - unindented.len() > 3 {
        return None;
    }
    let mark = unindented
        .chars()
        .next()
        .filter(|c| matches!(c, '`' | '~'))?;
    let run = unindented.chars().take_while(|c| *c == mark).count();
    if run < 3 {
        return None;
    }
    let rest = unindented[run..].trim();
    if mark == '`' && rest.contains('`') {
        return None;
    }
    Some((mark, run, rest))
}

/// Every fenced block of `page`, in order.
fn fences(page: &str) -> Vec<Fence<'_>> {
    let mut out = Vec::new();
    let mut open: Option<(char, usize, Fence<'_>)> = None;
    for (at, line) in page.split_inclusive('\n').enumerate() {
        match (open.take(), fence_marker(line)) {
            (None, Some((mark, run, info))) => {
                let fence = Fence {
                    line: at + 1,
                    info,
                    body: String::new(),
                    closed: false,
                };
                open = Some((mark, run, fence));
            }
            (None, None) => {}
            (Some((mark, run, mut fence)), Some((close, length, "")))
                if close == mark && length >= run =>
            {
                fence.closed = true;
                out.push(fence);
            }
            (Some((mark, run, mut fence)), _) => {
                fence.body.push_str(line);
                open = Some((mark, run, fence));
            }
        }
    }
    if let Some((_, _, fence)) = open {
        out.push(fence);
    }
    out
}

/// What a `guard=` value names.
#[derive(Debug, PartialEq, Eq)]
struct Target<'a> {
    slug: &'a str,
    rule: Option<&'a str>,
}

/// True for a kebab-case slug, which also keeps a value from naming a path outside the fixtures.
fn is_slug(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('-')
        && !text.ends_with('-')
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// What an info string asks for: nothing when the block is not YAML and has no `guard=`, the
/// value and its target when it is well formed, else the value and why it is not.
fn parse_info(info: &str) -> Option<(&str, Result<Target<'_>, String>)> {
    let words: Vec<&str> = info.split_whitespace().collect();
    let guard = words.iter().find_map(|w| w.strip_prefix("guard="));
    let is_yaml = words.first() == Some(&"yaml");
    let Some(value) = guard else {
        return is_yaml.then(|| {
            let why = "a YAML block without guard=<slug> is not checked against any fixture";
            ("", Err(why.to_owned()))
        });
    };
    if !is_yaml || words.len() != 2 {
        let why = format!("the info string `{info}` is not `yaml guard=<slug>[#<rule>]`");
        return Some((value, Err(why)));
    }
    let (slug, rule) = match value.split_once('#') {
        Some((slug, rule)) => (slug, Some(rule)),
        None => (value, None),
    };
    if !is_slug(slug) {
        return Some((value, Err(format!("`{slug}` is not a kebab-case slug"))));
    }
    if rule == Some("") {
        return Some((value, Err("the rule name after `#` is empty".to_owned())));
    }
    Some((value, Ok(Target { slug, rule })))
}

/// The count of leading spaces.
fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// The rule name a list item's first line gives, when it is `- name: <rule>`.
fn item_name(line: &str) -> Option<&str> {
    let value = line.trim_start_matches(' ').strip_prefix("- name:")?.trim();
    Some(value.trim_matches(|c| c == '"' || c == '\''))
}

/// The one rule named `name` in `recipe`, from the list item that starts it to the line before
/// the next item or the next key that is not inside it, without trailing blank lines.
pub fn extract_rule(recipe: &str, name: &str) -> Option<String> {
    let lines: Vec<&str> = recipe.split_inclusive('\n').collect();
    let start = lines.iter().position(|l| item_name(l) == Some(name))?;
    let depth = indent(lines[start]);
    let inside = lines[start + 1..]
        .iter()
        .take_while(|l| l.trim().is_empty() || indent(l) > depth)
        .count();
    let item = &lines[start..=start + inside];
    let last = item.iter().rposition(|l| !l.trim().is_empty()).unwrap_or(0);
    Some(item[..=last].concat())
}

/// The first line where `block` and `expected` differ, with its 1-based number in the block.
/// `None` when they are the same text.
fn first_difference(block: &str, expected: &str) -> Option<(usize, String)> {
    let show = |line: &str| format!("{:?}", line.trim_end_matches('\n'));
    let mut left = block.split_inclusive('\n');
    let mut right = expected.split_inclusive('\n');
    let mut number = 0;
    loop {
        number += 1;
        let why = match (left.next(), right.next()) {
            (None, None) => return None,
            (Some(a), Some(b)) if a == b => continue,
            (Some(a), Some(b)) => format!(
                "differs from the fixture: the block has {}, the fixture has {}",
                show(a),
                show(b)
            ),
            (Some(a), None) => format!("the block has {} after the fixture ends", show(a)),
            (None, Some(b)) => format!("the block ends where the fixture has {}", show(b)),
        };
        return Some((number, why));
    }
}

/// What a target's block must equal, or why there is no such thing.
fn expected_text(target: &Target<'_>, recipe: Option<String>) -> Result<String, String> {
    let recipe =
        recipe.ok_or_else(|| format!("no fixture at {FIXTURES}/{}/{RECIPE}", target.slug))?;
    match target.rule {
        None => Ok(recipe),
        Some(rule) => extract_rule(&recipe, rule).ok_or_else(|| {
            format!(
                "{FIXTURES}/{}/{RECIPE} has no rule named `{rule}`",
                target.slug
            )
        }),
    }
}

/// Checks `page` against the fixtures `read` returns by slug.
pub fn check_page(page: &str, read: impl Fn(&str) -> Option<String>) -> Report {
    let mut report = Report::default();
    for fence in fences(page) {
        let Some((value, parsed)) = parse_info(fence.info) else {
            continue;
        };
        let finding = |line: usize, message: String| Finding {
            line,
            guard: value.to_owned(),
            message,
        };
        let target = match parsed {
            Ok(target) => target,
            Err(why) => {
                report.findings.push(finding(fence.line, why));
                continue;
            }
        };
        report.blocks += 1;
        report.slugs.insert(target.slug.to_owned());
        if !fence.closed {
            let why = "the block is never closed".to_owned();
            report.findings.push(finding(fence.line, why));
            continue;
        }
        match expected_text(&target, read(target.slug)) {
            Err(why) => report.findings.push(finding(fence.line, why)),
            Ok(expected) => {
                if let Some((number, why)) = first_difference(&fence.body, &expected) {
                    report.findings.push(finding(fence.line + number, why));
                }
            }
        }
    }
    if report.blocks == 0 {
        report.findings.push(Finding {
            line: 1,
            guard: String::new(),
            message: "the page has no guard= block, so there is nothing to compare".to_owned(),
        });
    }
    report
}

/// The slugs of the fixtures under `root`: each directory of [`FIXTURES`] holding a [`RECIPE`].
fn fixture_slugs(root: &Path) -> io::Result<BTreeSet<String>> {
    let mut slugs = BTreeSet::new();
    for entry in fs::read_dir(root.join(FIXTURES))? {
        let entry = entry?;
        if entry.path().join(RECIPE).is_file() {
            slugs.insert(entry.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(slugs)
}

/// Checks `docs/guards.md` under `root` against its fixtures, and that each fixture has a block.
///
/// # Errors
/// The page or the fixtures folder cannot be read.
pub fn check(root: &Path) -> io::Result<Report> {
    let page = fs::read_to_string(root.join(PAGE))?;
    let fixtures = root.join(FIXTURES);
    let mut report = check_page(&page, |slug| {
        fs::read_to_string(fixtures.join(slug).join(RECIPE)).ok()
    });
    for slug in fixture_slugs(root)? {
        if !report.slugs.contains(&slug) {
            report.findings.push(Finding {
                line: 1,
                message: format!("no block copies {FIXTURES}/{slug}/{RECIPE}"),
                guard: slug,
            });
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECIPE_TEXT: &str = "rules:\n  dependencies:\n    forbidden:\n      - name: first\n        from: {}\n\n      - name: \"second\"\n        from: {}\n    allowed:\n      - from: {}\n";

    fn fixture(slug: &str) -> Option<String> {
        (slug == "sample").then(|| RECIPE_TEXT.to_owned())
    }

    fn page(info: &str, body: &str) -> String {
        format!("# Guards\n\n```{info}\n{body}```\n")
    }

    fn messages(report: &Report) -> Vec<String> {
        report.findings.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_block_equal_to_its_fixture_passes() {
        let report = check_page(&page("yaml guard=sample", RECIPE_TEXT), fixture);
        assert_eq!(report.findings, []);
        assert!(report.is_clean());
        assert_eq!(report.blocks, 1);
        assert!(report.slugs.contains("sample"));
    }

    #[test]
    fn a_block_with_one_changed_character_names_the_line_of_the_page() {
        let drifted = RECIPE_TEXT.replace("first", "firzt");
        let report = check_page(&page("yaml guard=sample", &drifted), fixture);
        assert!(!report.is_clean());
        // The fence is on line 3, so the fourth line of the block is line 7.
        assert_eq!(
            messages(&report),
            [
                "docs/guards.md:7: guard=sample: differs from the fixture: the block has \"      - name: firzt\", the fixture has \"      - name: first\""
            ]
        );
    }

    #[test]
    fn a_block_shorter_or_longer_than_its_fixture_differs() {
        let short = check_page(&page("yaml guard=sample", "rules:\n"), fixture);
        assert_eq!(
            messages(&short),
            [
                "docs/guards.md:5: guard=sample: the block ends where the fixture has \"  dependencies:\""
            ]
        );
        let long = format!("{RECIPE_TEXT}extra\n");
        let report = check_page(&page("yaml guard=sample", &long), fixture);
        assert_eq!(
            messages(&report),
            ["docs/guards.md:14: guard=sample: the block has \"extra\" after the fixture ends"]
        );
    }

    #[test]
    fn a_rule_block_is_that_rule_alone() {
        assert_eq!(
            extract_rule(RECIPE_TEXT, "first").as_deref(),
            Some("      - name: first\n        from: {}\n")
        );
        assert_eq!(
            extract_rule(RECIPE_TEXT, "second").as_deref(),
            Some("      - name: \"second\"\n        from: {}\n")
        );
        assert_eq!(extract_rule(RECIPE_TEXT, "third"), None);
        let body = "      - name: first\n        from: {}\n";
        let report = check_page(&page("yaml guard=sample#first", body), fixture);
        assert_eq!(report.findings, []);
        assert_eq!(report.blocks, 1);
    }

    #[test]
    fn a_rule_at_the_end_of_a_file_keeps_its_last_line() {
        assert_eq!(
            extract_rule("- name: a\n  from: {}", "a").as_deref(),
            Some("- name: a\n  from: {}")
        );
        assert_eq!(
            extract_rule("- name: a\n  from: {}\n\n", "a").as_deref(),
            Some("- name: a\n  from: {}\n")
        );
    }

    #[test]
    fn an_item_name_needs_the_list_form() {
        assert_eq!(item_name("  - name: a"), Some("a"));
        assert_eq!(item_name("  - name: 'a'"), Some("a"));
        assert_eq!(item_name("  name: a"), None);
    }

    #[test]
    fn an_unknown_slug_or_rule_is_named() {
        let unknown = check_page(&page("yaml guard=other", "x\n"), fixture);
        assert_eq!(
            messages(&unknown),
            ["docs/guards.md:3: guard=other: no fixture at cookbook/guards/other/rulebearing.yaml"]
        );
        let rule = check_page(&page("yaml guard=sample#third", "x\n"), fixture);
        assert_eq!(
            messages(&rule),
            [
                "docs/guards.md:3: guard=sample#third: cookbook/guards/sample/rulebearing.yaml has no rule named `third`"
            ]
        );
    }

    #[test]
    fn a_page_with_nothing_to_compare_is_a_finding() {
        for text in ["# Guards\n", "```sh\nls\n```\n", ""] {
            let report = check_page(text, fixture);
            assert_eq!(report.blocks, 0);
            assert_eq!(
                messages(&report),
                [
                    "docs/guards.md:1: guard=: the page has no guard= block, so there is nothing to compare"
                ],
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_yaml_block_without_a_guard_is_a_finding_and_another_language_is_not() {
        let plain = format!(
            "{}\n```json\n{{}}\n```\n",
            page("yaml guard=sample", RECIPE_TEXT)
        );
        assert_eq!(check_page(&plain, fixture).findings, []);
        let report = check_page(&page("yaml", "a: 1\n"), fixture);
        assert!(messages(&report)[0].contains("without guard=<slug>"));
    }

    /// The info string vocabulary: each malformed form, and why.
    #[test]
    fn a_malformed_info_string_is_named() {
        for (info, why) in [
            ("json guard=sample", "is not `yaml guard=<slug>[#<rule>]`"),
            ("guard=sample", "is not `yaml guard=<slug>[#<rule>]`"),
            (
                "yaml guard=sample extra",
                "is not `yaml guard=<slug>[#<rule>]`",
            ),
            ("yaml guard=", "`` is not a kebab-case slug"),
            ("yaml guard=../sample", "is not a kebab-case slug"),
            ("yaml guard=-sample", "is not a kebab-case slug"),
            ("yaml guard=sample-", "is not a kebab-case slug"),
            ("yaml guard=Sample", "is not a kebab-case slug"),
            ("yaml guard=sample#", "the rule name after `#` is empty"),
        ] {
            let report = check_page(&page(info, RECIPE_TEXT), fixture);
            let found = messages(&report);
            assert!(found[0].contains(why), "{info}: {found:?}");
            assert_eq!(report.slugs.len(), 0, "{info}");
        }
    }

    #[test]
    fn slugs_may_hold_digits_and_inner_hyphens() {
        assert!(is_slug("a-1"));
        assert!(is_slug("7"));
        assert!(!is_slug("a_b"));
    }

    #[test]
    fn an_unclosed_block_is_a_finding() {
        let report = check_page("```yaml guard=sample\nrules:\n", fixture);
        assert_eq!(
            messages(&report),
            ["docs/guards.md:1: guard=sample: the block is never closed"]
        );
    }

    #[test]
    fn fences_follow_commonmark() {
        // A longer fence holds a shorter one; a closing fence has no info string.
        let found = fences("````yaml guard=sample\n```\n``` x\n````\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].body, "```\n``` x\n");
        assert!(found[0].closed);
        // A shorter fence does not close a longer one, and a line of the other mark does not
        // close at all.
        assert!(!fences("````\n```\n")[0].closed);
        let tilde = fences("~~~yaml\n```\n~~~\n");
        assert_eq!(tilde[0].body, "```\n");
        assert_eq!(tilde[0].info, "yaml");
        // Three spaces may indent a fence; four make it code.
        assert!(fence_marker("   ```\n").is_some());
        assert!(fence_marker("    ```\n").is_none());
        assert!(fence_marker("``\n").is_none());
        assert!(fence_marker("text\n").is_none());
        // A code span on its own line is not a fence.
        assert!(fence_marker("```yaml guard=x``` \n").is_none());
        assert_eq!(fence_marker("~~~ a`b\r\n"), Some(('~', 3, "a`b")));
    }

    #[test]
    fn a_fixture_with_no_trailing_newline_differs_from_a_block() {
        let report = check_page(&page("yaml guard=bare", "a: 1\n"), |_| {
            Some("a: 1".to_owned())
        });
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].line, 4);
    }

    proptest::proptest! {
        /// Any block equal to its fixture passes.
        #[test]
        fn any_block_equal_to_its_fixture_passes(
            lines in proptest::collection::vec("[a-z :#-]{0,20}", 0..12),
        ) {
            let recipe: String = lines.iter().map(|l| l.clone() + "\n").collect();
            let copy = recipe.clone();
            let report = check_page(&page("yaml guard=sample", &recipe), move |_| Some(copy.clone()));
            proptest::prop_assert_eq!(report.findings, vec![]);
        }

        /// Any one-character change to a block is found, on the line it was made.
        #[test]
        fn any_one_character_change_is_found(
            lines in proptest::collection::vec("[a-z]{1,20}", 1..12),
            at in 0usize..12,
        ) {
            let at = at % lines.len();
            let recipe: String = lines.iter().map(|l| l.clone() + "\n").collect();
            let drifted: String = lines
                .iter()
                .enumerate()
                .map(|(i, l)| if i == at { l.clone() + "X\n" } else { l.clone() + "\n" })
                .collect();
            let copy = recipe.clone();
            let report = check_page(&page("yaml guard=sample", &drifted), move |_| Some(copy.clone()));
            proptest::prop_assert_eq!(report.findings.len(), 1);
            proptest::prop_assert_eq!(report.findings[0].line, at + 4);
        }
    }
}
