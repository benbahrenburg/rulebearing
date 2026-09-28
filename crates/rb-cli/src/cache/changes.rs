//! What changed since a `--cache` entry was written: the two strategies, and the input set an
//! entry records.
//!
//! - Architecture: [Performance model](../../../../docs/architecture.md#performance-model)
//! - Plan: [Wave 3 § 1.4](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#the-cache-and---affected)
//!   ("`metadata` uses git status and file metadata to decide what changed, `content` hashes
//!   every file"), [Step 1](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)
//! - Coverage: [coverage § Options](../../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options)
//!   (row `cache`), [coverage § Command line](../../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line)
//!   (`--cache-strategy`)
//! - Decision: [ADR-0008](../../../../docs/adr/0008-exit-code-contract.md)
//! - Requirement: [FR-CLI-05](../../../../docs/prd.md#fr-cli-05)
//!
//! An entry records its inputs, each with a `sha256:` digest (and, for `metadata`, its size and
//! modification time):
//!
//! | Input | Recorded as |
//! | --- | --- |
//! | every file an extractor read (the parts' per-file states) | its digest; a changed TypeScript or Python source is read again alone |
//! | every other file the run read: the manifests of the cache key, every `package.json` above a TypeScript file, the tsconfig with its `extends` chain and references, the Babel configuration, the Plug'n'Play map, the solution and project files | its digest; any change is structural |
//! | every assembly and PDB in an analysed project's output folder | its digest; a change reads the .NET graph again |
//! | a file that may appear and change resolution (a manifest the key names, `node_modules/.package-lock.json`, `.modules.yaml`, `.yarn-state.yml`) | [`ABSENT`] while it does not exist; its appearance is structural |
//! | the folder of every relative import's target, a resolved one's or an unresolved one's nearest existing folder | the digest of its entry names (a name ending `/`); an entry added or removed is structural, whether or not version control ignores it |
//! | the presence set: files whose appearance could change what an unchanged file resolves to | as above; `content` lists them by walking the worktree (or the folder holding every input, when wider), `metadata` takes git's untracked and added files, or walks outside a repository |
//!
//! Beside the inputs an entry records *probes*, values computed from the environment before the
//! extraction: the .NET assemblies discovery finds (so a project built since, whose output
//! folder was never recorded, is seen) and the Python environment (the chosen `site-packages`
//! and its distributions, so an installation into an ignored `.venv` is seen). A probe that
//! differs is structural. Paths are relative to the working directory, or absolute outside it;
//! `.git`, `.graph`, `node_modules`, virtual environments (a folder holding `pyvenv.cfg`) and the
//! cache folder are never walked.
//!
//! [`detect`] answers with the recorded inputs whose content changed, and a *structural* reason
//! when the change is one reuse cannot follow (a file added or deleted, any input other than a
//! source changed, a probe changed, git unable to say what changed); the caller then extracts in
//! full.
//!
//! | Strategy | Deleted | Added | Content |
//! | --- | --- | --- | --- |
//! | `metadata`, in a repository | a recorded input missing, or git listing a relevant file deleted since the recorded `HEAD` | git listing a relevant file added or untracked that is not recorded; a recorded folder's entries or a probe changing | hashed only when its size or modification time changed, or git lists it modified since the recorded `HEAD` |
//! | `metadata`, outside a repository | a recorded input missing | the walk finding a relevant file not recorded; a recorded folder's entries or a probe changing | hashed only when its size or modification time changed |
//! | `content` | a recorded input missing | the walk finding a relevant file not recorded; a recorded folder's entries or a probe changing | every input hashed |
//!
//! An input is recorded after the extraction that read it. One whose modification time is after
//! the run started, or that changed while it was being recorded, or that could not be read, is
//! recorded as [`UNSETTLED`], a digest nothing matches, so the next run reads it again rather than
//! trusting an extraction of other bytes.
//!
//! Outside a repository dependency-cruiser's metadata strategy stops with an error; this one
//! falls back to the walk and file metadata instead, so `--cache` works in any folder. A file
//! whose edit keeps its size and modification time, and that git does not list, is not seen by
//! `metadata`; `content` sees every edit.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use rayon::prelude::*;
use rb_model::CacheStrategy;

use super::key;
use super::manifest::{Manifest, digest};

/// The digest recorded for an input that must be read again next time: it matches no file.
pub const UNSETTLED: &str = "sha256:unsettled";

/// The digest recorded for an optional input that does not exist.
pub const ABSENT: &str = "absent";

/// The extensions an extractor reads or a resolution can land on: a file with one appearing or
/// disappearing can change what an unchanged file resolves to.
pub const RELEVANT_EXTENSIONS: &[&str] = &[
    "js",
    "cjs",
    "mjs",
    "jsx",
    "ts",
    "tsx",
    "mts",
    "cts",
    "vue",
    "svelte",
    "json",
    "py",
    "pyi",
    "coffee",
    "litcoffee",
    "ls",
    "cjsx",
    "csx",
    "dll",
    "pdb",
];

/// Folders never listed: version control, Rulebearing's own output, installed packages (their
/// lock files stand for them).
const SKIPPED: &[&str] = &[".git", ".graph", "node_modules"];

/// Manifests beyond the cache key's list: the Yarn Plug'n'Play map and Python lock files, whose
/// change can move a resolution.
const MORE_MANIFESTS: &[&str] = &[
    ".pnp.cjs",
    ".pnp.data.json",
    "poetry.lock",
    "uv.lock",
    "Pipfile.lock",
    "requirements.txt",
];

/// Where a run is: the folder paths are relative to, the worktree, and what not to list.
#[derive(Debug, Clone)]
pub struct Scope {
    /// The working directory, canonical.
    pub base: PathBuf,
    /// The worktree root, canonical.
    pub root: PathBuf,
    /// The commit `HEAD` names, or `None` outside a repository.
    pub head: Option<String>,
    /// `extraExtensionsToScan`, with their dots.
    pub extra_extensions: Vec<String>,
    /// The cache folder, canonical when it exists.
    pub cache_folder: PathBuf,
}

impl Scope {
    /// The name a path is recorded under: relative to the working directory when inside it,
    /// else absolute, `/`-separated either way.
    pub fn name(&self, path: &Path) -> String {
        if let Ok(relative) = path.strip_prefix(&self.base) {
            return key::slashed(relative);
        }
        match path.canonicalize() {
            Ok(canonical) => canonical.strip_prefix(&self.base).map_or_else(
                |_| key::strip_verbatim(&key::slashed(&canonical)),
                key::slashed,
            ),
            Err(_) => key::slashed(path),
        }
    }

    /// The file a recorded name stands for.
    pub fn on_disk(&self, name: &str) -> PathBuf {
        let path = Path::new(name);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base.join(path)
        }
    }

    /// Whether a recorded name is under a folder that is never listed.
    fn skipped(&self, name: &str) -> bool {
        name.split('/').any(|part| SKIPPED.contains(&part))
            || self.on_disk(name).starts_with(&self.cache_folder)
    }

    /// Whether a file's appearance or disappearance can change an extraction: a manifest, or an
    /// extension an extractor reads or resolves to.
    pub fn is_relevant(&self, name: &str) -> bool {
        if is_manifest(name) {
            return true;
        }
        let file = name.rsplit('/').next().unwrap_or(name);
        // Literate CoffeeScript under its double extension, which the sidecar reads.
        if file.ends_with(".coffee.md") {
            return true;
        }
        file.rsplit_once('.').is_some_and(|(_, extension)| {
            RELEVANT_EXTENSIONS.contains(&extension)
                || self
                    .extra_extensions
                    .iter()
                    .any(|e| e.trim_start_matches('.') == extension)
        })
    }
}

/// Whether a file is a manifest: its change can alter how unchanged files resolve, so reuse
/// stops and the run extracts in full.
pub fn is_manifest(name: &str) -> bool {
    let file = name.rsplit('/').next().unwrap_or(name);
    let extension = file.rsplit_once('.').map(|(_, e)| e);
    key::MANIFESTS.contains(&file)
        || MORE_MANIFESTS.contains(&file)
        || extension.is_some_and(|e| key::MANIFEST_EXTENSIONS.contains(&e) || e == "props")
        || ((file.starts_with("tsconfig") || file.starts_with("jsconfig"))
            && extension == Some("json"))
}

/// A file's `sha256:` digest, or `None` when it cannot be read.
pub fn hash_file(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|bytes| digest(&bytes))
}

/// A file's size and modification time (nanoseconds since the epoch), or `None` when it is not a
/// readable file.
pub fn stamp(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path)
        .ok()
        .filter(std::fs::Metadata::is_file)?;
    Some((meta.len(), nanos(meta.modified().ok())))
}

/// Whether a recorded name is a folder's listing (it ends `/`).
pub fn is_listing(name: &str) -> bool {
    name.ends_with('/')
}

/// The digest of a folder's entry names (a folder's with `/` after it), sorted, or `None` when
/// it is not a folder.
pub fn listing(path: &Path) -> Option<String> {
    let mut names: Vec<String> = std::fs::read_dir(path)
        .ok()?
        .flatten()
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                format!("{name}/")
            } else {
                name
            }
        })
        .collect();
    names.sort();
    Some(digest(names.join("\n").as_bytes()))
}

/// A recorded input's size and modification time: a file's, or a folder's (its entry count and
/// its modification time, which adding or removing an entry moves).
pub fn observe(scope: &Scope, name: &str) -> Option<Stamp> {
    let path = scope.on_disk(name.trim_end_matches('/'));
    if is_listing(name) {
        let meta = std::fs::metadata(&path)
            .ok()
            .filter(std::fs::Metadata::is_dir)?;
        let count = std::fs::read_dir(&path).ok()?.count() as u64;
        return Some((count, nanos(meta.modified().ok())));
    }
    stamp(&path)
}

/// A recorded input's digest now: a file's bytes, or a folder's entry names.
pub fn content(scope: &Scope, name: &str) -> Option<String> {
    let path = scope.on_disk(name.trim_end_matches('/'));
    if is_listing(name) {
        listing(&path)
    } else {
        hash_file(&path)
    }
}

fn nanos(time: Option<std::time::SystemTime>) -> u64 {
    time.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

/// The folder a walk starts from: the worktree root, or the deepest folder holding it and every
/// recorded input when an input lies outside it, so a file created beside an input outside the
/// working directory is seen.
pub fn walk_root<'a>(scope: &Scope, names: impl Iterator<Item = &'a String>) -> PathBuf {
    let mut root = scope.root.clone();
    for name in names {
        let path = scope.on_disk(name.trim_end_matches('/'));
        let folder = if is_listing(name) {
            path
        } else {
            path.parent()
                .map_or_else(|| path.clone(), Path::to_path_buf)
        };
        while !folder.starts_with(&root) {
            match root.parent() {
                // Never the file system's root: a walk of everything is not a presence check.
                Some(parent) if parent.parent().is_some() => root = parent.to_path_buf(),
                _ => return scope.root.clone(),
            }
        }
    }
    root
}

/// Every relevant file under `from`, by recorded name, sorted. A virtual environment (a folder
/// holding `pyvenv.cfg`) is not walked: the Python environment probe stands for it.
pub fn walk(scope: &Scope, from: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut folders = vec![from.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = scope.name(&path);
            if scope.skipped(&name) {
                continue;
            }
            if kind.is_dir() {
                if !path.join("pyvenv.cfg").is_file() {
                    folders.push(path);
                }
            } else if kind.is_file() && scope.is_relevant(&name) {
                found.insert(name);
            }
        }
    }
    found
}

/// How git lists a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listed {
    /// Added since the commit, or untracked.
    Added,
    /// Deleted since the commit.
    Deleted,
    /// Changed in any other way (modified, type changed, unmerged).
    Changed,
}

fn git(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    Command::new("git")
        .args(["-c", "core.quotepath=off"])
        .args(args)
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| o.stdout)
}

/// What git says changed since `head` in the worktree (committed, staged and unstaged), with the
/// untracked files, by recorded name; `None` when git cannot answer (no repository, or `head` is
/// no longer an object it has).
pub fn git_changes(scope: &Scope, head: &str) -> Option<Vec<(Listed, String)>> {
    let diff = git(
        &scope.root,
        &[
            "diff",
            "--name-status",
            "-z",
            "--no-renames",
            "--ignore-submodules=none",
            head,
            "--",
        ],
    )?;
    let untracked = git(
        &scope.root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    let mut listed = Vec::new();
    let mut fields = diff.split(|b| *b == 0).filter(|f| !f.is_empty());
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        let kind = match status.first() {
            Some(b'A' | b'C') => Listed::Added,
            Some(b'D') => Listed::Deleted,
            _ => Listed::Changed,
        };
        listed.push((kind, String::from_utf8_lossy(path).into_owned()));
    }
    listed.extend(
        untracked
            .split(|b| *b == 0)
            .filter(|f| !f.is_empty())
            .map(|p| (Listed::Added, String::from_utf8_lossy(p).into_owned())),
    );
    Some(
        listed
            .into_iter()
            .map(|(kind, path)| (kind, scope.name(&scope.root.join(path))))
            .filter(|(_, name)| !scope.skipped(name))
            .collect(),
    )
}

/// The relevant files git lists as untracked or added to the index, by recorded name: the
/// presence set the `metadata` strategy records in a repository. `None` outside one.
pub fn git_presence(scope: &Scope) -> Option<BTreeSet<String>> {
    let status = git(
        &scope.root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ],
    )?;
    Some(
        status
            .split(|b| *b == 0)
            .filter(|e| e.len() > 3)
            .filter(|e| e.starts_with(b"??") || e.first() == Some(&b'A'))
            .filter_map(|e| e.get(3..))
            .map(|p| scope.name(&scope.root.join(String::from_utf8_lossy(p).as_ref())))
            .filter(|name| !scope.skipped(name) && scope.is_relevant(name))
            .collect(),
    )
}

/// What [`detect`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// Why reuse cannot follow the change, when it cannot; the first reason by name.
    pub structural: Option<String>,
    /// Recorded inputs whose content changed.
    pub modified: BTreeSet<String>,
    /// The current digest of every recorded input that still exists: freshly hashed, or the
    /// recorded one where the strategy trusts it unchanged.
    pub hashes: BTreeMap<String, String>,
    /// The current size and modification time of every recorded input that still exists.
    pub stamps: BTreeMap<String, (u64, u64)>,
}

impl Changes {
    /// Whether nothing changed at all.
    pub fn is_empty(&self) -> bool {
        self.structural.is_none() && self.modified.is_empty()
    }

    fn structural(&mut self, reason: String) {
        if self.structural.as_ref().is_none_or(|r| reason < *r) {
            self.structural = Some(reason);
        }
    }
}

/// A recorded input with its recorded digest and its size and time now.
type Observed<'a> = (&'a String, &'a String, Option<Stamp>);

/// An input with its digest and its size and time now, either missing when unreadable.
type Recorded = (String, Option<String>, Option<Stamp>);

/// A file's size and modification time in nanoseconds.
pub type Stamp = (u64, u64);

/// Compares the entry `manifest` records with the files as they are now, by `strategy`, and its
/// probes with `probes`, the values this run computed.
pub fn detect(
    manifest: &Manifest,
    strategy: CacheStrategy,
    scope: &Scope,
    probes: &BTreeMap<String, String>,
) -> Changes {
    let mut changes = Changes::default();
    for name in manifest.probes.keys().chain(probes.keys()) {
        if manifest.probes.get(name) != probes.get(name) {
            changes.structural(format!("the {name} changed"));
        }
    }
    let current: Vec<Observed<'_>> = manifest
        .inputs
        .par_iter()
        .map(|(name, hash)| (name, hash, observe(scope, name)))
        .collect();
    let mut candidates: BTreeSet<String> = BTreeSet::new();
    for (name, hash, now) in &current {
        if hash.as_str() == ABSENT {
            if now.is_some() {
                changes.structural(format!("{name} appeared"));
            } else {
                changes.hashes.insert((*name).clone(), ABSENT.to_owned());
            }
            continue;
        }
        let Some(now) = now else {
            changes.structural(format!("{name} was deleted"));
            continue;
        };
        changes.stamps.insert((*name).clone(), *now);
        let trusted = strategy == CacheStrategy::Metadata
            && hash.as_str() != UNSETTLED
            && manifest.stamps.get(*name) == Some(now);
        if trusted {
            changes.hashes.insert((*name).clone(), (*hash).clone());
        } else {
            candidates.insert((*name).clone());
        }
    }
    let presence = match (strategy, &manifest.head, &scope.head) {
        (CacheStrategy::Metadata, Some(recorded), Some(_)) => {
            match git_changes(scope, recorded) {
                Some(listed) => {
                    for (kind, name) in listed {
                        let recorded = manifest.inputs.contains_key(&name);
                        match kind {
                            Listed::Added if !recorded && scope.is_relevant(&name) => {
                                changes.structural(format!("{name} was added"));
                            }
                            Listed::Deleted if scope.is_relevant(&name) => {
                                changes.structural(format!("{name} was deleted"));
                            }
                            Listed::Added | Listed::Changed if recorded => {
                                if changes.stamps.contains_key(&name) {
                                    changes.hashes.remove(&name);
                                    candidates.insert(name);
                                }
                            }
                            Listed::Changed if is_manifest(&name) => {
                                changes.structural(format!("{name} changed"));
                            }
                            Listed::Added | Listed::Deleted | Listed::Changed => {}
                        }
                    }
                }
                None => changes.structural(format!("git cannot list the changes since {recorded}")),
            }
            None
        }
        (CacheStrategy::Metadata, Some(_), None) | (CacheStrategy::Metadata, None, Some(_)) => {
            changes.structural("the folder is no longer, or is now, in a git repository".into());
            None
        }
        (CacheStrategy::Metadata, None, None) | (CacheStrategy::Content, _, _) => {
            Some(walk(scope, &walk_root(scope, manifest.inputs.keys())))
        }
    };
    if let Some(present) = presence {
        for name in present.difference(&manifest.inputs.keys().cloned().collect()) {
            changes.structural(format!("{name} was added"));
        }
    }
    let hashed: Vec<(String, Option<String>)> = candidates
        .into_par_iter()
        .map(|name| {
            let hash = content(scope, &name);
            (name, hash)
        })
        .collect();
    for (name, hash) in hashed {
        match hash {
            Some(hash) => {
                if manifest.inputs.get(&name) != Some(&hash) {
                    changes.modified.insert(name.clone());
                }
                changes.hashes.insert(name, hash);
            }
            None => changes.structural(format!("{name} cannot be read")),
        }
    }
    changes
}

/// The presence set an entry written now records, by `strategy`: git's, or a walk from
/// [`walk_root`] over `inputs`.
pub fn presence(
    strategy: CacheStrategy,
    scope: &Scope,
    inputs: &BTreeSet<String>,
) -> BTreeSet<String> {
    let walked = || walk(scope, &walk_root(scope, inputs.iter()));
    match (strategy, &scope.head) {
        (CacheStrategy::Metadata, Some(_)) => git_presence(scope).unwrap_or_else(walked),
        (CacheStrategy::Metadata, None) | (CacheStrategy::Content, _) => walked(),
    }
}

/// The folders whose entries decide where relative imports land: for each `(importing file,
/// specifier)`, the folder the specifier names, or its nearest existing ancestor, as a listing
/// name. A file added there, ignored by version control or not, is then seen.
pub fn target_folders<'a>(
    scope: &Scope,
    imports: impl Iterator<Item = (&'a str, &'a str)>,
) -> BTreeSet<String> {
    let mut folders = BTreeSet::new();
    for (file, specifier) in imports {
        let relative = specifier.starts_with("./")
            || specifier.starts_with("../")
            || specifier == "."
            || specifier == "..";
        if !relative {
            continue;
        }
        let from = scope.on_disk(file);
        let Some(dir) = from.parent() else {
            continue;
        };
        let mut target = normalise(&dir.join(specifier));
        // The folder the target file would sit in; a target that is a folder is listed itself.
        if !target.is_dir() {
            target.pop();
        }
        while !target.is_dir() {
            if !target.pop() {
                break;
            }
        }
        // Never the file system's root: its listing is not a presence check.
        if target.parent().is_none_or(|p| p.parent().is_none()) {
            continue;
        }
        folders.insert(format!("{}/", scope.name(&target).trim_end_matches('/')));
    }
    folders
}

/// A path with `.` and `..` components resolved lexically.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Every `package.json` in the folders from each file's up to the worktree root, by recorded
/// name: the manifests a TypeScript file's resolution reads.
pub fn package_manifests<'a>(
    scope: &Scope,
    files: impl Iterator<Item = &'a str>,
) -> BTreeSet<String> {
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut found = BTreeSet::new();
    for file in files {
        let mut folder = scope.on_disk(file).parent().map(Path::to_path_buf);
        while let Some(dir) = folder {
            if !dir.starts_with(&scope.root) || !seen.insert(dir.clone()) {
                break;
            }
            let manifest = dir.join("package.json");
            if manifest.is_file() {
                found.insert(scope.name(&manifest));
            }
            folder = dir.parent().map(Path::to_path_buf);
        }
    }
    found
}

/// The digests and stamps of `inputs`, recorded after the extraction that read them: taken from
/// `known` (what [`detect`] verified before the run) where it has them, hashed otherwise. An
/// input in `optional` that does not exist is recorded as [`ABSENT`]; any other input that cannot
/// be read, or whose modification time is after `started` (nanoseconds since the epoch, taken
/// before the run looked at anything), or whose size or time moved while it was being recorded,
/// is recorded as [`UNSETTLED`], so the next run reads it again. On a file system that keeps
/// whole seconds only, a time within two seconds of `started` counts as after it.
pub fn record(
    scope: &Scope,
    inputs: &BTreeSet<String>,
    optional: &BTreeSet<String>,
    known: &BTreeMap<String, String>,
    strategy: CacheStrategy,
    started: u64,
) -> (BTreeMap<String, String>, BTreeMap<String, (u64, u64)>) {
    let recorded: Vec<Recorded> = inputs
        .union(optional)
        .collect::<Vec<_>>()
        .par_iter()
        .map(|name| {
            let before = observe(scope, name);
            let Some(before) = before else {
                let marker = if optional.contains(*name) && !inputs.contains(*name) {
                    ABSENT
                } else {
                    UNSETTLED
                };
                return ((*name).clone(), Some(marker.to_owned()), None);
            };
            let hash = known.get(*name).cloned().or_else(|| content(scope, name));
            let after = observe(scope, name);
            let settled =
                hash.is_some() && after == Some(before) && !after_start(before.1, started);
            let hash = if settled {
                hash
            } else {
                Some(UNSETTLED.to_owned())
            };
            ((*name).clone(), hash, Some(before))
        })
        .collect();
    let mut hashes = BTreeMap::new();
    let mut stamps = BTreeMap::new();
    for (name, hash, now) in recorded {
        if let (Some(now), CacheStrategy::Metadata) = (now, strategy) {
            stamps.insert(name.clone(), now);
        }
        hashes.insert(name, hash.unwrap_or_else(|| UNSETTLED.to_owned()));
    }
    (hashes, stamps)
}

/// Whether a modification time is after `started`: strictly after it, or, when the time has no
/// fraction of a second (a file system that keeps whole seconds), within two seconds before it.
fn after_start(modified: u64, started: u64) -> bool {
    const SECOND: u64 = 1_000_000_000;
    if modified.is_multiple_of(SECOND) {
        modified.saturating_add(2 * SECOND) > started
    } else {
        modified > started
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rb-cache-changes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir.canonicalize().unwrap_or(dir)
    }

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, text);
    }

    /// Now, as `record` takes it.
    fn now() -> u64 {
        nanos(Some(std::time::SystemTime::now()))
    }

    /// No probes.
    fn none() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    fn scope(dir: &Path) -> Scope {
        Scope {
            base: dir.to_path_buf(),
            root: dir.to_path_buf(),
            head: None,
            extra_extensions: vec![".md".into()],
            cache_folder: dir.join(".cache"),
        }
    }

    #[test]
    fn manifests_and_relevant_files_are_told_by_name() {
        for manifest in [
            "package.json",
            "a/package.json",
            "tsconfig.json",
            "tsconfig.base.json",
            "web/jsconfig.app.json",
            "App.csproj",
            "Directory.Build.props",
            "x.sln",
            "pyproject.toml",
            "uv.lock",
            ".pnp.cjs",
            "yarn.lock",
        ] {
            assert!(is_manifest(manifest), "{manifest}");
        }
        for other in ["src/a.ts", "tsconfig.yaml", "README.md", "package.json5"] {
            assert!(!is_manifest(other), "{other}");
        }
        let dir = scratch("names");
        let scope = scope(&dir);
        for relevant in [
            "a.ts",
            "b/c.d.ts",
            "x.json",
            "m.py",
            "s.pyi",
            "A.dll",
            "notes.md",
            "yarn.lock",
        ] {
            assert!(scope.is_relevant(relevant), "{relevant}");
        }
        for other in ["README", "a.txt", "image.png", "Makefile"] {
            assert!(!scope.is_relevant(other), "{other}");
        }
        // The sidecar's files are relevant; literate CoffeeScript even where `.md` is not listed.
        let plain = Scope {
            extra_extensions: Vec::new(),
            ..scope.clone()
        };
        for relevant in [
            "a.coffee",
            "a.litcoffee",
            "a.ls",
            "a.cjsx",
            "src/b.coffee.md",
        ] {
            assert!(plain.is_relevant(relevant), "{relevant}");
        }
        assert!(!plain.is_relevant("notes.md"));
        assert_eq!(scope.name(&dir.join("src/a.ts")), "src/a.ts");
        assert_eq!(scope.on_disk("src/a.ts"), dir.join("src/a.ts"));
        let outside = Path::new("/definitely/not/here.ts");
        assert_eq!(scope.name(outside), "/definitely/not/here.ts");
        assert_eq!(scope.on_disk("/definitely/not/here.ts"), outside);
        assert!(scope.skipped("node_modules/x/index.js"));
        assert!(scope.skipped("a/.graph/cache/manifest.json"));
        assert!(scope.skipped(".cache/extraction-0.json"));
        assert!(!scope.skipped("src/graph.ts"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_walk_lists_relevant_files_outside_skipped_folders() {
        let dir = scratch("walk");
        for file in [
            "src/a.ts",
            "src/b.txt",
            "node_modules/p/index.js",
            ".git/HEAD.json",
            ".graph/cache/manifest.json",
            ".cache/extraction-0123456789abcdef.json",
            "docs/n.md",
            "package.json",
        ] {
            write(&dir.join(file), "x");
        }
        write(&dir.join(".venv/pyvenv.cfg"), "home = /usr");
        write(
            &dir.join(".venv/lib/python3.12/site-packages/x/__init__.py"),
            "",
        );
        write(&dir.join("env/lib/y.py"), "");
        let found: Vec<String> = walk(&scope(&dir), &dir).into_iter().collect();
        assert_eq!(
            found,
            ["docs/n.md", "env/lib/y.py", "package.json", "src/a.ts"],
            "a virtual environment is not walked; a folder without pyvenv.cfg is"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn recorded(scope: &Scope, strategy: CacheStrategy) -> Manifest {
        let inputs: BTreeSet<String> = walk(scope, &scope.root);
        let (hashes, stamps) = record(
            scope,
            &inputs,
            &BTreeSet::new(),
            &BTreeMap::new(),
            strategy,
            now(),
        );
        Manifest {
            tool_version: "v".into(),
            config_hash: "sha256:c".into(),
            worktree: key::slashed(&scope.root),
            head: None,
            strategy,
            inputs: hashes,
            stamps,
            extraction: String::new(),
            probes: BTreeMap::new(),
            watched: BTreeSet::new(),
        }
    }

    #[test]
    fn each_strategy_sees_an_edit_an_addition_and_a_deletion() {
        for strategy in [CacheStrategy::Metadata, CacheStrategy::Content] {
            let dir = scratch(&format!("detect-{}", strategy.as_str()));
            let scope = scope(&dir);
            write(&dir.join("src/a.ts"), "export const a = 1;\n");
            write(&dir.join("src/b.ts"), "export const b = 1;\n");
            let manifest = recorded(&scope, strategy);
            assert_eq!(manifest.inputs.len(), 2);
            assert_eq!(
                manifest.stamps.is_empty(),
                strategy == CacheStrategy::Content
            );
            let quiet = detect(&manifest, strategy, &scope, &none());
            assert!(quiet.is_empty(), "{strategy:?} {quiet:?}");
            assert_eq!(quiet.hashes, manifest.inputs);
            // A longer file: a new size for metadata, a new digest for content.
            write(&dir.join("src/a.ts"), "export const a = 10;\n");
            let edited = detect(&manifest, strategy, &scope, &none());
            assert_eq!(edited.structural, None);
            assert_eq!(edited.modified.iter().collect::<Vec<_>>(), ["src/a.ts"]);
            assert_ne!(
                edited.hashes.get("src/a.ts"),
                manifest.inputs.get("src/a.ts")
            );
            write(&dir.join("src/c.ts"), "export const c = 1;\n");
            let added = detect(&manifest, strategy, &scope, &none());
            assert_eq!(added.structural.as_deref(), Some("src/c.ts was added"));
            let _ = std::fs::remove_file(dir.join("src/c.ts"));
            let _ = std::fs::remove_file(dir.join("src/b.ts"));
            let deleted = detect(&manifest, strategy, &scope, &none());
            assert_eq!(deleted.structural.as_deref(), Some("src/b.ts was deleted"));
            // An irrelevant file comes and goes unseen.
            write(&dir.join("src/b.ts"), "export const b = 1;\n");
            write(&dir.join("notes.txt"), "x");
            let unseen = detect(&manifest, strategy, &scope, &none());
            assert_eq!(unseen.structural, None, "{strategy:?}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn metadata_trusts_an_unchanged_stamp_and_content_does_not() {
        for strategy in [CacheStrategy::Metadata, CacheStrategy::Content] {
            let dir = scratch(&format!("trust-{}", strategy.as_str()));
            let scope = scope(&dir);
            write(&dir.join("a.ts"), "one\n");
            let manifest = recorded(&scope, strategy);
            // Same length, same modification time: metadata cannot see the edit.
            let path = dir.join("a.ts");
            let before = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            write(&path, "two\n");
            if let Some(time) = before {
                let _ = std::fs::File::options()
                    .write(true)
                    .open(&path)
                    .and_then(|f| f.set_modified(time));
            }
            let found = detect(&manifest, strategy, &scope, &none());
            assert_eq!(
                found.modified.is_empty(),
                strategy == CacheStrategy::Metadata,
                "{strategy:?}"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn a_touched_file_with_the_same_bytes_is_not_modified() {
        let dir = scratch("touch");
        let scope = scope(&dir);
        write(&dir.join("a.ts"), "same\n");
        let manifest = recorded(&scope, CacheStrategy::Metadata);
        let path = dir.join("a.ts");
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        let _ = std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|f| f.set_modified(later));
        let found = detect(&manifest, CacheStrategy::Metadata, &scope, &none());
        assert!(found.is_empty(), "{found:?}");
        assert_ne!(found.stamps, manifest.stamps, "the new stamp is reported");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_repository_that_appears_is_structural() {
        let dir = scratch("appears");
        let mut scope = scope(&dir);
        write(&dir.join("a.ts"), "x\n");
        let manifest = recorded(&scope, CacheStrategy::Metadata);
        scope.head = Some("0123".into());
        let found = detect(&manifest, CacheStrategy::Metadata, &scope, &none());
        assert!(
            found
                .structural
                .is_some_and(|r| r.contains("git repository"))
        );
        let recorded_in_git = Manifest {
            head: Some("0123".into()),
            ..manifest
        };
        scope.head = None;
        let found = detect(&recorded_in_git, CacheStrategy::Metadata, &scope, &none());
        assert!(
            found
                .structural
                .is_some_and(|r| r.contains("git repository"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_package_manifests_above_each_file_are_found_once() {
        let dir = scratch("packages");
        let scope = scope(&dir);
        for file in [
            "package.json",
            "apps/web/package.json",
            "apps/web/src/a.ts",
            "apps/web/src/b.ts",
            "libs/x/y.ts",
        ] {
            write(&dir.join(file), "{}");
        }
        let found: Vec<String> = package_manifests(
            &scope,
            ["apps/web/src/a.ts", "apps/web/src/b.ts", "libs/x/y.ts"].into_iter(),
        )
        .into_iter()
        .collect();
        assert_eq!(found, ["apps/web/package.json", "package.json"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_input_is_recorded_unsettled_and_an_absent_optional_one_absent() {
        let dir = scratch("record");
        let scope = scope(&dir);
        write(&dir.join("a.ts"), "x\n");
        let started = now();
        let inputs = BTreeSet::from(["a.ts".to_owned(), "gone.ts".to_owned()]);
        let optional = BTreeSet::from(["tsconfig.json".to_owned(), "a.ts".to_owned()]);
        let known = BTreeMap::from([("a.ts".to_owned(), "sha256:known".to_owned())]);
        let (hashes, stamps) = record(
            &scope,
            &inputs,
            &optional,
            &known,
            CacheStrategy::Metadata,
            started,
        );
        assert_eq!(
            hashes,
            BTreeMap::from([
                ("a.ts".to_owned(), "sha256:known".to_owned()),
                ("gone.ts".to_owned(), UNSETTLED.to_owned()),
                ("tsconfig.json".to_owned(), ABSENT.to_owned()),
            ]),
            "a known digest is not recomputed; an input that is also optional is required"
        );
        assert_eq!(stamps.keys().collect::<Vec<_>>(), ["a.ts"]);
        let (_, unstamped) = record(
            &scope,
            &inputs,
            &optional,
            &BTreeMap::new(),
            CacheStrategy::Content,
            started,
        );
        assert!(unstamped.is_empty(), "content keeps no stamps");
        assert_eq!(hash_file(&dir.join("gone.ts")), None);
        assert_eq!(stamp(&dir), None, "a folder has no file stamp");
        // The next run: the unreadable input is a deletion, the absent optional one is not.
        let manifest = Manifest {
            inputs: hashes,
            stamps,
            ..recorded(&scope, CacheStrategy::Metadata)
        };
        let found = detect(&manifest, CacheStrategy::Metadata, &scope, &none());
        assert_eq!(found.structural.as_deref(), Some("gone.ts was deleted"));
        write(&dir.join("gone.ts"), "back\n");
        let found = detect(&manifest, CacheStrategy::Metadata, &scope, &none());
        assert_eq!(found.structural, None);
        assert!(
            found.modified.contains("gone.ts"),
            "back, it is read again: {found:?}"
        );
        write(&dir.join("tsconfig.json"), "{}");
        let found = detect(&manifest, CacheStrategy::Metadata, &scope, &none());
        assert_eq!(found.structural.as_deref(), Some("tsconfig.json appeared"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_input_modified_after_the_run_started_is_recorded_unsettled() {
        for strategy in [CacheStrategy::Metadata, CacheStrategy::Content] {
            let dir = scratch(&format!("unsettled-{}", strategy.as_str()));
            let scope = scope(&dir);
            write(&dir.join("a.ts"), "before\n");
            write(&dir.join("b.ts"), "steady\n");
            // Settled before the run starts.
            let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
            for file in ["a.ts", "b.ts"] {
                let _ = std::fs::File::options()
                    .write(true)
                    .open(dir.join(file))
                    .and_then(|f| f.set_modified(old));
            }
            let started = now();
            let known = BTreeMap::from([(
                "a.ts".to_owned(),
                hash_file(&dir.join("a.ts")).unwrap_or_default(),
            )]);
            // The edit lands while the run extracts: after it started, before it records.
            write(&dir.join("a.ts"), "after!\n");
            let inputs = BTreeSet::from(["a.ts".to_owned(), "b.ts".to_owned()]);
            let (hashes, stamps) =
                record(&scope, &inputs, &BTreeSet::new(), &known, strategy, started);
            assert_eq!(hashes.get("a.ts").map(String::as_str), Some(UNSETTLED));
            assert_ne!(hashes.get("b.ts").map(String::as_str), Some(UNSETTLED));
            let manifest = Manifest {
                inputs: hashes,
                stamps,
                ..recorded(&scope, strategy)
            };
            // Nothing moves after the record, and still the next run reads the file again.
            let found = detect(&manifest, strategy, &scope, &none());
            assert!(found.modified.contains("a.ts"), "{strategy:?}: {found:?}");
            assert!(!found.modified.contains("b.ts"));
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn a_whole_second_time_counts_as_after_a_start_less_than_two_seconds_later() {
        const SECOND: u64 = 1_000_000_000;
        assert!(after_start(10 * SECOND + 1, 10 * SECOND));
        assert!(!after_start(10 * SECOND - 1, 10 * SECOND));
        assert!(!after_start(10 * SECOND + 1, 10 * SECOND + 1));
        assert!(
            after_start(10 * SECOND, 11 * SECOND),
            "a coarse time a second before"
        );
        assert!(
            !after_start(10 * SECOND, 12 * SECOND),
            "two seconds before is settled"
        );
    }

    #[test]
    fn a_folder_listing_sees_any_entry_come_or_go() {
        for strategy in [CacheStrategy::Metadata, CacheStrategy::Content] {
            let dir = scratch(&format!("listing-{}", strategy.as_str()));
            let scope = scope(&dir);
            write(
                &dir.join("web/src/a.ts"),
                "import { x } from '../../shared/util';\n",
            );
            write(
                &dir.join("web/src/b.ts"),
                "import { y } from './missing/deep';\n",
            );
            std::fs::create_dir_all(dir.join("shared")).unwrap_or_default();
            let folders = target_folders(
                &scope,
                [
                    ("web/src/a.ts", "../../shared/util"),
                    ("web/src/b.ts", "./missing/deep"),
                    ("web/src/b.ts", "react"),
                ]
                .into_iter(),
            );
            assert_eq!(
                folders.iter().collect::<Vec<_>>(),
                ["shared/", "web/src/"],
                "the target's folder, or its nearest existing one; a package is not relative"
            );
            let manifest = Manifest {
                inputs: record(
                    &scope,
                    &folders.union(&walk(&scope, &dir)).cloned().collect(),
                    &BTreeSet::new(),
                    &BTreeMap::new(),
                    strategy,
                    now(),
                )
                .0,
                ..recorded(&scope, strategy)
            };
            let quiet = detect(&manifest, strategy, &scope, &none());
            assert_eq!(quiet.structural, None, "{strategy:?}");
            // A file no walk would call relevant still changes the listing.
            write(&dir.join("shared/util"), "");
            let found = detect(&manifest, strategy, &scope, &none());
            assert!(
                found.modified.contains("shared/"),
                "{strategy:?}: {found:?}"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
        assert!(is_listing("a/") && !is_listing("a"));
        assert_eq!(listing(Path::new("/definitely/not/here")), None);
    }

    #[test]
    fn the_walk_widens_to_hold_an_input_outside_the_root() {
        let dir = scratch("widen");
        let web = dir.join("web");
        write(&web.join("src/a.ts"), "");
        write(&dir.join("shared/util.ts"), "");
        let scope = Scope {
            base: web.clone(),
            root: web.clone(),
            ..scope(&dir)
        };
        let outside = key::slashed(&dir.join("shared/util.ts"));
        assert_eq!(walk_root(&scope, [outside.clone()].iter()), dir);
        assert_eq!(walk_root(&scope, ["src/a.ts".to_owned()].iter()), web);
        assert_eq!(
            walk_root(&scope, ["/elsewhere/entirely/x.ts".to_owned()].iter()),
            web,
            "never the file system's root"
        );
        // A file created beside the outside input is seen by the content walk.
        let manifest = Manifest {
            inputs: record(
                &scope,
                &BTreeSet::from(["src/a.ts".to_owned(), outside]),
                &BTreeSet::new(),
                &BTreeMap::new(),
                CacheStrategy::Content,
                now(),
            )
            .0,
            ..recorded(&scope, CacheStrategy::Content)
        };
        assert_eq!(
            detect(&manifest, CacheStrategy::Content, &scope, &none()).structural,
            None
        );
        write(&dir.join("shared/more.ts"), "");
        let found = detect(&manifest, CacheStrategy::Content, &scope, &none());
        assert!(
            found
                .structural
                .as_ref()
                .is_some_and(|r| r.ends_with("shared/more.ts was added")),
            "{found:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_probe_that_differs_is_structural() {
        let dir = scratch("probes");
        let scope = scope(&dir);
        let manifest = Manifest {
            probes: BTreeMap::from([("Python environment".to_owned(), "none".to_owned())]),
            ..recorded(&scope, CacheStrategy::Metadata)
        };
        let same = BTreeMap::from([("Python environment".to_owned(), "none".to_owned())]);
        assert!(detect(&manifest, CacheStrategy::Metadata, &scope, &same).is_empty());
        let installed = BTreeMap::from([(
            "Python environment".to_owned(),
            ".venv/lib/python3.12/site-packages\nrequests-2.31.0.dist-info".to_owned(),
        )]);
        assert_eq!(
            detect(&manifest, CacheStrategy::Metadata, &scope, &installed)
                .structural
                .as_deref(),
            Some("the Python environment changed")
        );
        assert_eq!(
            detect(&manifest, CacheStrategy::Metadata, &scope, &none())
                .structural
                .as_deref(),
            Some("the Python environment changed"),
            "a probe no longer computed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
