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
//! An entry records its inputs: every file an extractor read (the parts' per-file states), the
//! manifests the extractors read (the cache key's list, and every `package.json` above a
//! TypeScript file), every assembly and PDB beside the analysed ones, and the *presence set*:
//! the files whose appearance or disappearance could change what an unchanged file resolves to
//! (a file with an extension an extractor reads or resolves to, or a manifest), which the
//! `content` strategy lists by walking the working directory and the `metadata` strategy takes
//! from git's untracked and added files. Paths are relative to the working directory, or absolute
//! outside it; `.git`, `.graph`, `node_modules` and the cache folder are never listed.
//!
//! [`detect`] answers with the recorded inputs whose content changed, and a *structural* reason
//! when the change is one reuse cannot follow (a file added or deleted, a manifest changed, git
//! unable to say what changed); the caller then extracts in full.
//!
//! | Strategy | Deleted | Added | Content |
//! | --- | --- | --- | --- |
//! | `metadata`, in a repository | a recorded input missing, or git listing a relevant file deleted since the recorded `HEAD` | git listing a relevant file added or untracked that is not recorded | hashed only when its size or modification time changed, or git lists it modified since the recorded `HEAD`; a manifest git lists modified is structural even when not recorded |
//! | `metadata`, outside a repository | a recorded input missing | the walk finding a relevant file not recorded | hashed only when its size or modification time changed |
//! | `content` | a recorded input missing | the walk finding a relevant file not recorded | every input hashed |
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
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
    Some((meta.len(), modified))
}

/// Every relevant file under the working directory, by recorded name, sorted.
pub fn walk(scope: &Scope) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut folders = vec![scope.base.clone()];
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
                folders.push(path);
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

/// Compares the entry `manifest` records with the files as they are now, by `strategy`.
pub fn detect(manifest: &Manifest, strategy: CacheStrategy, scope: &Scope) -> Changes {
    let mut changes = Changes::default();
    let current: Vec<Observed<'_>> = manifest
        .inputs
        .par_iter()
        .map(|(name, hash)| (name, hash, stamp(&scope.on_disk(name))))
        .collect();
    let mut candidates: BTreeSet<String> = BTreeSet::new();
    for (name, hash, now) in &current {
        let Some(now) = now else {
            changes.structural(format!("{name} was deleted"));
            continue;
        };
        changes.stamps.insert((*name).clone(), *now);
        let trusted =
            strategy == CacheStrategy::Metadata && manifest.stamps.get(*name) == Some(now);
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
        (CacheStrategy::Metadata, None, None) | (CacheStrategy::Content, _, _) => Some(walk(scope)),
    };
    if let Some(present) = presence {
        for name in present.difference(&manifest.inputs.keys().cloned().collect()) {
            changes.structural(format!("{name} was added"));
        }
    }
    let hashed: Vec<(String, Option<String>)> = candidates
        .into_par_iter()
        .map(|name| {
            let hash = hash_file(&scope.on_disk(&name));
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

/// The presence set an entry written now records, by `strategy`.
pub fn presence(strategy: CacheStrategy, scope: &Scope) -> BTreeSet<String> {
    match (strategy, &scope.head) {
        (CacheStrategy::Metadata, Some(_)) => git_presence(scope).unwrap_or_else(|| walk(scope)),
        (CacheStrategy::Metadata, None) | (CacheStrategy::Content, _) => walk(scope),
    }
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

/// The digests and stamps of `inputs`: taken from `known` (what [`detect`] verified this run)
/// where it has them, hashed otherwise; an input that cannot be read is left out, so the next
/// run finds it missing and extracts in full.
pub fn record(
    scope: &Scope,
    inputs: &BTreeSet<String>,
    known: &BTreeMap<String, String>,
    strategy: CacheStrategy,
) -> (BTreeMap<String, String>, BTreeMap<String, (u64, u64)>) {
    let recorded: Vec<Recorded> = inputs
        .par_iter()
        .map(|name| {
            let path = scope.on_disk(name);
            let now = stamp(&path);
            let hash = known.get(name).cloned().or_else(|| hash_file(&path));
            (name.clone(), hash, now)
        })
        .collect();
    let mut hashes = BTreeMap::new();
    let mut stamps = BTreeMap::new();
    for (name, hash, now) in recorded {
        let (Some(hash), Some(now)) = (hash, now) else {
            continue;
        };
        if strategy == CacheStrategy::Metadata {
            stamps.insert(name.clone(), now);
        }
        hashes.insert(name, hash);
    }
    (hashes, stamps)
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
        let found: Vec<String> = walk(&scope(&dir)).into_iter().collect();
        assert_eq!(found, ["docs/n.md", "package.json", "src/a.ts"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn recorded(scope: &Scope, strategy: CacheStrategy) -> Manifest {
        let inputs: BTreeSet<String> = walk(scope);
        let (hashes, stamps) = record(scope, &inputs, &BTreeMap::new(), strategy);
        Manifest {
            tool_version: "v".into(),
            config_hash: "sha256:c".into(),
            worktree: key::slashed(&scope.root),
            head: None,
            strategy,
            inputs: hashes,
            stamps,
            extraction: String::new(),
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
            let quiet = detect(&manifest, strategy, &scope);
            assert!(quiet.is_empty(), "{strategy:?} {quiet:?}");
            assert_eq!(quiet.hashes, manifest.inputs);
            // A longer file: a new size for metadata, a new digest for content.
            write(&dir.join("src/a.ts"), "export const a = 10;\n");
            let edited = detect(&manifest, strategy, &scope);
            assert_eq!(edited.structural, None);
            assert_eq!(edited.modified.iter().collect::<Vec<_>>(), ["src/a.ts"]);
            assert_ne!(
                edited.hashes.get("src/a.ts"),
                manifest.inputs.get("src/a.ts")
            );
            write(&dir.join("src/c.ts"), "export const c = 1;\n");
            let added = detect(&manifest, strategy, &scope);
            assert_eq!(added.structural.as_deref(), Some("src/c.ts was added"));
            let _ = std::fs::remove_file(dir.join("src/c.ts"));
            let _ = std::fs::remove_file(dir.join("src/b.ts"));
            let deleted = detect(&manifest, strategy, &scope);
            assert_eq!(deleted.structural.as_deref(), Some("src/b.ts was deleted"));
            // An irrelevant file comes and goes unseen.
            write(&dir.join("src/b.ts"), "export const b = 1;\n");
            write(&dir.join("notes.txt"), "x");
            let unseen = detect(&manifest, strategy, &scope);
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
            let found = detect(&manifest, strategy, &scope);
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
        let found = detect(&manifest, CacheStrategy::Metadata, &scope);
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
        let found = detect(&manifest, CacheStrategy::Metadata, &scope);
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
        let found = detect(&recorded_in_git, CacheStrategy::Metadata, &scope);
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
    fn an_unreadable_input_is_left_out_of_the_record() {
        let dir = scratch("record");
        let scope = scope(&dir);
        write(&dir.join("a.ts"), "x\n");
        let inputs = BTreeSet::from(["a.ts".to_owned(), "gone.ts".to_owned()]);
        let known = BTreeMap::from([("a.ts".to_owned(), "sha256:known".to_owned())]);
        let (hashes, stamps) = record(&scope, &inputs, &known, CacheStrategy::Metadata);
        assert_eq!(hashes, known, "a known digest is not recomputed");
        assert_eq!(stamps.keys().collect::<Vec<_>>(), ["a.ts"]);
        let (_, none) = record(&scope, &inputs, &BTreeMap::new(), CacheStrategy::Content);
        assert!(none.is_empty(), "content keeps no stamps");
        assert_eq!(hash_file(&dir.join("gone.ts")), None);
        assert_eq!(stamp(&dir), None, "a folder has no stamp");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
