//! `rulebearing snapshot [--version V]`: a small summary of the architecture at a release, for
//! `changelog` and `rules --unused` to read.
//!
//! - Source: [design § The architect's hat](../../../../docs/artifacts/design.md#the-architects-hat-across-repos-and-across-time)
//!   ("`snapshot` committed per release: a small summary JSON (counts, instability per folder or
//!   project, violations) that `changelog` and a trend chart read")
//! - Contract: [Wave 3 plan § 1.5](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//!   (the snapshot's JSON shape)
//! - Plan: [Wave 3, Step 13](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#23-steps-for-sub-wave-3c-presets-lifecycle-fields-snapshot-and-changelog)
//! - Decisions: [ADR-0029](../../../../docs/adr/0029-ratchets-enforced-by-cruise-and-reported-in-the-summary.md)
//!   (the ratchet counts), [ADR-0030](../../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md)
//!   (a report exits 0)
//! - Requirement: [FR-CLI-07](../../../../docs/prd.md#fr-cli-07)
//!
//! The command extracts the paths afresh (the working directory by default), or reads the saved
//! result `--graph FILE` names, and evaluates it with the configuration the flags find, liveness
//! off (a rule that matches nothing is `cruise`'s finding) and the folder metrics on. Unlike the
//! query commands it never falls back to a saved `.graph/cruise.json`: a release record must
//! describe the tree at its commit, and a saved result may come from any commit. `--graph` is for
//! a result the caller knows is of that release. It writes two files under `.graph/snapshots/`:
//!
//! | File | Holds |
//! | --- | --- |
//! | `<version>.json` | the snapshot: `version`, `sha`, `counts` (`modules`, `dependencies`, `violations` by severity), `instability` per folder, `rules` (`fromMatches`, `toMatches`, `violations` per dependency rule, as `rules --json` counts them) and `ratchets` (the edge count per ratchet) |
//! | `<version>.cruise.json` | the cruise result, as `cruise -T json` writes it, from which `changelog` takes the edges |
//!
//! **The version** is `--version` when given; otherwise the git tag at `HEAD` (the latest by
//! [`rb_config::version`] order when there are several, so `v1.3.0` wins over `v1.3.0-rc.1`). With neither, the command exits 3 and
//! says to pass `--version`. A version is used as a file name, so it is one to 128 characters of
//! letters, digits, `.`, `_`, `+` and `-`, not starting with `.` or `-` and not ending in
//! `.cruise`; anything else exits 3. Writing a version again replaces its files.
//!
//! **The sha** is the commit the graph records (`revisionData.SHA1`), else `HEAD` of the
//! repository the command runs in, else `null`.
//!
//! The output is deterministic: every map is sorted by key, a metric prints as JavaScript prints
//! it ([`rb_model::js_number`]), and nothing in the snapshot depends on the clock. The command
//! prints the two paths it wrote and exits 0; a graph that cannot be read or extracted exits 2.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::Args;
use rb_config::Config;
use rb_model::GraphDocument;
use serde::{Deserialize, Serialize};

use crate::cli::{ConfigArgs, CruiseArgs};
use crate::context::Context;
use crate::exit::RunExit;
use crate::pipeline::{self, RunError, RunOptions};
use crate::progress::Progress;
use crate::{Outcome, configure, ratchets};

/// Where snapshots are written and read, relative to the working directory.
pub const SNAPSHOTS: &str = ".graph/snapshots";

/// The suffix of the cruise result written beside a snapshot.
pub const CRUISE_SUFFIX: &str = ".cruise.json";

/// `snapshot`.
#[derive(Debug, Clone, Default, Args)]
pub struct SnapshotArgs {
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
    /// A saved cruise result of this release to summarise, instead of extracting the paths
    #[arg(long, value_name = "FILE")]
    pub graph: Option<String>,
    /// Files, directories and globs to extract (default: the working directory)
    #[arg(value_name = "FILES-OR-DIRECTORIES")]
    pub paths: Vec<String>,
    /// The release this snapshot records (default: the git tag at HEAD)
    #[arg(long = "version", value_name = "VERSION")]
    pub release: Option<String>,
}

/// A metric, printed as JavaScript prints it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Metric(#[serde(serialize_with = "rb_model::js_number::plain")] pub f64);

/// Violations by severity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Severities {
    /// Error-severity violations.
    #[serde(default)]
    pub error: u64,
    /// Warn-severity violations.
    #[serde(default)]
    pub warn: u64,
    /// Info-severity violations.
    #[serde(default)]
    pub info: u64,
}

/// `counts`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    /// Modules cruised.
    #[serde(default)]
    pub modules: u64,
    /// Dependencies cruised.
    #[serde(default)]
    pub dependencies: u64,
    /// Violations by severity.
    #[serde(default)]
    pub violations: Severities,
}

/// One rule's statistics, as `rules --json` counts them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleCounts {
    /// Modules the selecting side matched.
    #[serde(default)]
    pub from_matches: u64,
    /// Dependencies (or modules) `to` matched.
    #[serde(default)]
    pub to_matches: u64,
    /// Violations of the rule.
    #[serde(default)]
    pub violations: u64,
}

impl RuleCounts {
    /// Whether the rule matched nothing on either side.
    pub fn unused(&self) -> bool {
        self.from_matches == 0 && self.to_matches == 0
    }
}

/// A snapshot, the shape of the [Wave 3 plan § 1.5](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// The release.
    pub version: String,
    /// The commit, when known.
    #[serde(default)]
    pub sha: Option<String>,
    /// Module, dependency and violation counts.
    #[serde(default)]
    pub counts: Counts,
    /// Instability per folder.
    #[serde(default)]
    pub instability: BTreeMap<String, Metric>,
    /// Statistics per dependency rule.
    #[serde(default)]
    pub rules: BTreeMap<String, RuleCounts>,
    /// The edge count per ratchet.
    #[serde(default)]
    pub ratchets: BTreeMap<String, u64>,
}

impl Snapshot {
    /// Summarises an evaluated cruise: `document` is the result as `cruise -T json` reports it,
    /// with `summary.ratchets` filled, and `stats` the evaluation's per-rule statistics.
    pub fn of(
        version: &str,
        sha: Option<String>,
        document: &GraphDocument,
        stats: &[rb_rules::RuleStats],
    ) -> Self {
        let mut rules: BTreeMap<String, RuleCounts> = BTreeMap::new();
        for s in stats {
            // Two rules may share a name; the name is what a snapshot is read by, so they add up.
            let entry = rules.entry(s.name.clone()).or_default();
            entry.from_matches += s.from_matches as u64;
            entry.to_matches += s.to_matches as u64;
            entry.violations += s.violations as u64;
        }
        let summary = &document.summary;
        Self {
            version: version.to_owned(),
            sha,
            counts: Counts {
                modules: document.modules.len() as u64,
                dependencies: document
                    .modules
                    .iter()
                    .map(|m| m.dependencies.len() as u64)
                    .sum(),
                violations: Severities {
                    error: summary.error,
                    warn: summary.warn,
                    info: summary.info,
                },
            },
            instability: document
                .folders
                .iter()
                .flatten()
                .filter_map(|f| f.instability.map(|i| (f.name.clone(), Metric(i))))
                .collect(),
            rules,
            ratchets: summary
                .ratchets
                .iter()
                .flatten()
                .map(|r| (r.name.clone(), r.count))
                .collect(),
        }
    }

    /// The snapshot as it is written: pretty JSON with a trailing newline.
    pub fn to_text(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_default();
        text.push('\n');
        text
    }
}

/// Why `version` cannot name a snapshot file, or `None` when it can.
pub fn invalid_version(version: &str) -> Option<String> {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-');
    let reason = if version.is_empty() || version.len() > 128 {
        "must be 1 to 128 characters"
    } else if !version.chars().all(allowed) {
        "may hold only letters, digits, `.`, `_`, `+` and `-`"
    } else if version.starts_with(['.', '-']) {
        "may not start with `.` or `-`"
    } else if version.ends_with(".cruise") {
        "may not end in `.cruise`, which names the cruise result beside a snapshot"
    } else {
        return None;
    };
    Some(format!(
        "the version `{version}` {reason}, since it names the snapshot's file; pass --version with a release such as 1.3.0"
    ))
}

/// The snapshot file of `version` in `directory`.
pub fn snapshot_path(directory: &Path, version: &str) -> PathBuf {
    directory.join(format!("{version}.json"))
}

/// The cruise result written beside the snapshot of `version`.
pub fn cruise_path(directory: &Path, version: &str) -> PathBuf {
    directory.join(format!("{version}{CRUISE_SUFFIX}"))
}

/// Reads one snapshot file.
///
/// # Errors
/// A message naming the file when it cannot be read or is not a snapshot.
pub fn read(path: &Path) -> Result<Snapshot, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read the snapshot {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| {
        format!(
            "{} is not a snapshot: {e}; write one with `rulebearing snapshot --version <release>`",
            path.display()
        )
    })
}

/// Checks that the snapshot read from `path` records the version its file is named for, and a
/// version [`invalid_version`] accepts: the version names the cruise result beside it, so a
/// snapshot that records another version (a file renamed by hand) would pair with another
/// release's edges, and one that records a path (`../../x`) would reach outside the folder.
///
/// # Errors
/// A message naming the file, what it records and the fix.
pub fn check_named(path: &Path, snapshot: &Snapshot) -> Result<(), String> {
    let stem = path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_suffix(".json"))
        .unwrap_or_default();
    if let Some(reason) = invalid_version(&snapshot.version) {
        return Err(format!(
            "{} records a version that cannot name a snapshot: {reason}",
            path.display()
        ));
    }
    if snapshot.version != stem {
        return Err(format!(
            "{} records the version `{}` but is named for `{stem}`, so it would be paired with the wrong cruise result; write it again with `rulebearing snapshot --version {stem}`, or rename it to {}.json with its {}{CRUISE_SUFFIX}",
            path.display(),
            snapshot.version,
            snapshot.version,
            snapshot.version
        ));
    }
    Ok(())
}

/// Every snapshot in `directory`, oldest first by [`rb_config::version::sort`]; none when the
/// folder does not exist. Each must record the version its file is named for ([`check_named`]).
///
/// # Errors
/// A message naming the file that cannot be read or does not record its own version, or the
/// folder when it cannot be listed.
pub fn read_all(directory: &Path) -> Result<Vec<Snapshot>, String> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("cannot list {}: {e}", directory.display())),
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension().is_some_and(|e| e == "json")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| !n.ends_with(CRUISE_SUFFIX) && !n.starts_with('.'))
        })
        .collect();
    files.sort();
    let mut snapshots = files
        .iter()
        .map(|f| read(f).and_then(|s| check_named(f, &s).map(|()| s)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut order: Vec<String> = snapshots.iter().map(|s| s.version.clone()).collect();
    rb_config::version::sort(&mut order);
    snapshots.sort_by_key(|s| order.iter().position(|v| *v == s.version));
    Ok(snapshots)
}

/// The version to record: `--version`, else the git tag at `HEAD`.
fn version(ctx: &Context<'_>, given: Option<&str>) -> Result<String, String> {
    let version = if let Some(v) = given {
        v.to_owned()
    } else {
        let tags =
            crate::cmd::diff::git(&ctx.cwd, &["tag", "--points-at", "HEAD"]).unwrap_or_default();
        let mut tags: Vec<&str> = tags
            .lines()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect();
        rb_config::version::sort(&mut tags);
        tags.last().map(|t| (*t).to_owned()).ok_or_else(|| {
            "no --version was given and HEAD carries no git tag; pass --version with the release this snapshot records".to_owned()
        })?
    };
    invalid_version(&version).map_or(Ok(version), Err)
}

/// The commit: the graph's `revisionData.SHA1`, else `HEAD`, else none.
fn sha(ctx: &Context<'_>, document: &GraphDocument) -> Option<String> {
    document
        .revision_data
        .as_ref()
        .map(|r| r.sha1.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| crate::cmd::diff::resolve_revision(&ctx.cwd, "HEAD").ok())
}

fn failed(code: RunExit, message: &str) -> Outcome {
    Outcome::failed(code, format!("rulebearing snapshot: {message}\n"))
}

/// The exit code for a cruise that failed.
fn run_failed(error: &RunError) -> Outcome {
    let code = error.exit();
    failed(code, &error.to_string())
}

/// The configuration, with the defaults `cruise` lays over it and the folder metrics on, and
/// whether a file was found.
fn configuration(ctx: &mut Context<'_>, args: &ConfigArgs) -> Result<(Config, bool), Outcome> {
    let loaded = configure::load(ctx, args).map_err(|e| run_failed(&RunError::Config(e)))?;
    let has_config = loaded.is_some();
    let mut config = loaded.unwrap_or_default();
    let flags = CruiseArgs {
        config: args.clone(),
        ..CruiseArgs::default()
    };
    configure::apply_flags(&mut config, &flags, ctx)
        .map_err(|e| run_failed(&RunError::Config(e)))?;
    config.options.metrics = Some(true);
    Ok((config, has_config))
}

/// Cruises as `snapshot` does: `graph` when given, else a fresh extraction of `paths` (never the
/// saved `.graph/cruise.json`); the evaluated result with its ratchets, and the per-rule
/// statistics.
///
/// # Errors
/// An [`Outcome`] to return when the graph cannot be read, extracted or evaluated.
pub fn cruise(
    ctx: &Context<'_>,
    config: &Config,
    has_config: bool,
    (graph, paths): (Option<&str>, &[String]),
) -> Result<(GraphDocument, Vec<rb_rules::RuleStats>), Outcome> {
    let mut document = match graph {
        Some(file) => {
            pipeline::load_graph(ctx, file).map_err(|m| failed(RunExit::Untrustworthy, &m))?
        }
        None => {
            pipeline::extract(ctx, config, paths).map_err(|e| run_failed(&RunError::Extract(e)))?
        }
    };
    pipeline::reset(&mut document);
    let options = RunOptions {
        liveness: false,
        options_used: configure::options_used(has_config.then_some(config), ctx, "json", "-"),
        paths: paths.to_vec(),
        affected: None,
    };
    let run =
        pipeline::evaluate_document(ctx, config, document, &options, &mut Progress::new(None))
            .map_err(|e| run_failed(&e))?;
    let counted = ratchets::evaluate(ctx, config, &run.evaluation.document, false);
    let mut document = run.document;
    if !counted.results.is_empty() {
        document.summary.ratchets = Some(counted.results);
    }
    Ok((document, run.evaluation.rule_stats))
}

/// Writes `text` to `path` under a temporary name renamed into place.
fn write(path: &Path, text: &str) -> Result<(), String> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, text)
        .map_err(|e| format!("cannot write {}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, path).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Runs `snapshot`.
pub fn run(ctx: &mut Context<'_>, args: &SnapshotArgs) -> Outcome {
    let release = match version(ctx, args.release.as_deref()) {
        Ok(v) => v,
        Err(message) => return failed(RunExit::InvalidConfig, &message),
    };
    let (config, has_config) = match configuration(ctx, &args.config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let (document, stats) = match cruise(
        ctx,
        &config,
        has_config,
        (args.graph.as_deref(), &args.paths),
    ) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let snapshot = Snapshot::of(&release, sha(ctx, &document), &document, &stats);
    let value = match serde_json::to_value(&document) {
        Ok(v) => v,
        Err(e) => return failed(RunExit::Untrustworthy, &e.to_string()),
    };
    let cruised = match rb_report::render("json", &value, &rb_report::ReportOptions::default()) {
        Ok(r) => r.output,
        Err(e) => return failed(RunExit::Untrustworthy, &e.to_string()),
    };
    let directory = ctx.resolve(SNAPSHOTS);
    if let Err(e) = std::fs::create_dir_all(&directory) {
        return failed(
            RunExit::Untrustworthy,
            &format!("cannot create {}: {e}", directory.display()),
        );
    }
    let (summary, result) = (
        snapshot_path(&directory, &release),
        cruise_path(&directory, &release),
    );
    for (path, text) in [(&summary, snapshot.to_text()), (&result, cruised)] {
        if let Err(message) = write(path, &text) {
            return failed(RunExit::Untrustworthy, &message);
        }
    }
    Outcome::printed(format!(
        "rulebearing snapshot: wrote {SNAPSHOTS}/{release}.json and {SNAPSHOTS}/{release}{CRUISE_SUFFIX}\n"
    ))
}

#[cfg(test)]
mod tests {
    use rb_model::{Folder, Module, RatchetResult, RatchetStatus, RevisionData, Summary};

    use super::*;

    fn stats(name: &str, from: usize, to: usize, violations: usize) -> rb_rules::RuleStats {
        rb_rules::RuleStats {
            name: name.into(),
            family: rb_config::Family::Forbidden,
            from_matches: from,
            to_matches: to,
            violations,
        }
    }

    fn document() -> Result<GraphDocument, serde_json::Error> {
        let modules = serde_json::from_value::<Vec<Module>>(serde_json::json!([
            {
                "source": "src/a.ts",
                "dependencies": [
                    { "module": "./b", "resolved": "src/b.ts", "coreModule": false, "followable": true, "couldNotResolve": false, "dependencyTypes": ["local"], "dynamic": false, "exoticallyRequired": false, "circular": false, "valid": true, "moduleSystem": "es6" }
                ],
                "valid": true
            },
            { "source": "src/b.ts", "dependencies": [], "valid": true }
        ]))?;
        let folders = Some(vec![
            Folder {
                name: "src/b".into(),
                instability: Some(1.0),
                ..Folder::default()
            },
            Folder {
                name: "src/a".into(),
                instability: Some(0.5),
                ..Folder::default()
            },
            Folder {
                name: "src".into(),
                instability: None,
                ..Folder::default()
            },
        ]);
        let summary = Summary {
            error: 2,
            warn: 1,
            ratchets: Some(vec![RatchetResult {
                name: "via-service".into(),
                budget: "b.json".into(),
                count: 4,
                ceiling: Some(5),
                status: RatchetStatus::Held,
            }]),
            ..Summary::default()
        };
        Ok(GraphDocument {
            modules,
            folders,
            summary,
            revision_data: Some(RevisionData {
                sha1: "abc".into(),
                ..RevisionData::default()
            }),
            ..GraphDocument::default()
        })
    }

    #[test]
    fn a_snapshot_summarises_the_cruise_in_the_frozen_shape() -> Result<(), serde_json::Error> {
        let snapshot = Snapshot::of(
            "1.3.0",
            Some("abc".into()),
            &document()?,
            &[
                stats("r", 2, 0, 1),
                stats("r", 1, 3, 0),
                stats("s", 0, 0, 0),
            ],
        );
        let expected = r#"{
  "version": "1.3.0",
  "sha": "abc",
  "counts": {
    "modules": 2,
    "dependencies": 1,
    "violations": {
      "error": 2,
      "warn": 1,
      "info": 0
    }
  },
  "instability": {
    "src/a": 0.5,
    "src/b": 1
  },
  "rules": {
    "r": {
      "fromMatches": 3,
      "toMatches": 3,
      "violations": 1
    },
    "s": {
      "fromMatches": 0,
      "toMatches": 0,
      "violations": 0
    }
  },
  "ratchets": {
    "via-service": 4
  }
}
"#;
        assert_eq!(snapshot.to_text(), expected);
        assert_eq!(
            snapshot.to_text(),
            snapshot.clone().to_text(),
            "deterministic"
        );
        let back: Snapshot = serde_json::from_str(expected).unwrap_or_default();
        assert_eq!(back, snapshot);
        assert!(back.rules["s"].unused());
        assert!(!back.rules["r"].unused());
        Ok(())
    }

    #[test]
    fn an_unknown_sha_is_null_and_missing_keys_read_as_empty() {
        let text = Snapshot::of("1", None, &GraphDocument::default(), &[]).to_text();
        assert!(text.contains("\"sha\": null"), "{text}");
        let minimal: Snapshot = serde_json::from_str(r#"{"version":"2"}"#).unwrap_or_default();
        assert_eq!(minimal.version, "2");
        assert!(minimal.rules.is_empty() && minimal.sha.is_none());
    }

    #[test]
    fn versions_that_cannot_name_a_file_are_refused() {
        for good in ["1.3.0", "v1.3.0-rc.1+build.5", "2026.09", "r_42"] {
            assert_eq!(invalid_version(good), None, "{good}");
        }
        for (bad, why) in [
            ("", "1 to 128"),
            (&"1".repeat(129), "1 to 128"),
            ("release/1.0", "may hold only"),
            ("1 0", "may hold only"),
            ("..", "may not start"),
            ("-1", "may not start"),
            (".hidden", "may not start"),
            ("1.0.cruise", "`.cruise`"),
        ] {
            let message = invalid_version(bad).unwrap_or_default();
            assert!(message.contains(why), "{bad}: {message}");
        }
        assert_eq!(
            invalid_version(&"1".repeat(128)),
            None,
            "128 characters are allowed"
        );
    }

    #[test]
    fn snapshots_are_read_in_version_order() -> Result<(), String> {
        let dir = std::env::temp_dir().join(format!("rb-snapshot-order-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(read_all(&dir)?, Vec::new(), "no folder, no snapshots");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for version in ["1.10.0", "1.9.0", "1.10.0-rc.1"] {
            let snapshot = Snapshot {
                version: version.into(),
                ..Snapshot::default()
            };
            std::fs::write(snapshot_path(&dir, version), snapshot.to_text())
                .map_err(|e| e.to_string())?;
        }
        std::fs::write(cruise_path(&dir, "1.9.0"), "{}").map_err(|e| e.to_string())?;
        std::fs::write(dir.join("notes.txt"), "x").map_err(|e| e.to_string())?;
        let versions: Vec<String> = read_all(&dir)?.into_iter().map(|s| s.version).collect();
        assert_eq!(versions, ["1.9.0", "1.10.0-rc.1", "1.10.0"]);
        for (file, recorded, needle) in [
            (
                "1.12.0.json",
                "1.13.0",
                "records the version `1.13.0` but is named for `1.12.0`",
            ),
            (
                "x.json",
                "../../x",
                "records a version that cannot name a snapshot",
            ),
        ] {
            let bad = Snapshot {
                version: recorded.into(),
                ..Snapshot::default()
            };
            std::fs::write(dir.join(file), bad.to_text()).map_err(|e| e.to_string())?;
            let error = read_all(&dir).err().unwrap_or_default();
            assert!(error.contains(needle), "{file}: {error}");
            std::fs::remove_file(dir.join(file)).map_err(|e| e.to_string())?;
        }
        std::fs::write(dir.join("broken.json"), "[").map_err(|e| e.to_string())?;
        let error = read_all(&dir).err().unwrap_or_default();
        assert!(error.contains("broken.json is not a snapshot"), "{error}");
        let missing = read(&dir.join("none.json")).err().unwrap_or_default();
        assert!(missing.contains("cannot read the snapshot"), "{missing}");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
