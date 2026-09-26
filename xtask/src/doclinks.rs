//! Documentation link checking.
//!
//! Every relative link in a Markdown file, and every relative link in a Rust doc comment, must
//! resolve to a file that exists; when the link carries an anchor, the target must contain a
//! heading with that slug. This is the machine-checked half of the "link everything" rule in
//! [ADR-0001](../../docs/adr/0001-record-architecture-decisions.md) and the gate described in
//! [ADR-0023](../../docs/adr/0023-documentation-link-and-lint-gates.md).
//!
//! What is deliberately not checked:
//!
//! - Absolute URLs (`http`, `https`, `mailto`) and in-page-only fragments of other tools. The
//!   tool makes no network requests ([architecture § Security posture](../../docs/architecture.md#security-posture)).
//! - `file/<tab-id>` targets in `docs/artifacts/design.md`, which are the source document's own
//!   tab references, kept because the export is verbatim ([docs/artifacts/README.md](../../docs/artifacts/README.md)).
//! - Anything inside a fenced code block, which is an example rather than a reference.
//! - A link into a [`LOCAL_ONLY_ROOTS`] directory, on a CI server only (the `CI` environment
//!   variable is set). The plans and the exported design are kept out of the published
//!   repository, so the server does not have them. On a developer's machine those links are
//!   checked like any other, and a missing plans directory is reported as broken links rather
//!   than skipped ([ADR-0039](../../docs/adr/0039-plans-and-design-kept-local.md)).

use crate::{walk, walk_shallow};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Directories scanned for Markdown files, relative to the repository root.
pub const MARKDOWN_ROOTS: &[&str] = &[
    "docs",
    "conformance",
    "testbeds",
    "wrappers",
    "adapters",
    "frontends",
    "presets",
    "schema",
    "fuzz",
    "scripts",
    "eng",
];

/// Directories scanned for Rust doc comments, relative to the repository root.
pub const RUST_ROOTS: &[&str] = &["crates", "xtask", "fuzz"];

/// Directories that are git-ignored and live only on a developer's machine, relative to the
/// repository root. A link into one is checked locally and skipped on a CI server
/// ([ADR-0039](../../docs/adr/0039-plans-and-design-kept-local.md)).
pub const LOCAL_ONLY_ROOTS: &[&str] = &["docs/plans", "docs/artifacts"];

/// Why a link did not resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The path does not exist.
    MissingFile,
    /// The path exists but holds no heading with that anchor.
    MissingAnchor,
    /// A crate's root module doc does not link its architecture section and its plan.
    MissingCrateHeader,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFile => write!(f, "no such file"),
            Self::MissingAnchor => write!(f, "no such heading"),
            Self::MissingCrateHeader => write!(
                f,
                "the crate's //! header must link docs/architecture.md#<section> and docs/plans/"
            ),
        }
    }
}

/// One link that did not resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broken {
    /// File holding the link, relative to the repository root.
    pub file: PathBuf,
    /// 1-based line the link sits on.
    pub line: usize,
    /// The link target exactly as written.
    pub target: String,
    /// Why it failed.
    pub reason: Reason,
}

impl fmt::Display for Broken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {} -> {}",
            self.file.display(),
            self.line,
            self.reason,
            self.target
        )
    }
}

/// The outcome of one run.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// Files scanned.
    pub files: usize,
    /// Relative links checked.
    pub checked: usize,
    /// Links that did not resolve, in scan order.
    pub broken: Vec<Broken>,
}

impl Report {
    /// True when every relative link resolved.
    pub fn is_clean(&self) -> bool {
        self.broken.is_empty()
    }
}

/// One link found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Link {
    line: usize,
    target: String,
}

/// GitHub's heading-anchor slug: lowercase, drop punctuation other than `-` and `_`, spaces to
/// dashes. Duplicate headings are not disambiguated because the check only asks whether an
/// anchor exists.
pub fn slug(heading: &str) -> String {
    let lowered = heading.trim().to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    for ch in lowered.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else if ch == ' ' {
            out.push('-');
        }
    }
    out
}

/// Extracts the anchors a Markdown file offers: one per ATX heading, plus any explicit
/// `<a id="...">`.
fn anchors(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut in_fence = false;
    for line in text.lines() {
        if is_fence(line) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            let title = rest.trim_start_matches('#');
            if title.starts_with(' ') || title.is_empty() {
                found.insert(slug(title));
            }
        }
        let mut haystack = line;
        while let Some(at) = haystack.find("<a id=\"") {
            let after = &haystack[at + 7..];
            if let Some(end) = after.find('"') {
                found.insert(after[..end].to_owned());
                haystack = &after[end..];
            } else {
                break;
            }
        }
    }
    found
}

fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// Pulls the inline Markdown links out of a file's text.
///
/// `doc_comments_only` restricts the scan to `//!` and `///` lines, which is how Rust sources are
/// read so that string literals and code are never mistaken for documentation.
fn links(text: &str, doc_comments_only: bool) -> Vec<Link> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for (index, raw) in text.lines().enumerate() {
        let line = if doc_comments_only {
            let trimmed = raw.trim_start();
            match trimmed
                .strip_prefix("//!")
                .or_else(|| trimmed.strip_prefix("///"))
            {
                Some(body) => body,
                None => continue,
            }
        } else {
            raw
        };
        if is_fence(line) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let bytes = line.as_bytes();
        let mut cursor = 0usize;
        while let Some(found) = line[cursor..].find("](") {
            let open = cursor + found + 2;
            let Some(close_offset) = line[open..].find(')') else {
                break;
            };
            let close = open + close_offset;
            // A backslash-escaped bracket is not a link, and neither is `](` inside inline code.
            let escaped = found > 0 && bytes[cursor + found - 1] == b'\\';
            let target = line[open..close].trim();
            if !escaped && !target.is_empty() && !target.contains(char::is_whitespace) {
                out.push(Link {
                    line: index + 1,
                    target: target.to_owned(),
                });
            }
            cursor = close + 1;
        }
    }
    out
}

/// True for a target the checker deliberately ignores.
fn is_ignored(target: &str) -> bool {
    target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with("file/")
        || target.starts_with('<')
}

/// True when `relative` lies under a [`LOCAL_ONLY_ROOTS`] directory.
fn is_local_only(relative: &Path) -> bool {
    LOCAL_ONLY_ROOTS.iter().any(|dir| relative.starts_with(dir))
}

/// True when the value of the `CI` environment variable says this is a CI server: set, and
/// neither empty nor `false`, as GitHub Actions and most other providers set it.
fn is_ci(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|v| !v.is_empty() && !v.eq_ignore_ascii_case("false") && v != "0")
}

/// Resolves `.` and `..` without touching the filesystem, so a link that escapes the repository
/// is reported rather than silently followed.
///
/// A `..` that would climb past the root is kept as a component: the resulting path does not
/// exist, so the link is reported missing instead of quietly resolving somewhere else.
fn normalise(path: &Path) -> PathBuf {
    let mut out: Vec<Component<'_>> = Vec::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                _ => out.push(part),
            },
            other => out.push(other),
        }
    }
    let mut result = PathBuf::new();
    for component in out {
        result.push(component.as_os_str());
    }
    result
}

/// Every file the checker reads, relative to `root`, in a stable order.
///
/// # Errors
/// Returns the underlying error when a directory cannot be listed.
pub fn scanned_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut top: Vec<PathBuf> = Vec::new();
    walk_shallow(root, &["md"], &mut top)?;
    files.extend(top);
    for dir in MARKDOWN_ROOTS {
        walk(&root.join(dir), &["md"], &mut files)?;
    }
    for dir in RUST_ROOTS {
        walk(&root.join(dir), &["rs"], &mut files)?;
    }
    Ok(files)
}

/// Checks every relative link under `root`.
///
/// # Errors
/// Returns the underlying error when a directory cannot be listed or a file cannot be read.
pub fn check(root: &Path) -> io::Result<Report> {
    check_links(root, is_ci(std::env::var_os("CI").as_deref()))
}

/// Checks every relative link under `root`, skipping links into [`LOCAL_ONLY_ROOTS`] when
/// `skip_local_only` is true. [`check`] sets it on a CI server.
///
/// # Errors
/// Returns the underlying error when a directory cannot be listed or a file cannot be read.
pub fn check_links(root: &Path, skip_local_only: bool) -> io::Result<Report> {
    let files = scanned_files(root)?;
    let mut report = Report {
        files: files.len(),
        ..Report::default()
    };
    let mut anchor_cache: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();

    for file in &files {
        let text = fs::read_to_string(file)?;
        let is_rust = file.extension().and_then(|e| e.to_str()) == Some("rs");
        let base = file.parent().unwrap_or(root).to_path_buf();
        for link in links(&text, is_rust) {
            if is_ignored(&link.target) {
                continue;
            }
            let (path_part, anchor) = match link.target.split_once('#') {
                Some((p, a)) => (p, Some(a)),
                None => (link.target.as_str(), None),
            };
            let target_path = if path_part.is_empty() {
                file.clone()
            } else {
                normalise(&base.join(path_part))
            };
            let relative = target_path
                .strip_prefix(root)
                .unwrap_or(&target_path)
                .to_path_buf();
            if skip_local_only && is_local_only(&relative) {
                continue;
            }
            report.checked += 1;
            if !target_path.exists() {
                report.broken.push(Broken {
                    file: relative_to(root, file),
                    line: link.line,
                    target: link.target.clone(),
                    reason: Reason::MissingFile,
                });
                continue;
            }
            let Some(anchor) = anchor else { continue };
            if anchor.is_empty() || target_path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if !anchor_cache.contains_key(&relative) {
                let target_text = fs::read_to_string(&target_path)?;
                anchor_cache.insert(relative.clone(), anchors(&target_text));
            }
            let known = anchor_cache.get(&relative);
            if !known.is_some_and(|set| set.contains(anchor)) {
                report.broken.push(Broken {
                    file: relative_to(root, file),
                    line: link.line,
                    target: link.target.clone(),
                    reason: Reason::MissingAnchor,
                });
            }
        }
    }
    report.broken.extend(crate_headers(root)?);
    report
        .broken
        .sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    Ok(report)
}

/// Checks that every crate under `crates/` opens its root module (`src/lib.rs`, or `src/main.rs`
/// for a binary-only crate) with a `//!` block that links its architecture section and its plan,
/// the header [CLAUDE.md](../../CLAUDE.md) asks every crate to copy
/// ([NFR-DOC-01](../../docs/prd.md#nfr-doc-01)). Whether those links resolve is the job of
/// [`check`]; this only asserts they are there.
///
/// # Errors
/// Returns the underlying error when `crates/` cannot be listed or a root module cannot be read.
pub fn crate_headers(root: &Path) -> io::Result<Vec<Broken>> {
    let crates = root.join("crates");
    let mut missing = Vec::new();
    if !crates.is_dir() {
        return Ok(missing);
    }
    let mut dirs: Vec<PathBuf> = fs::read_dir(&crates)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join("Cargo.toml").is_file())
        .collect();
    dirs.sort();
    for dir in dirs {
        let lib = dir.join("src").join("lib.rs");
        let module = if lib.is_file() {
            lib
        } else {
            dir.join("src").join("main.rs")
        };
        let text = fs::read_to_string(&module).unwrap_or_default();
        let header: String = text
            .lines()
            .take_while(|line| line.trim_start().starts_with("//!"))
            .collect::<Vec<_>>()
            .join("\n");
        if !(header.contains("architecture.md#") && header.contains("docs/plans/")) {
            missing.push(Broken {
                file: relative_to(root, &module),
                line: 1,
                target: "docs/architecture.md#..., docs/plans/...".to_owned(),
                reason: Reason::MissingCrateHeader,
            });
        }
    }
    Ok(missing)
}

fn relative_to(root: &Path, file: &Path) -> PathBuf {
    file.strip_prefix(root).unwrap_or(file).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rb-doclinks-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, text);
    }

    #[test]
    fn a_crate_without_the_linked_header_is_reported() {
        let root = scratch("headers");
        write(&root.join("crates/good/Cargo.toml"), "");
        write(
            &root.join("crates/good/src/lib.rs"),
            "//! Good.\n//! - [a](../../../docs/architecture.md#x)\n//! - [p](../../../docs/plans/pending/p.md)\n",
        );
        write(&root.join("crates/bin/Cargo.toml"), "");
        write(
            &root.join("crates/bin/src/main.rs"),
            "//! [a](docs/architecture.md#x) [p](docs/plans/x.md)\nfn main() {}\n",
        );
        write(&root.join("crates/noplan/Cargo.toml"), "");
        write(
            &root.join("crates/noplan/src/lib.rs"),
            "//! [a](docs/architecture.md#x)\n",
        );
        write(&root.join("crates/noarch/Cargo.toml"), "");
        write(
            &root.join("crates/noarch/src/lib.rs"),
            "//! [p](docs/plans/x.md)\n",
        );
        // A link below the header does not count: the header is the first `//!` block.
        write(&root.join("crates/late/Cargo.toml"), "");
        write(
            &root.join("crates/late/src/lib.rs"),
            "//! Late.\nuse x;\n//! [a](docs/architecture.md#x) [p](docs/plans/x.md)\n",
        );
        write(&root.join("crates/not-a-crate/README.md"), "");
        let missing = crate_headers(&root).unwrap_or_default();
        let files: Vec<String> = missing
            .iter()
            .map(|b| b.file.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(
            files,
            [
                "crates/late/src/lib.rs",
                "crates/noarch/src/lib.rs",
                "crates/noplan/src/lib.rs"
            ]
        );
        assert!(
            missing
                .iter()
                .all(|b| b.reason == Reason::MissingCrateHeader)
        );
        assert!(missing[0].to_string().contains("header must link"));
        assert!(
            crate_headers(&scratch("empty"))
                .unwrap_or_default()
                .is_empty()
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn every_crate_in_this_repository_has_its_header() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        assert_eq!(crate_headers(&root).ok(), Some(vec![]));
    }

    #[test]
    fn slug_matches_github_rules() {
        assert_eq!(slug("## The rule engine"), "-the-rule-engine");
        assert_eq!(slug("The rule engine"), "the-rule-engine");
        assert_eq!(
            slug("1. Architect section (for the board)"),
            "1-architect-section-for-the-board"
        );
        assert_eq!(slug("`rb-model` and friends"), "rb-model-and-friends");
        assert_eq!(slug("  Trailing space  "), "trailing-space");
        assert_eq!(slug("FR-CORE-01"), "fr-core-01");
    }

    #[test]
    fn anchors_come_from_headings_and_explicit_ids() {
        let text = "# Title\n\n## A section\n\ntext\n\n<a id=\"manual\"></a>\n";
        let found = anchors(text);
        assert!(found.contains("title"));
        assert!(found.contains("a-section"));
        assert!(found.contains("manual"));
    }

    #[test]
    fn headings_inside_a_fence_are_not_anchors() {
        let text = "# Real\n\n```sh\n# not a heading\n```\n";
        let found = anchors(text);
        assert!(found.contains("real"));
        assert!(!found.contains("not-a-heading"));
    }

    #[test]
    fn hash_without_space_is_not_a_heading() {
        assert!(anchors("#nothashheading\n").is_empty());
    }

    #[test]
    fn links_are_found_with_line_numbers() {
        let text = "see [a](b.md) and [c](d.md#e)\n\n[f](g.md)\n";
        let found = links(text, false);
        assert_eq!(found.len(), 3);
        assert_eq!(
            found[0],
            Link {
                line: 1,
                target: "b.md".into()
            }
        );
        assert_eq!(
            found[1],
            Link {
                line: 1,
                target: "d.md#e".into()
            }
        );
        assert_eq!(
            found[2],
            Link {
                line: 3,
                target: "g.md".into()
            }
        );
    }

    #[test]
    fn link_parsing_handles_the_awkward_cases() {
        // Each case pins one branch of the scanner: an escaped bracket is not a link, an empty
        // target is not a link, a target with a space is not a link, and two links on one line
        // are both found with the cursor landing after the first.
        assert!(
            links("text \\](escaped.md) more", false).is_empty(),
            "escaped bracket"
        );
        assert!(links("empty ]() target", false).is_empty(), "empty target");
        assert!(
            links("spaced ](a b.md) target", false).is_empty(),
            "target with a space"
        );
        assert!(
            links("unclosed ](a.md", false).is_empty(),
            "no closing paren"
        );

        let two = links("[a](one.md) then [b](two.md)", false);
        assert_eq!(two.len(), 2);
        assert_eq!(two[0].target, "one.md");
        assert_eq!(two[1].target, "two.md");

        // A link at the very start of a line has nothing before it to inspect for an escape.
        let first = links("](start.md)", false);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].target, "start.md");

        // Surrounding whitespace inside the parentheses is trimmed, not treated as a space.
        let padded = links("[a]( padded.md )", false);
        assert_eq!(padded.len(), 1);
        assert_eq!(padded[0].target, "padded.md");
    }

    #[test]
    fn links_in_a_fence_are_examples_not_references() {
        let text = "```md\n[x](nope.md)\n```\n[y](yes.md)\n";
        let found = links(text, false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].target, "yes.md");
    }

    #[test]
    fn rust_scan_reads_doc_comments_only() {
        let text = "//! see [a](../x.md)\nlet s = \"](not-a-link.md)\";\n/// and [b](../y.md)\n";
        let found = links(text, true);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].target, "../x.md");
        assert_eq!(found[1].target, "../y.md");
    }

    #[test]
    fn ignored_targets_are_skipped() {
        assert!(is_ignored("https://example.com"));
        assert!(is_ignored("http://example.com"));
        assert!(is_ignored("mailto:someone@example.com"));
        assert!(is_ignored("file/1704a18f-ee24"));
        assert!(!is_ignored("../docs/prd.md"));
    }

    #[test]
    fn normalise_resolves_dot_segments() {
        assert_eq!(
            normalise(Path::new("a/b/../c/./d.md")),
            PathBuf::from("a/c/d.md")
        );
    }

    #[test]
    fn normalise_keeps_a_climb_past_the_root_visible() {
        // Clamping this to `etc/passwd` would let an escaping link resolve to a real file.
        assert_eq!(
            normalise(Path::new("../../etc/passwd")),
            PathBuf::from("../../etc/passwd")
        );
        assert_eq!(normalise(Path::new("/a/../../b")), PathBuf::from("/../b"));
    }

    proptest::proptest! {
        /// Whatever the heading, an anchor holds only characters GitHub puts in one.
        #[test]
        fn slug_only_emits_anchor_characters(heading in ".{0,80}") {
            let slug = slug(&heading);
            proptest::prop_assert!(
                slug.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_'),
                "slug of {heading:?} was {slug:?}"
            );
        }

        /// Slugging an anchor again must not change it, or a link written from a heading and a
        /// link written from an anchor would disagree.
        #[test]
        fn slug_is_idempotent(heading in ".{0,80}") {
            let once = slug(&heading);
            proptest::prop_assert_eq!(slug(&once), once.clone());
        }

        /// `.` never survives, and `..` only ever appears before any real segment, so a
        /// resolved path is either inside the repository or visibly outside it.
        #[test]
        fn normalise_leaves_no_interior_dot_segments(
            parts in proptest::collection::vec("[a-z.]{1,4}", 0..8)
        ) {
            let joined: PathBuf = parts.iter().collect();
            let result = normalise(&joined);
            let mut seen_normal = false;
            for component in result.components() {
                match component {
                    Component::CurDir => {
                        proptest::prop_assert!(false, "`.` survived in {:?}", result);
                    }
                    Component::ParentDir => {
                        proptest::prop_assert!(
                            !seen_normal,
                            "`..` follows a real segment in {:?}",
                            result
                        );
                    }
                    _ => seen_normal = true,
                }
            }
        }
    }

    #[test]
    fn check_reports_missing_files_and_anchors() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("rb-doclinks-{}", std::process::id()));
        let docs = root.join("docs");
        fs::create_dir_all(&docs)?;
        fs::write(
            docs.join("a.md"),
            "# Head One\n\n[ok](b.md#head-two)\n[bad anchor](b.md#nope)\n[gone](c.md)\n[web](https://example.com/x.md)\n",
        )?;
        fs::write(docs.join("b.md"), "# Head Two\n")?;
        let report = check(&root)?;
        assert_eq!(report.files, 2);
        assert_eq!(report.checked, 3);
        assert!(!report.is_clean());
        assert_eq!(report.broken.len(), 2);
        assert_eq!(report.broken[0].reason, Reason::MissingAnchor);
        assert_eq!(report.broken[0].line, 4);
        assert_eq!(report.broken[1].reason, Reason::MissingFile);
        assert_eq!(report.broken[1].target, "c.md");
        assert!(report.broken[1].to_string().contains("no such file"));
        // The report addresses files relative to the repository root, not absolutely.
        assert_eq!(report.broken[0].file, PathBuf::from("docs/a.md"));
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn a_local_only_link_is_checked_locally_and_skipped_on_ci() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("rb-doclinks-local-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let docs = root.join("docs");
        fs::create_dir_all(docs.join("plans"))?;
        fs::write(docs.join("plans/p.md"), "# Step two\n")?;
        fs::write(
            docs.join("a.md"),
            "[plan](plans/p.md#step-one)\n[design](artifacts/d.md)\n[other](gone.md)\n",
        )?;
        // Locally every link is checked: a wrong anchor into a plan and a missing artifacts
        // directory are both reported, not skipped.
        let report = check_links(&root, false)?;
        assert_eq!(report.checked, 3);
        let targets: Vec<&str> = report.broken.iter().map(|b| b.target.as_str()).collect();
        assert_eq!(
            targets,
            ["plans/p.md#step-one", "artifacts/d.md", "gone.md"]
        );
        assert_eq!(report.broken[0].reason, Reason::MissingAnchor);
        assert_eq!(report.broken[1].reason, Reason::MissingFile);
        // On CI only the published link is checked.
        let report = check_links(&root, true)?;
        assert_eq!(report.checked, 1);
        assert_eq!(report.broken.len(), 1);
        assert_eq!(report.broken[0].target, "gone.md");
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn local_only_roots_match_by_path_component() {
        assert!(is_local_only(Path::new("docs/plans/pending/0001.md")));
        assert!(is_local_only(Path::new("docs/artifacts/design.md")));
        assert!(!is_local_only(Path::new("docs/plansx/a.md")));
        assert!(!is_local_only(Path::new("docs/prd.md")));
    }

    #[test]
    fn the_ci_variable_is_read_as_providers_set_it() {
        use std::ffi::OsStr;
        assert!(!is_ci(None));
        for off in ["", "false", "FALSE", "0"] {
            assert!(!is_ci(Some(OsStr::new(off))), "{off:?}");
        }
        for on in ["true", "1", "yes"] {
            assert!(is_ci(Some(OsStr::new(on))), "{on:?}");
        }
    }

    #[test]
    fn a_fragment_is_only_checked_against_markdown() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("rb-doclinks-frag-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let docs = root.join("docs");
        fs::create_dir_all(&docs)?;
        fs::write(docs.join("code.rs"), "fn main() {}\n")?;
        fs::write(
            docs.join("a.md"),
            // A fragment into source code names a line, not a heading; an empty fragment names
            // nothing. Neither may be reported as a missing heading.
            "# Head\n\n[src](code.rs#L1)\n[empty](a.md#)\n[real](a.md#head)\n",
        )?;
        let report = check(&root)?;
        assert_eq!(report.checked, 3);
        assert!(report.is_clean(), "unexpected: {:?}", report.broken);
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn check_is_clean_on_this_repository() -> io::Result<()> {
        let root = repo_root();
        let report = check(&root)?;
        assert!(
            report.checked > 100,
            "expected the repository's links, found {}",
            report.checked
        );
        assert!(
            report.is_clean(),
            "broken documentation links:\n{}",
            report
                .broken
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
        Ok(())
    }

    fn repo_root() -> PathBuf {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest
            .parent()
            .map_or(manifest.clone(), Path::to_path_buf)
    }
}
