//! The `--cache` entry: `manifest.json`, which a later run checks before it trusts anything, and
//! the stored extraction it names.
//!
//! - Architecture: [Performance model](../../../../docs/architecture.md#performance-model)
//! - Plan: [Wave 3 § 1.5](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//!   (the manifest's shape), [Step 1](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)
//!   (`load` and `store`)
//! - Coverage: [coverage § Options](../../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options)
//!   (row `cache`: `folder`, `strategy`, `compress`)
//! - Decisions: [ADR-0008](../../../../docs/adr/0008-exit-code-contract.md) (an untrustworthy
//!   input never silently degrades a run: a doubtful entry is a miss),
//!   [ADR-0004](../../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//! - Requirement: [FR-CLI-05](../../../../docs/prd.md#fr-cli-05)
//!
//! The manifest carries the fields plan § 1.5 freezes (`toolVersion`, `configHash`, `worktree`,
//! `head`, `strategy`, `inputs`) and two additive ones: `stamps`, each input's size and
//! modification time for the `metadata` strategy, and `extraction`, the `sha256:` digest of the
//! stored extraction, which also names its file (`extraction-<first 16 hex digits>.json`, or
//! `.json.z` when `compress` is on, a zlib stream).
//!
//! [`store`] writes the extraction under its content-derived name first, then the manifest under
//! a temporary name renamed into place, then removes the extraction files no manifest names; a
//! reader therefore sees either the old pair or the new one, never a manifest with another
//! manifest's extraction. [`load`] checks, in order, that the manifest parses, that its
//! `toolVersion`, `configHash`, `worktree` and `strategy` are this run's, that the extraction it
//! names exists with the compression this run asks for, that its bytes hash to the recorded
//! digest, and that they decompress and parse. Any failure is a [`Miss`] naming why; nothing in an
//! entry is used before every check has passed, and nothing an entry holds can make a run panic.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rb_model::{CacheOptions, CacheStrategy};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::pipeline::Parts;

/// The manifest's file name in the cache folder.
pub const MANIFEST_FILE: &str = "manifest.json";

/// The most bytes any file of an entry may hold, compressed or inflated: a bound on memory
/// against a crafted or damaged entry, far above the stored extraction of the largest test bed
/// (tens of megabytes). A larger file is a miss, never an allocation.
pub const PAYLOAD_LIMIT: usize = 1 << 29;

/// Reads `path` whole when it holds at most `limit` bytes.
///
/// # Errors
/// The I/O error; `InvalidData` naming the limit when the file is larger.
pub fn read_limited(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    let cap = u64::try_from(limit).unwrap_or(u64::MAX);
    file.take(cap.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("larger than the {limit}-byte limit for a cache file"),
        ));
    }
    Ok(bytes)
}

/// What an entry must match before it is used: plan § 1.5's `CacheKey`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    /// The version of the build.
    pub tool_version: String,
    /// `sha256:` and the digest of the configuration and the extraction settings
    /// ([`super::key::extraction_hash`]).
    pub config_hash: String,
    /// The worktree root, `/`-separated.
    pub worktree: String,
    /// The commit `HEAD` names, or `None` outside a repository.
    pub head: Option<String>,
}

/// `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    /// The version of the build that wrote the entry.
    pub tool_version: String,
    /// The configuration hash the entry was written under.
    pub config_hash: String,
    /// The worktree root it was written in.
    pub worktree: String,
    /// The commit `HEAD` named when it was written.
    pub head: Option<String>,
    /// The strategy that wrote it.
    pub strategy: CacheStrategy,
    /// Every input, by path (relative to the working directory, `/`-separated, or absolute when
    /// outside it), to `sha256:` and the digest of its bytes.
    pub inputs: BTreeMap<String, String>,
    /// Additive: each input's size and modification time (nanoseconds since the epoch), for the
    /// `metadata` strategy.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stamps: BTreeMap<String, (u64, u64)>,
    /// Additive: `sha256:` and the digest of the stored extraction's bytes.
    pub extraction: String,
    /// Additive: values computed from the environment before the extraction (the .NET
    /// assemblies discovery finds, the Python environment), compared on the next run
    /// ([`super::changes::detect`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub probes: BTreeMap<String, String>,
    /// Additive: the inputs recorded only for their presence (files of the presence set no
    /// extractor or configuration reads): their appearing or disappearing is structural, a
    /// change to their bytes changes nothing extracted.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub watched: BTreeSet<String>,
}

impl Manifest {
    /// The key fields as a [`Key`].
    pub fn key(&self) -> Key {
        Key {
            tool_version: self.tool_version.clone(),
            config_hash: self.config_hash.clone(),
            worktree: self.worktree.clone(),
            head: self.head.clone(),
        }
    }
}

/// Why an entry was not used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Miss {
    /// No manifest in the folder.
    Absent,
    /// The manifest or the extraction cannot be read, or does not parse, or does not match its
    /// digest.
    Corrupt(String),
    /// The entry was written for something else: the named field differs.
    Stale(&'static str),
}

impl std::fmt::Display for Miss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Absent => f.write_str("no entry"),
            Self::Corrupt(reason) => write!(f, "the entry is unusable ({reason})"),
            Self::Stale(field) => write!(f, "the entry was written with another {field}"),
        }
    }
}

/// An entry that passed every check of [`load`].
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Its manifest.
    pub manifest: Manifest,
    /// The extraction it holds, without the per-file states: what a hit merges.
    pub parts: Parts,
    /// The per-file states, still as JSON: read only when a run extracts again
    /// ([`Entry::with_states`]).
    pub states: Vec<u8>,
}

impl Entry {
    /// The parts with each extraction's per-file states put back, which an incremental run
    /// reuses from.
    ///
    /// # Errors
    /// A [`Miss`] when the states do not parse; the caller extracts in full.
    pub fn with_states(&self) -> Result<Parts, Miss> {
        let states: States = serde_json::from_slice(&self.states)
            .map_err(|e| Miss::Corrupt(format!("the per-file states: {e}")))?;
        let mut parts = self.parts.clone();
        for (part, files) in [
            (&mut parts.typescript, states.typescript),
            (&mut parts.dotnet, states.dotnet),
            (&mut parts.python, states.python),
        ] {
            match part {
                Some(extraction) => extraction.files = files,
                None if files.is_empty() => {}
                None => {
                    return Err(Miss::Corrupt(
                        "per-file states for a language with no extraction".into(),
                    ));
                }
            }
        }
        Ok(parts)
    }
}

/// The per-file states of each part, stored after the parts so a hit parses only the parts.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct States {
    #[serde(default)]
    typescript: BTreeMap<String, rb_model::FileState>,
    #[serde(default)]
    dotnet: BTreeMap<String, rb_model::FileState>,
    #[serde(default)]
    python: BTreeMap<String, rb_model::FileState>,
}

/// [`States`] as written, borrowed from the parts.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatesOut<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    typescript: Option<&'a BTreeMap<String, rb_model::FileState>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dotnet: Option<&'a BTreeMap<String, rb_model::FileState>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    python: Option<&'a BTreeMap<String, rb_model::FileState>>,
}

/// One part as written: an [`rb_model::Extraction`] without its `files`, borrowed, so writing an
/// entry copies no module.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LeanPart<'a> {
    modules: &'a [rb_model::Module],
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'a rb_model::CodeLayer>,
    inspected: &'a rb_model::Receipt,
    #[serde(skip_serializing_if = "<[rb_model::Warning]>::is_empty")]
    warnings: &'a [rb_model::Warning],
    #[serde(skip_serializing_if = "Option::is_none")]
    sidecar: Option<&'a rb_model::SidecarReceipt>,
}

impl<'a> LeanPart<'a> {
    fn of(extraction: &'a rb_model::Extraction) -> Self {
        Self {
            modules: &extraction.modules,
            code: extraction.code.as_ref(),
            inspected: &extraction.inspected,
            warnings: &extraction.warnings,
            sidecar: extraction.sidecar.as_ref(),
        }
    }
}

/// [`Parts`] as written, without the per-file states.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LeanParts<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    typescript: Option<LeanPart<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dotnet: Option<LeanPart<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    python: Option<LeanPart<'a>>,
}

/// The stored bytes of `parts`: the parts without their per-file states, a newline, then the
/// states.
fn serialise(parts: &Parts) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(&LeanParts {
        typescript: parts.typescript.as_ref().map(LeanPart::of),
        dotnet: parts.dotnet.as_ref().map(LeanPart::of),
        python: parts.python.as_ref().map(LeanPart::of),
    })?;
    bytes.push(b'\n');
    serde_json::to_writer(
        &mut bytes,
        &StatesOut {
            typescript: parts.typescript.as_ref().map(|p| &p.files),
            dotnet: parts.dotnet.as_ref().map(|p| &p.files),
            python: parts.python.as_ref().map(|p| &p.files),
        },
    )?;
    Ok(bytes)
}

/// Why an entry could not be written. The run goes on without it: the cache is a speed-up.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// A file in the cache folder could not be written or renamed.
    #[error("cannot write {path}: {reason}; check that the cache folder is writable, or pass --no-cache", path = path.display())]
    Write {
        /// The file.
        path: PathBuf,
        /// The operating system's reason.
        reason: String,
    },
    /// The extraction could not be serialised.
    #[error("cannot serialise the extraction: {0}")]
    Serialise(String),
}

/// `sha256:` and the lowercase hex digest of `bytes`.
pub fn digest(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::from("sha256:"), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// The file name the extraction with `digest` is stored under.
pub fn extraction_file(digest: &str, compressed: bool) -> String {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    let short: String = hex.chars().take(16).collect();
    format!(
        "extraction-{short}.json{}",
        if compressed { ".z" } else { "" }
    )
}

/// Whether `name` is an extraction file an entry could name, so [`store`] removes only its own.
fn is_extraction_file(name: &str) -> bool {
    name.strip_prefix("extraction-")
        .and_then(|rest| {
            rest.strip_suffix(".json")
                .or_else(|| rest.strip_suffix(".json.z"))
        })
        .is_some_and(|hex| hex.len() == 16 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Reads the entry in `folder` and checks it against `key` and `options`, as the module doc lists.
///
/// # Errors
/// A [`Miss`] naming why the entry cannot be used; the caller extracts afresh.
pub fn load(folder: &Path, key: &Key, options: &CacheOptions) -> Result<Entry, Miss> {
    let manifest = load_manifest(folder, key, options)?;
    load_extraction(folder, manifest, options)
}

/// The first half of [`load`]: the manifest, checked against `key` and the strategy.
///
/// # Errors
/// A [`Miss`] naming why the manifest cannot be used.
pub fn load_manifest(folder: &Path, key: &Key, options: &CacheOptions) -> Result<Manifest, Miss> {
    let manifest_path = folder.join(MANIFEST_FILE);
    let text = match read_limited(&manifest_path, PAYLOAD_LIMIT) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Miss::Absent),
        Err(e) => return Err(Miss::Corrupt(format!("{}: {e}", manifest_path.display()))),
    };
    let manifest: Manifest = serde_json::from_slice(&text)
        .map_err(|e| Miss::Corrupt(format!("{}: {e}", manifest_path.display())))?;
    if manifest.tool_version != key.tool_version {
        return Err(Miss::Stale("toolVersion"));
    }
    if manifest.config_hash != key.config_hash {
        return Err(Miss::Stale("configHash"));
    }
    if manifest.worktree != key.worktree {
        return Err(Miss::Stale("worktree"));
    }
    if manifest.strategy != options.strategy {
        return Err(Miss::Stale("strategy"));
    }
    Ok(manifest)
}

/// The bytes of the stored file `name` in `folder`, checked against `expected` (a `sha256:`
/// digest of the stored bytes) and inflated when `compressed`.
///
/// # Errors
/// A [`Miss`]: `Stale("compress")` when the file is absent, `Corrupt` when it cannot be read,
/// does not match its digest or does not inflate.
pub fn read_payload(
    folder: &Path,
    name: &str,
    expected: &str,
    compressed: bool,
) -> Result<Vec<u8>, Miss> {
    read_payload_limited(folder, name, expected, compressed, PAYLOAD_LIMIT)
}

/// [`read_payload`] with the size bound given: `limit` bytes stored, and `limit` inflated.
///
/// # Errors
/// As [`read_payload`], and `Corrupt` for a file or an inflation over `limit`.
pub fn read_payload_limited(
    folder: &Path,
    name: &str,
    expected: &str,
    compressed: bool,
    limit: usize,
) -> Result<Vec<u8>, Miss> {
    let path = folder.join(name);
    let payload = read_limited(&path, limit).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Miss::Stale("compress")
        } else {
            Miss::Corrupt(format!("{}: {e}", path.display()))
        }
    })?;
    if digest(&payload) != expected {
        return Err(Miss::Corrupt(format!(
            "{} does not match its digest",
            path.display()
        )));
    }
    if compressed {
        miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&payload, limit)
            .map_err(|e| Miss::Corrupt(format!("{}: {e:?}", path.display())))
    } else {
        Ok(payload)
    }
}

/// The second half of [`load`]: the extraction `manifest` names.
///
/// # Errors
/// A [`Miss`] naming why the extraction cannot be used.
pub fn load_extraction(
    folder: &Path,
    manifest: Manifest,
    options: &CacheOptions,
) -> Result<Entry, Miss> {
    let compressed = options.compressed();
    let name = extraction_file(&manifest.extraction, compressed);
    let path = folder.join(&name);
    let bytes = read_payload(folder, &name, &manifest.extraction, compressed)?;
    let mut stream = serde_json::Deserializer::from_slice(&bytes).into_iter::<Parts>();
    let parts = stream
        .next()
        .ok_or_else(|| Miss::Corrupt(format!("{} is empty", path.display())))?
        .map_err(|e| Miss::Corrupt(format!("{}: {e}", path.display())))?;
    let at = stream.byte_offset();
    let mut bytes = bytes;
    let states = bytes.split_off(at);
    Ok(Entry {
        manifest,
        parts,
        states,
    })
}

/// Writes `parts` and `manifest` (its `extraction` field is set here) to `folder`, as the module
/// doc describes, and returns the manifest as written.
///
/// # Errors
/// [`CacheError`] when a file cannot be written; an entry half written is never named by a
/// manifest.
pub fn store(
    folder: &Path,
    mut manifest: Manifest,
    parts: &Parts,
    options: &CacheOptions,
) -> Result<Manifest, CacheError> {
    let write_error = |path: &Path, e: &std::io::Error| CacheError::Write {
        path: path.to_path_buf(),
        reason: e.to_string(),
    };
    std::fs::create_dir_all(folder).map_err(|e| write_error(folder, &e))?;
    let bytes = serialise(parts).map_err(|e| CacheError::Serialise(e.to_string()))?;
    let compressed = options.compressed();
    let payload = if compressed {
        // The fastest level, as dependency-cruiser compresses its cache at brotli's lowest
        // quality: the entry is written on every run that extracts.
        miniz_oxide::deflate::compress_to_vec_zlib(&bytes, 1)
    } else {
        bytes
    };
    manifest.extraction = digest(&payload);
    let name = extraction_file(&manifest.extraction, compressed);
    let target = folder.join(&name);
    // An existing file under the name is kept only when it holds these bytes, so a damaged one
    // is repaired rather than failing every later load.
    if std::fs::read(&target).ok().as_deref() != Some(payload.as_slice()) {
        replace(folder, &name, &payload).map_err(|e| write_error(&target, &e))?;
    }
    store_manifest(folder, &manifest)?;
    if let Ok(listing) = std::fs::read_dir(folder) {
        for entry in listing.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            if file != name && is_extraction_file(&file) {
                // Best effort: another run may be reading or removing it.
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok(manifest)
}

/// Rewrites the manifest alone, for an entry whose extraction did not change (new stamps, a
/// new `HEAD`, an input no extractor reads edited). The extraction it names must be in `folder`.
///
/// # Errors
/// [`CacheError`] when the manifest cannot be written.
pub fn store_manifest(folder: &Path, manifest: &Manifest) -> Result<(), CacheError> {
    let mut text =
        serde_json::to_vec_pretty(manifest).map_err(|e| CacheError::Serialise(e.to_string()))?;
    text.push(b'\n');
    replace(folder, MANIFEST_FILE, &text).map_err(|e| CacheError::Write {
        path: folder.join(MANIFEST_FILE),
        reason: e.to_string(),
    })
}

/// Writes `bytes` to `folder/name` through a temporary name, then renames it into place.
pub(crate) fn replace(folder: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = folder.join(format!("{name}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, folder.join(name)).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rb-cache-manifest-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn key() -> Key {
        Key {
            tool_version: "0.3.0".into(),
            config_hash: "sha256:c".into(),
            worktree: "/w".into(),
            head: Some("h".into()),
        }
    }

    fn manifest() -> Manifest {
        let key = key();
        Manifest {
            tool_version: key.tool_version,
            config_hash: key.config_hash,
            worktree: key.worktree,
            head: key.head,
            strategy: CacheStrategy::Metadata,
            inputs: BTreeMap::from([("src/a.ts".to_owned(), digest(b"a"))]),
            stamps: BTreeMap::new(),
            extraction: String::new(),
            probes: BTreeMap::new(),
            watched: BTreeSet::new(),
        }
    }

    fn parts() -> Parts {
        Parts {
            typescript: Some(rb_model::Extraction {
                modules: vec![rb_model::Module::new("src/a.ts")],
                // The sidecar's receipt is part of what a hit must give back.
                sidecar: Some(rb_model::SidecarReceipt {
                    tool: "dependency-cruiser".to_owned(),
                    version: "18.2.0".to_owned(),
                    files: 1,
                }),
                ..rb_model::Extraction::default()
            }),
            ..Parts::default()
        }
    }

    #[test]
    fn the_digest_is_sha256_and_names_the_file() {
        assert_eq!(
            digest(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            extraction_file(&digest(b"abc"), false),
            "extraction-ba7816bf8f01cfea.json"
        );
        assert_eq!(
            extraction_file(&digest(b"abc"), true),
            "extraction-ba7816bf8f01cfea.json.z"
        );
        assert!(is_extraction_file("extraction-ba7816bf8f01cfea.json"));
        assert!(is_extraction_file("extraction-ba7816bf8f01cfea.json.z"));
        for other in [
            "extraction-ba78.json",
            "extraction-zz7816bf8f01cfea.json",
            "manifest.json",
            "extraction-ba7816bf8f01cfea.json.tmp",
        ] {
            assert!(!is_extraction_file(other), "{other}");
        }
    }

    #[test]
    fn an_entry_round_trips_compressed_or_not() -> Result<(), Box<dyn std::error::Error>> {
        for compress in [None, Some(false), Some(true)] {
            let dir = scratch(&format!("round-{compress:?}"));
            let options = CacheOptions {
                compress,
                ..CacheOptions::in_folder("x")
            };
            let written = store(&dir, manifest(), &parts(), &options)?;
            assert!(written.extraction.starts_with("sha256:"));
            let entry = load(&dir, &key(), &options);
            assert_eq!(
                entry.as_ref().map(|e| (&e.parts, &e.manifest)),
                Ok((&parts(), &written))
            );
            assert_eq!(written.key(), key());
            let name = extraction_file(&written.extraction, options.compressed());
            let payload = std::fs::read(dir.join(&name))?;
            assert_eq!(
                payload.starts_with(b"{"),
                !options.compressed(),
                "{compress:?}"
            );
            // The other compression finds no file of its own: a miss, never a misread.
            let other = CacheOptions {
                compress: Some(!options.compressed()),
                ..options.clone()
            };
            assert_eq!(load(&dir, &key(), &other), Err(Miss::Stale("compress")));
            let _ = std::fs::remove_dir_all(&dir);
        }
        Ok(())
    }

    #[test]
    fn the_states_are_stored_after_the_parts_and_read_on_demand()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("states");
        let options = CacheOptions::in_folder("x");
        let mut with_files = parts();
        if let Some(typescript) = with_files.typescript.as_mut() {
            typescript.files.insert(
                "src/a.ts".into(),
                rb_model::FileState {
                    code: Some(serde_json::json!({ "file": "src/a.ts" })),
                    warnings: vec![rb_model::Warning::about("src/a.ts", "w")],
                },
            );
        }
        with_files.python = Some(rb_model::Extraction::default());
        store(&dir, manifest(), &with_files, &options)?;
        let entry = load(&dir, &key(), &options).map_err(|m| m.to_string())?;
        assert!(
            entry
                .parts
                .typescript
                .as_ref()
                .is_some_and(|p| p.files.is_empty())
        );
        assert_eq!(entry.with_states(), Ok(with_files.clone()));
        let mut broken = entry.clone();
        broken.states = b"{\"typescript\": 3}".to_vec();
        assert!(matches!(broken.with_states(), Err(Miss::Corrupt(_))));
        let orphan = Entry {
            parts: Parts::default(),
            states: br#"{"dotnet":{"x":{}}}"#.to_vec(),
            ..entry.clone()
        };
        assert!(
            matches!(orphan.with_states(), Err(Miss::Corrupt(m)) if m.contains("no extraction"))
        );
        let bare = Entry {
            states: b"{}".to_vec(),
            ..entry
        };
        assert_eq!(
            bare.with_states()
                .map(|p| p.typescript.map(|t| t.files.len())),
            Ok(Some(0))
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn each_key_field_and_the_strategy_make_a_miss() -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("stale");
        let options = CacheOptions::in_folder("x");
        store(&dir, manifest(), &parts(), &options)?;
        assert!(load(&dir, &key(), &options).is_ok());
        let table = [
            (
                Key {
                    tool_version: "0.4.0".into(),
                    ..key()
                },
                "toolVersion",
            ),
            (
                Key {
                    config_hash: "sha256:d".into(),
                    ..key()
                },
                "configHash",
            ),
            (
                Key {
                    worktree: "/elsewhere".into(),
                    ..key()
                },
                "worktree",
            ),
        ];
        for (other, field) in table {
            assert_eq!(load(&dir, &other, &options), Err(Miss::Stale(field)));
        }
        // A different HEAD is not a miss by itself: the strategy decides what it changed.
        let moved = Key {
            head: Some("other".into()),
            ..key()
        };
        assert!(load(&dir, &moved, &options).is_ok());
        let content = CacheOptions {
            strategy: CacheStrategy::Content,
            ..options.clone()
        };
        assert_eq!(load(&dir, &key(), &content), Err(Miss::Stale("strategy")));
        assert_eq!(
            load(&dir.join("absent"), &key(), &options),
            Err(Miss::Absent)
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn a_damaged_entry_is_a_miss_never_a_panic() -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("damaged");
        for compress in [false, true] {
            let options = CacheOptions {
                compress: Some(compress),
                ..CacheOptions::in_folder("x")
            };
            let written = store(&dir, manifest(), &parts(), &options)?;
            let name = dir.join(extraction_file(&written.extraction, compress));
            let good = std::fs::read(&name)?;
            // Truncated, flipped, emptied: the digest check refuses each.
            let half = good.get(..good.len() / 2).unwrap_or_default().to_vec();
            let mut flipped = good.clone();
            if let Some(byte) = flipped.last_mut() {
                *byte ^= 1;
            }
            for bad in [half, flipped, Vec::new()] {
                std::fs::write(&name, &bad)?;
                assert!(
                    matches!(load(&dir, &key(), &options), Err(Miss::Corrupt(m)) if m.contains("digest"))
                );
            }
            // A payload that matches a forged digest but is not an extraction.
            for forged in [b"[1]".to_vec(), b"\x78\x9c garbage".to_vec()] {
                let forged_manifest = Manifest {
                    extraction: digest(&forged),
                    ..written.clone()
                };
                std::fs::write(
                    dir.join(extraction_file(&forged_manifest.extraction, compress)),
                    &forged,
                )?;
                std::fs::write(
                    dir.join(MANIFEST_FILE),
                    serde_json::to_vec(&forged_manifest)?,
                )?;
                assert!(matches!(
                    load(&dir, &key(), &options),
                    Err(Miss::Corrupt(_))
                ));
            }
        }
        for manifest_text in [
            "",
            "{",
            "[]",
            "null",
            r#"{"toolVersion":1}"#,
            r#"{"toolVersion":"0.3.0","configHash":"sha256:c","worktree":"/w","head":null,"strategy":"sideways","inputs":{},"extraction":""}"#,
        ] {
            std::fs::write(dir.join(MANIFEST_FILE), manifest_text)?;
            assert!(
                matches!(
                    load(&dir, &key(), &CacheOptions::in_folder("x")),
                    Err(Miss::Corrupt(_))
                ),
                "{manifest_text}"
            );
        }
        std::fs::create_dir_all(dir.join("as-folder").join(MANIFEST_FILE))?;
        assert!(matches!(
            load(
                &dir.join("as-folder"),
                &key(),
                &CacheOptions::in_folder("x")
            ),
            Err(Miss::Corrupt(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn a_new_entry_removes_the_old_extraction_and_keeps_stray_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("replace");
        let options = CacheOptions::in_folder("x");
        let first = store(&dir, manifest(), &parts(), &options)?;
        std::fs::write(dir.join("notes.txt"), "mine")?;
        let mut other = parts();
        other.python = Some(rb_model::Extraction::default());
        let second = store(&dir, manifest(), &other, &options)?;
        assert_ne!(first.extraction, second.extraction);
        let mut names: Vec<String> = std::fs::read_dir(&dir)?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                extraction_file(&second.extraction, false),
                MANIFEST_FILE.to_owned(),
                "notes.txt".to_owned()
            ]
        );
        // Storing the same extraction again rewrites only the manifest.
        let again = store(&dir, manifest(), &other, &options)?;
        assert_eq!(again, second);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn an_unwritable_folder_is_a_named_error() -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("unwritable");
        std::fs::create_dir_all(dir.parent().unwrap_or(&dir))?;
        std::fs::write(&dir, "a file where the folder should be")?;
        let error = store(
            &dir.join("cache"),
            manifest(),
            &parts(),
            &CacheOptions::in_folder("x"),
        );
        assert!(error.is_err_and(|e| e.to_string().contains("--no-cache")));
        let _ = std::fs::remove_file(&dir);
        Ok(())
    }

    #[test]
    fn a_file_over_the_limit_is_a_miss_never_an_allocation()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("limit");
        std::fs::create_dir_all(&dir)?;
        let big = vec![b'x'; 4096];
        std::fs::write(dir.join("big"), &big)?;
        assert_eq!(read_limited(&dir.join("big"), 4096)?.len(), 4096);
        let over = read_limited(&dir.join("big"), 4095);
        assert!(over.is_err_and(|e| e.kind() == std::io::ErrorKind::InvalidData));
        assert!(matches!(
            read_payload_limited(&dir, "big", &digest(&big), false, 100),
            Err(Miss::Corrupt(m)) if m.contains("limit")
        ));
        // A small stream that inflates past the limit.
        let bomb = miniz_oxide::deflate::compress_to_vec_zlib(&vec![0u8; 1 << 20], 9);
        std::fs::write(dir.join("bomb"), &bomb)?;
        assert!(bomb.len() < 4096);
        assert!(matches!(
            read_payload_limited(&dir, "bomb", &digest(&bomb), true, 4096),
            Err(Miss::Corrupt(_))
        ));
        assert_eq!(
            read_payload_limited(&dir, "bomb", &digest(&bomb), true, 1 << 21).map(|b| b.len()),
            Ok(1 << 20)
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn misses_say_why() {
        assert_eq!(Miss::Absent.to_string(), "no entry");
        assert_eq!(
            Miss::Stale("configHash").to_string(),
            "the entry was written with another configHash"
        );
        assert!(
            Miss::Corrupt("x".into())
                .to_string()
                .contains("unusable (x)")
        );
    }
}
