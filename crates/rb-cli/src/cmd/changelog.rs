//! `rulebearing changelog --since <version> [--to <version>]`: the architecture between two
//! releases in words, for release notes.
//!
//! - Source: [design § The developer relations hat](../../../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo)
//!   ("the architecture diff between two revisions in words: new edges across boundaries, retired
//!   rules, ratchets that fell")
//! - Contract: [Wave 3 plan § 1.5](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//! - Plan: [Wave 3, Step 13](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#23-steps-for-sub-wave-3c-presets-lifecycle-fields-snapshot-and-changelog)
//! - Decisions: [ADR-0030](../../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md)
//!   (a report exits 0)
//! - Requirement: [FR-CLI-07](../../../../docs/prd.md#fr-cli-07)
//!
//! The command reads two snapshots under `.graph/snapshots/` ([`crate::cmd::snapshot`]): the one
//! `--since` names and the one `--to` names, by default the newest there is. It does not cruise:
//! to compare a release with the working tree, write a snapshot of the working tree first. It
//! prints four sections:
//!
//! | Section | What it holds |
//! | --- | --- |
//! | Counts | modules, dependencies and violations by severity, at each release, and the change |
//! | New edges across boundaries | the edges `diff` finds added between the two cruise results ([`rb_report::diff`]) whose two ends fall in different layers of a `layers` entry, or in different slices of a slice rule that slices paths (a `matching` with `/`) |
//! | Retired rules | rules the older snapshot records and the newer does not, and rules of the configuration whose `deprecated` release is after `--since` and not after `--to` ([`rb_config::version`] order), with their `replacedBy` |
//! | Ratchets that fell | ratchets both snapshots record whose edge count is lower in the newer one |
//!
//! The edges need both cruise results `snapshot` writes beside the snapshots
//! (`<version>.cruise.json`); when either is missing the section says which, and the JSON has
//! `newEdgesAcrossBoundaries: null`. A slice rule that slices namespaces or dotted module names is
//! not applied, since an edge of the cruise result joins two files. The boundaries and the
//! lifecycle fields come from the configuration the flags find in the working tree; without one
//! there are no boundaries and only removed rules are retired.
//!
//! `--output-type markdown` (the default) or `json`. The output is byte-stable: every list is
//! sorted, and nothing depends on the clock. A missing snapshot or an unreadable cruise result
//! exits 2 naming the file; an invalid configuration or output type exits 3.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use clap::Args;
use rb_config::Config;
use rb_config::pattern;
use rb_model::GraphDocument;
use serde::Serialize;

use crate::cli::ConfigArgs;
use crate::cmd::snapshot::{self, Counts, Snapshot};
use crate::context::Context;
use crate::exit::RunExit;
use crate::{Outcome, configure, write_output};

/// The output types `changelog` renders.
pub const OUTPUT_TYPES: &[&str] = &["markdown", "json"];

/// `changelog`.
#[derive(Debug, Clone, Default, Args)]
pub struct ChangelogArgs {
    /// The older release: a snapshot under .graph/snapshots
    #[arg(long, value_name = "VERSION")]
    pub since: String,
    /// The newer release (default: the newest snapshot)
    #[arg(long, value_name = "VERSION")]
    pub to: Option<String>,
    /// Configuration: the layers, slices and lifecycle fields
    #[command(flatten)]
    pub config: ConfigArgs,
    /// Output type: markdown or json
    #[arg(short = 'T', long, value_name = "TYPE", default_value = "markdown")]
    pub output_type: String,
    /// File to write output to; - for stdout
    #[arg(short = 'f', long, value_name = "FILE", default_value = "-")]
    pub output_to: String,
}

/// One release: its version, commit and counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Release {
    /// The version.
    pub version: String,
    /// The commit, when known.
    pub sha: Option<String>,
    /// The counts.
    pub counts: Counts,
}

impl Release {
    fn of(snapshot: &Snapshot) -> Self {
        Self {
            version: snapshot.version.clone(),
            sha: snapshot.sha.clone(),
            counts: snapshot.counts,
        }
    }
}

/// A boundary an edge crosses.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Boundary {
    /// `layers` or `slices`.
    pub kind: &'static str,
    /// The `layers` entry or the slice rule.
    pub rule: String,
    /// The layer (1-based, highest first) or slice of the edge's source.
    pub from: String,
    /// The layer or slice of its target.
    pub to: String,
}

/// A new edge that crosses at least one boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoundaryEdge {
    /// The source module.
    pub from: String,
    /// The target module.
    pub to: String,
    /// Every boundary it crosses, in configuration order.
    pub boundaries: Vec<Boundary>,
}

/// A retired rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Retired {
    /// The rule.
    pub name: String,
    /// The older snapshot records it and the newer does not.
    pub removed: bool,
    /// The release it was deprecated in, when that is in the range.
    pub deprecated: Option<String>,
    /// The rule that replaces it.
    pub replaced_by: Option<String>,
}

/// A ratchet whose count fell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fell {
    /// The ratchet.
    pub name: String,
    /// The count at the older release.
    pub before: u64,
    /// The count at the newer release.
    pub after: u64,
}

/// What changed between two releases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changelog {
    /// The older release.
    pub since: Release,
    /// The newer release.
    pub to: Release,
    /// The new edges across boundaries; `None` when a cruise result is missing.
    pub new_edges_across_boundaries: Option<Vec<BoundaryEdge>>,
    /// The cruise results that are missing, when any is.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_cruise_results: Vec<String>,
    /// The retired rules, by name.
    pub retired_rules: Vec<Retired>,
    /// The ratchets that fell, by name.
    pub ratchets_fell: Vec<Fell>,
}

/// The index of the first of `patterns` that matches `path`.
fn layer(patterns: &[String], path: &str) -> Option<usize> {
    patterns
        .iter()
        .position(|p| pattern::matcher(p).is_ok_and(|m| m.is_match(path)))
}

/// Every boundary of `config` the edge `from -> to` crosses: the `layers` entries in order, then
/// the slice rules that slice paths.
pub fn boundaries(config: &Config, from: &str, to: &str) -> Vec<Boundary> {
    let mut out = Vec::new();
    for entry in &config.rules.layers {
        if let (Some(a), Some(b)) = (layer(&entry.layers, from), layer(&entry.layers, to))
            && a != b
        {
            out.push(Boundary {
                kind: "layers",
                rule: entry.name.clone(),
                from: (a + 1).to_string(),
                to: (b + 1).to_string(),
            });
        }
    }
    for rule in config
        .rules
        .slices
        .iter()
        .filter(|r| rb_rules::slices::by_path(r))
    {
        let slice = |path: &str| rb_rules::slices::slice_name(rule, path).ok().flatten();
        if let (Some(a), Some(b)) = (slice(from), slice(to))
            && a != b
        {
            out.push(Boundary {
                kind: "slices",
                rule: rule.name.clone(),
                from: a,
                to: b,
            });
        }
    }
    out
}

/// The name, `deprecated` and `replacedBy` of every rule of every family that has `deprecated`.
fn deprecations(config: &Config) -> Vec<(&str, &str, Option<&str>)> {
    let rules = &config.rules;
    let dependency = rules.all_dependency_rules().filter_map(|(_, r)| {
        r.meta
            .deprecated
            .as_deref()
            .map(|d| (r.name(), d, r.meta.replaced_by.as_deref()))
    });
    let element = rules
        .elements
        .iter()
        .map(|r| (r.name.as_str(), &r.lifecycle))
        .chain(rules.slices.iter().map(|r| (r.name.as_str(), &r.lifecycle)))
        .chain(
            rules
                .diagrams
                .iter()
                .map(|r| (r.name.as_str(), &r.lifecycle)),
        )
        .filter_map(|(name, l)| {
            l.deprecated
                .as_deref()
                .map(|d| (name, d, l.replaced_by.as_deref()))
        });
    dependency.chain(element).collect()
}

/// The rules retired between `since` and `to`, by name.
pub fn retired(config: &Config, since: &Snapshot, to: &Snapshot) -> Vec<Retired> {
    use std::cmp::Ordering;
    let mut out: BTreeMap<&str, Retired> = BTreeMap::new();
    for name in since.rules.keys().filter(|n| !to.rules.contains_key(*n)) {
        out.insert(
            name,
            Retired {
                name: name.clone(),
                removed: true,
                deprecated: None,
                replaced_by: None,
            },
        );
    }
    for (name, deprecated, replaced_by) in deprecations(config) {
        let in_range = rb_config::version::compare(deprecated, &since.version) == Ordering::Greater
            && rb_config::version::compare(deprecated, &to.version) != Ordering::Greater;
        if !in_range {
            continue;
        }
        let entry = out.entry(name).or_insert_with(|| Retired {
            name: name.to_owned(),
            removed: false,
            deprecated: None,
            replaced_by: None,
        });
        entry
            .deprecated
            .get_or_insert_with(|| deprecated.to_owned());
        if entry.replaced_by.is_none() {
            entry.replaced_by = replaced_by.map(str::to_owned);
        }
    }
    out.into_values().collect()
}

/// The ratchets both snapshots record whose count fell, by name.
pub fn fell(since: &Snapshot, to: &Snapshot) -> Vec<Fell> {
    since
        .ratchets
        .iter()
        .filter_map(|(name, before)| {
            to.ratchets
                .get(name)
                .filter(|after| *after < before)
                .map(|after| Fell {
                    name: name.clone(),
                    before: *before,
                    after: *after,
                })
        })
        .collect()
}

/// The changelog from `since` to `to`, with the two cruise results when both are available.
pub fn compute(
    config: &Config,
    since: &Snapshot,
    to: &Snapshot,
    cruises: Result<(GraphDocument, GraphDocument), Vec<String>>,
) -> Changelog {
    let (edges, missing) = match cruises {
        Ok((old, new)) => {
            let added = rb_report::diff::compute(&old, &new).added_edges;
            let mut crossing: Vec<BoundaryEdge> = added
                .into_iter()
                .filter_map(|e| {
                    let boundaries = boundaries(config, &e.from, &e.to);
                    (!boundaries.is_empty()).then_some(BoundaryEdge {
                        from: e.from,
                        to: e.to,
                        boundaries,
                    })
                })
                .collect();
            // `diff` lists an edge once per line; the changelog is about the edge.
            crossing.dedup_by(|a, b| a.from == b.from && a.to == b.to);
            (Some(crossing), Vec::new())
        }
        Err(missing) => (None, missing),
    };
    Changelog {
        since: Release::of(since),
        to: Release::of(to),
        new_edges_across_boundaries: edges,
        missing_cruise_results: missing,
        retired_rules: retired(config, since, to),
        ratchets_fell: fell(since, to),
    }
}

/// A table cell: pipes escaped so they do not end the cell.
fn cell(text: &str) -> String {
    text.replace('|', "\\|")
}

fn change(before: u64, after: u64) -> String {
    match after.cmp(&before) {
        std::cmp::Ordering::Greater => format!("+{}", after - before),
        std::cmp::Ordering::Less => format!("-{}", before - after),
        std::cmp::Ordering::Equal => "0".to_owned(),
    }
}

fn boundary_text(b: &Boundary) -> String {
    match b.kind {
        "layers" => format!("`{}` layer {} to layer {}", cell(&b.rule), b.from, b.to),
        _ => format!(
            "`{}` slice `{}` to slice `{}`",
            cell(&b.rule),
            cell(&b.from),
            cell(&b.to)
        ),
    }
}

/// The "New edges across boundaries" section.
fn edges_section(log: &Changelog, out: &mut String) {
    out.push_str("\n## New edges across boundaries\n\n");
    match &log.new_edges_across_boundaries {
        None => {
            let _ = writeln!(
                out,
                "Not available: the edges need the cruise results `rulebearing snapshot` writes beside both snapshots, and {} missing.",
                log.missing_cruise_results
                    .iter()
                    .map(|m| format!("`{m}`"))
                    .collect::<Vec<_>>()
                    .join(" and ")
                    + if log.missing_cruise_results.len() == 1 {
                        " is"
                    } else {
                        " are"
                    }
            );
        }
        Some(edges) if edges.is_empty() => out.push_str("None.\n"),
        Some(edges) => {
            out.push_str("| From | To | Boundary |\n| --- | --- | --- |\n");
            for e in edges {
                let crossed: Vec<String> = e.boundaries.iter().map(boundary_text).collect();
                let _ = writeln!(
                    out,
                    "| `{}` | `{}` | {} |",
                    cell(&e.from),
                    cell(&e.to),
                    crossed.join("; ")
                );
            }
        }
    }
}

/// The `markdown` rendering.
pub fn markdown(log: &Changelog) -> String {
    let (a, b) = (&log.since, &log.to);
    let mut out = format!(
        "# Architecture changelog: {} to {}\n\n",
        a.version, b.version
    );
    if let (Some(x), Some(y)) = (&a.sha, &b.sha) {
        let _ = writeln!(out, "From commit `{x}` to commit `{y}`.\n");
    }
    out.push_str("## Counts\n\n");
    let _ = writeln!(
        out,
        "| Count | {} | {} | Change |\n| --- | ---: | ---: | ---: |",
        cell(&a.version),
        cell(&b.version)
    );
    for (label, x, y) in [
        ("Modules", a.counts.modules, b.counts.modules),
        ("Dependencies", a.counts.dependencies, b.counts.dependencies),
        (
            "Error violations",
            a.counts.violations.error,
            b.counts.violations.error,
        ),
        (
            "Warn violations",
            a.counts.violations.warn,
            b.counts.violations.warn,
        ),
        (
            "Info violations",
            a.counts.violations.info,
            b.counts.violations.info,
        ),
    ] {
        let _ = writeln!(out, "| {label} | {x} | {y} | {} |", change(x, y));
    }
    edges_section(log, &mut out);
    out.push_str("\n## Retired rules\n\n");
    if log.retired_rules.is_empty() {
        out.push_str("None.\n");
    }
    for r in &log.retired_rules {
        let mut parts = Vec::new();
        if r.removed {
            parts.push("removed".to_owned());
        }
        if let Some(d) = &r.deprecated {
            parts.push(format!("deprecated in {d}"));
        }
        if let Some(next) = &r.replaced_by {
            parts.push(format!("replaced by `{next}`"));
        }
        let _ = writeln!(out, "- `{}`: {}", r.name, parts.join(", "));
    }
    out.push_str("\n## Ratchets that fell\n\n");
    if log.ratchets_fell.is_empty() {
        out.push_str("None.\n");
    } else {
        let _ = writeln!(
            out,
            "| Ratchet | {} | {} |\n| --- | ---: | ---: |",
            cell(&a.version),
            cell(&b.version)
        );
        for f in &log.ratchets_fell {
            let _ = writeln!(out, "| `{}` | {} | {} |", cell(&f.name), f.before, f.after);
        }
    }
    out
}

/// The `json` rendering: pretty, with a trailing newline.
pub fn json(log: &Changelog) -> String {
    let mut text = serde_json::to_string_pretty(log).unwrap_or_default();
    text.push('\n');
    text
}

fn failed(code: RunExit, message: &str) -> Outcome {
    Outcome::failed(code, format!("rulebearing changelog: {message}\n"))
}

/// The snapshot of `version` among `all`.
fn find<'a>(all: &'a [Snapshot], version: &str, directory: &str) -> Result<&'a Snapshot, String> {
    all.iter().find(|s| s.version == version).ok_or_else(|| {
        let known: Vec<&str> = all.iter().map(|s| s.version.as_str()).collect();
        format!(
            "no snapshot of `{version}` under {directory} (it has {}); write it with `rulebearing snapshot --version {version}` at that release",
            if known.is_empty() {
                "none".to_owned()
            } else {
                known.join(", ")
            }
        )
    })
}

/// The two cruise results, or the missing files; an unreadable one is an error.
fn cruises(
    directory: &Path,
    since: &str,
    to: &str,
) -> Result<Result<(GraphDocument, GraphDocument), Vec<String>>, String> {
    let paths = [
        snapshot::cruise_path(directory, since),
        snapshot::cruise_path(directory, to),
    ];
    let missing: Vec<String> = paths
        .iter()
        .filter(|p| !p.is_file())
        .map(|p| {
            format!(
                "{}/{}",
                snapshot::SNAPSHOTS,
                p.file_name().and_then(|n| n.to_str()).unwrap_or_default()
            )
        })
        .collect();
    if !missing.is_empty() {
        return Ok(Err(missing));
    }
    let read = |path: &Path| -> Result<GraphDocument, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        rb_ingest::dependency_cruiser::read(&text).map_err(|e| {
            format!(
                "{} is not a cruise result: {e}; write it again with `rulebearing snapshot`",
                path.display()
            )
        })
    };
    Ok(Ok((read(&paths[0])?, read(&paths[1])?)))
}

/// Runs `changelog`.
pub fn run(ctx: &mut Context<'_>, args: &ChangelogArgs) -> Outcome {
    if !OUTPUT_TYPES.contains(&args.output_type.as_str()) {
        return failed(
            RunExit::InvalidConfig,
            &format!(
                "`{}` is not an output type of changelog; use markdown or json",
                args.output_type
            ),
        );
    }
    let config = match configure::load(ctx, &args.config) {
        Ok(c) => c.unwrap_or_default(),
        Err(e) => return failed(RunExit::InvalidConfig, &e.to_string()),
    };
    let directory = ctx.resolve(snapshot::SNAPSHOTS);
    let all = match snapshot::read_all(&directory) {
        Ok(all) => all,
        Err(message) => return failed(RunExit::Untrustworthy, &message),
    };
    let since = match find(&all, &args.since, snapshot::SNAPSHOTS) {
        Ok(s) => s,
        Err(message) => return failed(RunExit::Untrustworthy, &message),
    };
    let to = match &args.to {
        Some(version) => find(&all, version, snapshot::SNAPSHOTS),
        None => all
            .last()
            .ok_or_else(|| format!("no snapshot under {}", snapshot::SNAPSHOTS)),
    };
    let to = match to {
        Ok(s) => s,
        Err(message) => return failed(RunExit::Untrustworthy, &message),
    };
    let documents = match cruises(&directory, &since.version, &to.version) {
        Ok(d) => d,
        Err(message) => return failed(RunExit::Untrustworthy, &message),
    };
    let log = compute(&config, since, to, documents);
    let text = if args.output_type == "json" {
        json(&log)
    } else {
        markdown(&log)
    };
    let mut stdout = String::new();
    if let Err(message) = write_output(ctx, &args.output_to, &text, &mut stdout) {
        return failed(RunExit::Untrustworthy, &message);
    }
    Outcome {
        stdout,
        stderr: String::new(),
        code: RunExit::Violations(0).code(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::cmd::snapshot::{RuleCounts, Severities};

    fn config() -> Result<Config, rb_config::ConfigError> {
        let text = r#"
rules:
  layers:
    - name: app-layers
      layers: ["^src/ui/", "^src/domain/"]
  slices:
    - name: features
      matching: "src/features/(**)//"
      should: notDependOnEachOther
    - name: namespaces
      matching: "App.(*)"
      should: beFreeOfCycles
  dependencies:
    forbidden:
      - name: old-http
        from: {}
        to: {}
        deprecated: "1.1.0"
        replacedBy: gateway-only
      - name: ancient
        from: {}
        to: {}
        deprecated: "0.9.0"
  elements:
    - name: sealed
      select: { kind: class }
      should: { beSealed: true }
      deprecated: "1.1.0"
"#;
        rb_config::load_text(
            text,
            rb_config::read::Syntax::Yaml,
            &std::env::temp_dir(),
            &rb_config::LoadOptions::default(),
        )
    }

    fn snapshot(version: &str, rules: &[&str], ratchets: &[(&str, u64)], modules: u64) -> Snapshot {
        Snapshot {
            version: version.into(),
            sha: Some(format!("sha-{version}")),
            counts: Counts {
                modules,
                dependencies: 4,
                violations: Severities {
                    error: 0,
                    warn: 2,
                    info: 0,
                },
            },
            rules: rules
                .iter()
                .map(|r| ((*r).to_owned(), RuleCounts::default()))
                .collect(),
            ratchets: ratchets
                .iter()
                .map(|(n, c)| ((*n).to_owned(), *c))
                .collect::<BTreeMap<_, _>>(),
            ..Snapshot::default()
        }
    }

    #[test]
    fn boundaries_are_layers_then_path_slices() -> Result<(), rb_config::ConfigError> {
        let config = config()?;
        assert_eq!(
            boundaries(&config, "src/domain/a.ts", "src/ui/b.ts"),
            [Boundary {
                kind: "layers",
                rule: "app-layers".into(),
                from: "2".into(),
                to: "1".into(),
            }]
        );
        assert!(boundaries(&config, "src/ui/a.ts", "src/ui/b.ts").is_empty());
        assert!(
            boundaries(&config, "src/ui/a.ts", "lib/b.ts").is_empty(),
            "outside every layer"
        );
        assert_eq!(
            boundaries(&config, "src/features/a/x.ts", "src/features/b/y.ts"),
            [Boundary {
                kind: "slices",
                rule: "features".into(),
                from: "a".into(),
                to: "b".into(),
            }]
        );
        assert!(boundaries(&config, "src/features/a/x.ts", "src/features/a/y.ts").is_empty());
        assert!(
            boundaries(&config, "App.Orders", "App.Billing").is_empty(),
            "a namespace slicing is not applied to files"
        );
        Ok(())
    }

    #[test]
    fn retired_rules_are_the_removed_and_the_deprecated_in_range()
    -> Result<(), rb_config::ConfigError> {
        let config = config()?;
        let since = snapshot("1.0.0", &["old-http", "gone", "kept"], &[], 1);
        let to = snapshot("1.1.0", &["kept"], &[], 1);
        let retired = retired(&config, &since, &to);
        let names: Vec<(&str, bool, Option<&str>, Option<&str>)> = retired
            .iter()
            .map(|r| {
                (
                    r.name.as_str(),
                    r.removed,
                    r.deprecated.as_deref(),
                    r.replaced_by.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                ("gone", true, None, None),
                ("old-http", true, Some("1.1.0"), Some("gateway-only")),
                ("sealed", false, Some("1.1.0"), None),
            ]
        );
        let later = snapshot("1.2.0", &["kept"], &[], 1);
        let after = super::retired(&config, &to, &later);
        assert!(
            after.is_empty(),
            "deprecated at 1.1.0 is not after 1.1.0: {after:?}"
        );
        Ok(())
    }

    #[test]
    fn only_ratchets_that_fell_are_listed() {
        let since = snapshot(
            "1",
            &[],
            &[("down", 12), ("up", 1), ("same", 3), ("gone", 4)],
            1,
        );
        let to = snapshot(
            "2",
            &[],
            &[("down", 11), ("up", 2), ("same", 3), ("new", 0)],
            1,
        );
        assert_eq!(
            fell(&since, &to),
            [Fell {
                name: "down".into(),
                before: 12,
                after: 11,
            }]
        );
    }

    #[test]
    fn markdown_says_when_the_edges_are_not_available() -> Result<(), rb_config::ConfigError> {
        let config = config()?;
        let since = snapshot("1.0.0", &[], &[], 3);
        let to = snapshot("1.1.0", &[], &[], 1);
        let log = compute(
            &config,
            &since,
            &to,
            Err(vec![".graph/snapshots/1.0.0.cruise.json".into()]),
        );
        let text = markdown(&log);
        assert!(
            text.contains("and `.graph/snapshots/1.0.0.cruise.json` is missing."),
            "{text}"
        );
        assert!(text.contains("| Modules | 3 | 1 | -2 |"), "{text}");
        assert!(text.contains("| Dependencies | 4 | 4 | 0 |"), "{text}");
        assert!(text.contains("From commit `sha-1.0.0` to commit `sha-1.1.0`."));
        let two = compute(&config, &since, &to, Err(vec!["a".into(), "b".into()]));
        assert!(markdown(&two).contains("`a` and `b` are missing."));
        let value: serde_json::Value = serde_json::from_str(&json(&log)).unwrap_or_default();
        assert_eq!(value["newEdgesAcrossBoundaries"], serde_json::Value::Null);
        assert_eq!(
            value["missingCruiseResults"][0],
            ".graph/snapshots/1.0.0.cruise.json"
        );
        assert_eq!(value["since"]["counts"]["modules"], 3);
        Ok(())
    }

    #[test]
    fn an_edge_lists_every_boundary_it_crosses() {
        let (since, to) = (snapshot("1", &[], &[], 1), snapshot("2", &[], &[], 1));
        let mut log = compute(&Config::default(), &since, &to, Err(Vec::new()));
        log.new_edges_across_boundaries = Some(Vec::new());
        assert!(markdown(&log).contains("## New edges across boundaries\n\nNone.\n"));
        log.new_edges_across_boundaries = Some(vec![BoundaryEdge {
            from: "src/ui/a|b.ts".into(),
            to: "src/domain/c.ts".into(),
            boundaries: vec![
                Boundary {
                    kind: "layers",
                    rule: "l".into(),
                    from: "1".into(),
                    to: "2".into(),
                },
                Boundary {
                    kind: "slices",
                    rule: "s".into(),
                    from: "ui".into(),
                    to: "domain".into(),
                },
            ],
        }]);
        assert!(markdown(&log).contains(
            "| `src/ui/a\\|b.ts` | `src/domain/c.ts` | `l` layer 1 to layer 2; `s` slice `ui` to slice `domain` |\n"
        ));
        assert!(markdown(&log).contains("## Retired rules\n\nNone.\n"));
        assert!(markdown(&log).contains("## Ratchets that fell\n\nNone.\n"));
        assert_eq!(markdown(&log), markdown(&log.clone()), "deterministic");
    }

    #[test]
    fn cells_and_changes_render() {
        assert_eq!(cell("a|b"), "a\\|b");
        assert_eq!(change(1, 3), "+2");
        assert_eq!(change(3, 1), "-2");
        assert_eq!(change(2, 2), "0");
    }
}
