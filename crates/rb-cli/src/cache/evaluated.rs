//! The evaluated layer of the `--cache` entry: on a full hit, the run as `cruise` reports it, so
//! only the reporter runs.
//!
//! - Architecture: [Performance model](../../../../docs/architecture.md#performance-model)
//! - Plan: [Wave 3, Step 1](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)
//!   ("a warm run is byte-identical to a cold run ... and at least 5x faster"),
//!   [§ 1.7](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#17-quality-attributes)
//! - Coverage: [coverage § Options](../../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options)
//!   (row `cache`: dependency-cruiser serves the whole evaluated result when nothing changed)
//! - Decision: [ADR-0008](../../../../docs/adr/0008-exit-code-contract.md) (a doubtful entry is a
//!   miss)
//! - Requirement: [FR-CLI-05](../../../../docs/prd.md#fr-cli-05)
//!
//! A [`Verdict`] is what `cruise` hands its reporter: the document after evaluation, the report's
//! filters and the ratchets, the expired entries, the vacuous rules, the ratchet results and the
//! extractors' warnings. It is stored beside the extraction as `evaluated-<16 hex>.json` (or
//! `.json.z`), named by `evaluated.json`, which records the key it was computed under and the
//! `sha256:` digest of the stored bytes. The key ([`key`]) is the extraction's digest and
//! everything evaluation reads besides:
//!
//! | Part | Why |
//! | --- | --- |
//! | the build's version and this layer's format | another build may evaluate differently |
//! | the configuration files' hash and the canonical configuration | the rules, `allowEmpty`, `defines` |
//! | the effective options after the flags | `focus`, `reaches`, `highlight`, `collapse`, `exclude`, `includeOnly`, `metrics` (which depends on the reporter), `prefix`, `suffix`, reporter options |
//! | `optionsUsed` as the run records it | the output type and destination, the working directory, the rules file, every flag it records |
//! | the known violations in force | `--ignore-known FILE` contents, or none under `--no-ignore-known` |
//! | the liveness mode | `strict`, `warn` or `off` changes the verdict and the summary |
//! | the positional paths | `summary.optionsUsed.args` |
//! | today's date | `expires` on a rule, a known violation or a baseline entry |
//! | every ratchet budget file's bytes | the ceiling each ratchet is held to |
//! | every diagram rule's `.puml` bytes | what `adhereTo` compares with |
//!
//! Once a verdict has been served, the reporter's output is kept too (`rendered.json`), keyed on
//! the verdict's key, the output type and every report option (the colour, `--strict-schema`,
//! `--max-findings`, the path prefix, the collapse pattern, and the timestamp for the reporters
//! that print it, [`rb_report::STAMPED`]), with the small part of the verdict the exit code and
//! the messages need ([`Tail`]). A run whose rendered output matches reads neither the verdict
//! nor the extraction.
//!
//! `--affected` turns the layer off for the run: its changed files come from version control
//! against a revision, and a change there that the extraction does not see (a commit on the
//! revision, say) would change the closure. The extraction layer still serves.

use std::path::Path;

use rb_config::Config;
use rb_model::{GraphDocument, VacuousRule};
use serde::{Deserialize, Serialize};

use super::key;
use super::manifest::{self, CacheError, Miss, digest};
use crate::context::Context;
use crate::pipeline::{Run, RunOptions};
use crate::ratchets::Ratchets;

/// The file that names the stored verdict and its key.
pub const EVALUATED_FILE: &str = "evaluated.json";

/// The layout of this layer, part of its key.
const FORMAT: &str = "1";

/// An expired rule or known violation, as the report's error line names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExpiredLine {
    /// `rule` or `knownViolation`.
    pub kind: String,
    /// The rule, or the known violation's id or `from -> to`.
    pub name: String,
    /// The last day it applied, `YYYY-MM-DD`.
    pub expires: String,
}

/// What `cruise` reports from: everything after evaluation, the report's filters and the
/// ratchets, before the reporter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Verdict {
    /// The document as the reporter renders it.
    pub document: GraphDocument,
    /// The rules and known violations past their date.
    pub expired: Vec<ExpiredLine>,
    /// The rules whose selecting side matched nothing.
    pub vacuous: Vec<VacuousRule>,
    /// The ratchets' results and their vacuous entries.
    pub ratchets: Ratchets,
    /// The extractors' warnings, as `path: message` lines.
    pub warnings: Vec<String>,
}

impl Verdict {
    /// The verdict of an evaluated run and its ratchets.
    pub fn of(run: Run, ratchets: Ratchets) -> Self {
        Self {
            expired: run
                .evaluation
                .expired
                .iter()
                .map(|e| ExpiredLine {
                    kind: e.kind.clone(),
                    name: e.name.clone(),
                    expires: e.expires.to_string(),
                })
                .collect(),
            vacuous: run.evaluation.vacuous,
            document: run.document,
            ratchets,
            warnings: run.warnings,
        }
    }
}

/// The part of a [`Verdict`] a finished report needs besides the rendered output: the messages
/// and the exit code's inputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tail {
    /// As [`Verdict::expired`].
    pub expired: Vec<ExpiredLine>,
    /// As [`Verdict::vacuous`].
    pub vacuous: Vec<VacuousRule>,
    /// As [`Verdict::ratchets`].
    pub ratchets: Ratchets,
    /// As [`Verdict::warnings`].
    pub warnings: Vec<String>,
    /// The report's error-severity violations, `summary.error`.
    pub error: u64,
}

impl Verdict {
    /// Its [`Tail`].
    pub fn tail(&self) -> Tail {
        Tail {
            expired: self.expired.clone(),
            vacuous: self.vacuous.clone(),
            ratchets: self.ratchets.clone(),
            warnings: self.warnings.clone(),
            error: self.document.summary.error,
        }
    }
}

/// A stored verdict whose key matched, not yet read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    /// The cache folder.
    pub folder: std::path::PathBuf,
    /// The verdict's key.
    pub key: String,
    /// Whether it is compressed.
    pub compressed: bool,
}

/// The file that names the rendered output of the last verdict served, with its key, digest
/// and tail.
pub const RENDERED_FILE: &str = "rendered.json";

/// The rendered output itself, as the reporter wrote it, so it is read without parsing.
pub const RENDERED_OUTPUT: &str = "rendered.out";

/// `rendered.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RenderedEntry {
    key: String,
    digest: String,
    tail: Tail,
}

/// The key of the rendered output of the verdict with `verdict` key, under `report` (the output
/// type and the report options, as `cruise` spells them).
pub fn render_key(verdict: &str, report: &str) -> String {
    digest(format!("{verdict}\n{report}").as_bytes())
}

/// The rendered output and tail stored in `folder` under `key`.
///
/// # Errors
/// A [`Miss`]: none stored, another key, or a file that does not parse or match its digest.
pub fn load_rendered(folder: &Path, key: &str) -> Result<(String, Tail), Miss> {
    let path = folder.join(RENDERED_FILE);
    let text = match std::fs::read(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Miss::Absent),
        Err(e) => return Err(Miss::Corrupt(format!("{}: {e}", path.display()))),
    };
    let entry: RenderedEntry = serde_json::from_slice(&text)
        .map_err(|e| Miss::Corrupt(format!("{}: {e}", path.display())))?;
    if entry.key != key {
        return Err(Miss::Stale("report key"));
    }
    let bytes = manifest::read_payload(folder, RENDERED_OUTPUT, &entry.digest, false)?;
    let output =
        String::from_utf8(bytes).map_err(|e| Miss::Corrupt(format!("{RENDERED_OUTPUT}: {e}")))?;
    Ok((output, entry.tail))
}

/// Stores a rendered output and its tail under `key` in `folder`, replacing the one before: the
/// output first, then `rendered.json` naming its digest, so a reader never takes one run's
/// output for another's.
///
/// # Errors
/// [`CacheError`] when a file cannot be written.
pub fn store_rendered(
    folder: &Path,
    key: &str,
    output: &str,
    tail: Tail,
) -> Result<(), CacheError> {
    let write_error = |name: &str, e: &std::io::Error| CacheError::Write {
        path: folder.join(name),
        reason: e.to_string(),
    };
    let entry = RenderedEntry {
        key: key.to_owned(),
        digest: digest(output.as_bytes()),
        tail,
    };
    manifest::replace(folder, RENDERED_OUTPUT, output.as_bytes())
        .map_err(|e| write_error(RENDERED_OUTPUT, &e))?;
    let bytes = serde_json::to_vec(&entry).map_err(|e| CacheError::Serialise(e.to_string()))?;
    manifest::replace(folder, RENDERED_FILE, &bytes).map_err(|e| write_error(RENDERED_FILE, &e))
}

/// Whether `evaluated.json` in `folder` names a verdict under `key`: the cheap check before a
/// run commits to the stored verdict.
pub fn stored_under(folder: &Path, key: &str) -> bool {
    std::fs::read(folder.join(EVALUATED_FILE))
        .ok()
        .and_then(|text| serde_json::from_slice::<Pointer>(&text).ok())
        .is_some_and(|pointer| pointer.key == key)
}

/// `evaluated.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pointer {
    key: String,
    digest: String,
}

/// The part of [`key`] that does not depend on the extraction, or `None` when the layer is off
/// for this run (`--affected`).
pub fn partial_key(
    ctx: &Context<'_>,
    config: &Config,
    options: &RunOptions,
    liveness: &str,
) -> Option<String> {
    fn json<T: Serialize + ?Sized>(value: &T) -> Vec<u8> {
        serde_json::to_vec(value).unwrap_or_default()
    }
    if options.affected.is_some() {
        return None;
    }
    let mut parts: Vec<(String, Vec<u8>)> = vec![
        ("format".into(), FORMAT.as_bytes().to_vec()),
        ("version".into(), key::VERSION.as_bytes().to_vec()),
        (
            "files".into(),
            key::config_hash(config, &ctx.cwd).into_bytes(),
        ),
        ("canonical".into(), json(&config.canonical)),
        ("options".into(), json(&config.options)),
        ("optionsUsed".into(), json(&options.options_used)),
        ("known".into(), json(&config.known_violations)),
        ("liveness".into(), liveness.as_bytes().to_vec()),
        ("paths".into(), json(&options.paths)),
        ("today".into(), ctx.today.to_string().into_bytes()),
    ];
    for ratchet in &config.rules.ratchets {
        let bytes = std::fs::read(ctx.resolve(&ratchet.budget)).unwrap_or_default();
        parts.push((format!("budget:{}", ratchet.budget), bytes));
    }
    // A diagram path is relative to the configuration's folder, as the evaluator reads it.
    let base = config
        .files
        .first()
        .and_then(|f| f.parent())
        .unwrap_or_else(|| Path::new("."));
    for diagram in &config.rules.diagrams {
        let bytes = std::fs::read(base.join(&diagram.adhere_to)).unwrap_or_default();
        parts.push((format!("diagram:{}", diagram.adhere_to), bytes));
    }
    Some(crate::cmd::attest::hash_files(
        parts.iter().map(|(n, b)| (n.clone(), b.as_slice())),
    ))
}

/// The key of the verdict of the extraction with digest `extraction`.
pub fn key(partial: &str, extraction: &str) -> String {
    digest(format!("{partial}\n{extraction}").as_bytes())
}

/// The file a verdict with `digest` is stored under.
fn verdict_file(digest: &str, compressed: bool) -> String {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    let short: String = hex.chars().take(16).collect();
    format!(
        "evaluated-{short}.json{}",
        if compressed { ".z" } else { "" }
    )
}

/// The verdict stored in `folder` under `key`.
///
/// # Errors
/// A [`Miss`]: none stored, another key, or a file that is damaged or does not parse.
pub fn load(folder: &Path, key: &str, compressed: bool) -> Result<Verdict, Miss> {
    let path = folder.join(EVALUATED_FILE);
    let text = match std::fs::read(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Miss::Absent),
        Err(e) => return Err(Miss::Corrupt(format!("{}: {e}", path.display()))),
    };
    let pointer: Pointer = serde_json::from_slice(&text)
        .map_err(|e| Miss::Corrupt(format!("{}: {e}", path.display())))?;
    if pointer.key != key {
        return Err(Miss::Stale("evaluation key"));
    }
    let name = verdict_file(&pointer.digest, compressed);
    let bytes = manifest::read_payload(folder, &name, &pointer.digest, compressed)?;
    serde_json::from_slice(&bytes).map_err(|e| Miss::Corrupt(format!("{name}: {e}")))
}

/// Stores `verdict` under `key` in `folder`: the verdict under its content-derived name, then
/// `evaluated.json` renamed into place, then the verdict files it no longer names removed.
///
/// # Errors
/// [`CacheError`] when a file cannot be written.
pub fn store(
    folder: &Path,
    key: &str,
    verdict: &Verdict,
    compressed: bool,
) -> Result<(), CacheError> {
    let write_error = |path: &Path, e: &std::io::Error| CacheError::Write {
        path: path.to_path_buf(),
        reason: e.to_string(),
    };
    std::fs::create_dir_all(folder).map_err(|e| write_error(folder, &e))?;
    let bytes = serde_json::to_vec(verdict).map_err(|e| CacheError::Serialise(e.to_string()))?;
    let payload = if compressed {
        miniz_oxide::deflate::compress_to_vec_zlib(&bytes, 1)
    } else {
        bytes
    };
    let pointer = Pointer {
        key: key.to_owned(),
        digest: digest(&payload),
    };
    let name = verdict_file(&pointer.digest, compressed);
    manifest::replace(folder, &name, &payload).map_err(|e| write_error(&folder.join(&name), &e))?;
    let mut text =
        serde_json::to_vec_pretty(&pointer).map_err(|e| CacheError::Serialise(e.to_string()))?;
    text.push(b'\n');
    manifest::replace(folder, EVALUATED_FILE, &text)
        .map_err(|e| write_error(&folder.join(EVALUATED_FILE), &e))?;
    if let Ok(listing) = std::fs::read_dir(folder) {
        for entry in listing.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            let ours = file.starts_with("evaluated-")
                && (Path::new(&file)
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("json"))
                    || file.to_ascii_lowercase().ends_with(".json.z"));
            if ours && file != name {
                // Best effort: another run may be reading or removing it.
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rb-cache-evaluated-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn verdict() -> Verdict {
        Verdict {
            document: GraphDocument::default(),
            expired: vec![ExpiredLine {
                kind: "rule".into(),
                name: "r".into(),
                expires: "2026-01-01".into(),
            }],
            vacuous: vec![VacuousRule::new("v", "from")],
            ratchets: Ratchets::default(),
            warnings: vec!["a.ts: w".into()],
        }
    }

    #[test]
    fn a_verdict_round_trips_under_its_key_and_misses_under_another()
    -> Result<(), Box<dyn std::error::Error>> {
        for compressed in [false, true] {
            let dir = scratch(&format!("round-{compressed}"));
            store(&dir, "k1", &verdict(), compressed)?;
            assert_eq!(load(&dir, "k1", compressed), Ok(verdict()));
            assert_eq!(
                load(&dir, "k2", compressed),
                Err(Miss::Stale("evaluation key"))
            );
            assert_eq!(
                load(&dir, "k1", !compressed),
                Err(Miss::Stale("compress")),
                "the other compression finds no file"
            );
            let mut other = verdict();
            other.warnings.clear();
            store(&dir, "k2", &other, compressed)?;
            assert_eq!(load(&dir, "k2", compressed), Ok(other));
            let files: Vec<String> = std::fs::read_dir(&dir)?
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("evaluated-"))
                .collect();
            assert_eq!(files.len(), 1, "the old verdict is removed: {files:?}");
            let _ = std::fs::remove_dir_all(&dir);
        }
        Ok(())
    }

    #[test]
    fn a_damaged_verdict_is_a_miss() -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("damaged");
        assert_eq!(load(&dir, "k", false), Err(Miss::Absent));
        store(&dir, "k", &verdict(), false)?;
        let pointer: Pointer = serde_json::from_slice(&std::fs::read(dir.join(EVALUATED_FILE))?)?;
        let file = dir.join(verdict_file(&pointer.digest, false));
        std::fs::write(&file, b"{}")?;
        assert!(matches!(load(&dir, "k", false), Err(Miss::Corrupt(m)) if m.contains("digest")));
        // Bytes that match a forged digest but are not a verdict.
        let forged = Pointer {
            key: "k".into(),
            digest: digest(b"[1]"),
        };
        std::fs::write(dir.join(verdict_file(&forged.digest, false)), b"[1]")?;
        std::fs::write(dir.join(EVALUATED_FILE), serde_json::to_vec(&forged)?)?;
        assert!(matches!(load(&dir, "k", false), Err(Miss::Corrupt(_))));
        for text in ["", "{", "[]", r#"{"key":1}"#] {
            std::fs::write(dir.join(EVALUATED_FILE), text)?;
            assert!(
                matches!(load(&dir, "k", false), Err(Miss::Corrupt(_))),
                "{text}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn a_rendered_output_round_trips_and_a_damaged_one_is_a_miss()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("rendered");
        std::fs::create_dir_all(&dir)?;
        assert_eq!(load_rendered(&dir, "k"), Err(Miss::Absent));
        let tail = verdict().tail();
        assert_eq!(tail.error, 0);
        assert_eq!(tail.warnings, ["a.ts: w"]);
        store_rendered(&dir, "k", "out\n", tail.clone())?;
        assert_eq!(
            load_rendered(&dir, "k"),
            Ok(("out\n".to_owned(), tail.clone()))
        );
        assert_eq!(load_rendered(&dir, "j"), Err(Miss::Stale("report key")));
        std::fs::write(dir.join(RENDERED_OUTPUT), "tampered")?;
        assert!(matches!(load_rendered(&dir, "k"), Err(Miss::Corrupt(m)) if m.contains("digest")));
        std::fs::write(dir.join(RENDERED_FILE), "{")?;
        assert!(matches!(load_rendered(&dir, "k"), Err(Miss::Corrupt(_))));
        assert!(!stored_under(&dir, "k"));
        store(&dir, "k", &verdict(), false)?;
        assert!(stored_under(&dir, "k") && !stored_under(&dir, "other"));
        assert_ne!(render_key("v", "err"), render_key("v", "json"));
        assert_ne!(render_key("v", "err"), render_key("w", "err"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn the_key_joins_the_evaluation_and_the_extraction() {
        let one = key("p", "sha256:a");
        assert!(one.starts_with("sha256:") && one.len() == 71);
        assert_eq!(one, key("p", "sha256:a"));
        assert_ne!(one, key("p", "sha256:b"));
        assert_ne!(one, key("q", "sha256:a"));
        assert_eq!(
            verdict_file("sha256:0123456789abcdef99", true),
            "evaluated-0123456789abcdef.json.z"
        );
    }
}
