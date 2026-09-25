//! The cache key: which worktree, at which commit, under which configuration, with which edits,
//! read by which build.
//!
//! - Plan: [Wave 2, Step 13](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#213-step-13-worktree-aware-cache-and-the-eslint-plugin-2g)
//!   (`cache/<sha256(worktree root, HEAD, config hash)[..16]>/`)
//! - Source: [design § The agentic engineering hat](../../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two)
//!   ("parallel agents in separate worktrees do not invalidate each other's caches or share
//!   stale graphs")
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Requirement: [FR-CLI-05](../../../../docs/prd.md#fr-cli-05)
//!
//! The plan names three inputs: the worktree root, `HEAD` and the configuration's hash. Those
//! three alone answer from a stale graph whenever the files differ from the commit (an agent's
//! uncommitted edit is exactly the case the cache serves), and across an upgrade of the binary,
//! so this key extends the plan's with two more:
//!
//! | Input | What it is |
//! | --- | --- |
//! | `root` | `git rev-parse --show-toplevel`, else the working directory; canonical, without a Windows verbatim (`\\?\`) prefix |
//! | `head` | the commit `HEAD` names, read from the git files (below), else empty |
//! | `configHash` | SHA-256 of every file the configuration load read |
//! | `version` | the crate version of this build, so an upgrade never reads an older build's graph |
//! | `inputs` | the working-tree fingerprint: SHA-256 over what differs from `HEAD` and the manifests the extractors read |
//!
//! The fingerprint, inside a repository, is `git status --porcelain -z --untracked-files=all`
//! (no network, one process), each listed path with the bytes of the file when it exists, so a
//! modified, added, deleted or untracked file changes the key; entries under `.graph/` (the
//! cache itself and saved results) are left out. Outside a repository it is every file under the
//! root but `.git` and `.graph`, by path, size and modification time. Both add the bytes of the
//! manifests the extractors read even when git ignores them: `package.json`, `tsconfig.json`,
//! `jsconfig.json`, the lock files, `pyproject.toml`, `setup.cfg`, `setup.py`, `global.json`,
//! `Directory.Build.props`, `Directory.Packages.props` and every `.csproj`, `.sln` and `.slnx`
//! in the root and the working directory, and the files `options.tsConfig`, `webpackConfig` and
//! `babelConfig` name. A file git ignores that is not one of these (a package under
//! `node_modules/` whose lock file did not change) does not change the key.
//!
//! The worktree root comes from `git rev-parse --show-toplevel`, else the working directory.
//! `HEAD` is resolved from the files under the git directory without spawning git: a `.git`
//! directory or a `.git` file naming the worktree's git directory (`gitdir: ...`), a symbolic
//! `HEAD` followed through loose refs in the worktree's and the common directory (`commondir`),
//! then `packed-refs`. Only when those files cannot answer is `git rev-parse HEAD` asked; with no
//! git at all `HEAD` is empty, and the key still separates configurations, folders and edits.

use std::path::{Path, PathBuf};
use std::process::Command;

use rb_config::Config;
use serde_json::Value;

use crate::cmd::attest::hash_files;

/// The folder under the base directory that holds every cache entry.
pub const CACHE_DIR: &str = ".graph/cache";

/// The version of this build, one of the key's inputs.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The manifests the extractors read, by file name, hashed whether or not git tracks them.
const MANIFESTS: &[&str] = &[
    "package.json",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "yarn.lock",
    "bun.lockb",
    "tsconfig.json",
    "jsconfig.json",
    "pyproject.toml",
    "setup.cfg",
    "setup.py",
    "global.json",
    "Directory.Build.props",
    "Directory.Packages.props",
];

/// The manifests the extractors read, by extension.
const MANIFEST_EXTENSIONS: &[&str] = &["csproj", "sln", "slnx"];

/// The inputs of a cache entry's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheKey {
    /// The worktree root, `/`-separated.
    pub root: String,
    /// The commit `HEAD` names, or empty outside a repository or on an unborn branch.
    pub head: String,
    /// SHA-256 of the configuration.
    pub config_hash: String,
    /// The configuration files, relative to the root when under it, `/`-separated, sorted: what
    /// `config_hash` hashes, so a reader can check it without loading the configuration.
    pub config_files: Vec<String>,
    /// The version of the build that wrote the entry.
    pub version: String,
    /// The working-tree fingerprint ([`inputs`]).
    pub inputs: String,
}

impl CacheKey {
    /// The key for a run in `cwd` under `config`.
    pub fn compute(cwd: &Path, config: &Config) -> Self {
        let root = worktree_root(cwd);
        Self {
            head: head(&root),
            config_hash: config_hash(config, &root),
            config_files: config_files(config, &root),
            version: VERSION.to_owned(),
            inputs: inputs(&root, cwd, config),
            root: slashed(&root),
        }
    }

    /// SHA-256 over the inputs, each length-prefixed, as lowercase hex.
    pub fn digest(&self) -> String {
        hash_files(
            [
                ("root".to_owned(), self.root.as_bytes()),
                ("head".to_owned(), self.head.as_bytes()),
                ("config".to_owned(), self.config_hash.as_bytes()),
                ("version".to_owned(), self.version.as_bytes()),
                ("inputs".to_owned(), self.inputs.as_bytes()),
            ]
            .into_iter(),
        )
    }

    /// The entry's folder name: the first sixteen hex digits of [`Self::digest`].
    pub fn name(&self) -> String {
        self.digest().chars().take(16).collect()
    }

    /// The entry's folder under `base` (the working directory).
    pub fn directory(&self, base: &Path) -> PathBuf {
        base.join(CACHE_DIR).join(self.name())
    }

    /// The inputs as `key.json` records them, for a reader such as `eslint-plugin-rulebearing`.
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "root": self.root,
            "head": self.head,
            "configHash": self.config_hash,
            "configFiles": self.config_files,
            "version": self.version,
            "inputs": self.inputs,
        })
    }
}

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// A Windows path without the verbatim prefix `canonicalize` gives it (`\\?\C:\x` is `C:\x`,
/// `\\?\UNC\server\share` is `\\server\share`), so it compares equal to the path Node's
/// `realpathSync` gives. Any other path is returned as it is.
pub fn strip_verbatim(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = path.strip_prefix("//?/UNC/") {
        return format!("//{rest}");
    }
    path.strip_prefix(r"\\?\")
        .or_else(|| path.strip_prefix("//?/"))
        .map_or_else(|| path.to_owned(), str::to_owned)
}

/// The root of the worktree `cwd` is in, from git, else `cwd` itself; canonical when it exists,
/// without a verbatim prefix.
pub fn worktree_root(cwd: &Path) -> PathBuf {
    let from_git = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let root = from_git.unwrap_or_else(|| cwd.to_path_buf());
    let canonical = root.canonicalize().unwrap_or(root);
    PathBuf::from(strip_verbatim(&canonical.to_string_lossy()))
}

/// The commit `HEAD` names in the worktree at `root`: from the files when they answer, else from
/// `git rev-parse HEAD`, else empty.
pub fn head(root: &Path) -> String {
    head_from_files(root).unwrap_or_else(|| {
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_default()
    })
}

/// The git directory of the worktree at `root`: `.git` itself, or the folder a `.git` file names.
pub fn git_dir(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let text = std::fs::read_to_string(&dot_git).ok()?;
    let named = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    let path = Path::new(named);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    })
}

/// The folder shared by every worktree: what `commondir` names, else the git directory.
fn common_dir(git_dir: &Path) -> PathBuf {
    std::fs::read_to_string(git_dir.join("commondir"))
        .ok()
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .map_or_else(
            || git_dir.to_path_buf(),
            |t| {
                let path = Path::new(&t);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    git_dir.join(path)
                }
            },
        )
}

fn is_object_name(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A ref's commit from `packed-refs`.
fn packed(common: &Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(common.join("packed-refs")).ok()?;
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with('^'))
        .find_map(|l| {
            let (object, reference) = l.split_once(' ')?;
            (reference.trim() == name && is_object_name(object)).then(|| object.to_owned())
        })
}

/// `HEAD` from the files alone: at most five symbolic hops, loose refs before packed ones.
pub fn head_from_files(root: &Path) -> Option<String> {
    let git = git_dir(root)?;
    let common = common_dir(&git);
    let mut content = std::fs::read_to_string(git.join("HEAD")).ok()?;
    for _ in 0..5 {
        let text = content.trim();
        if is_object_name(text) {
            return Some(text.to_owned());
        }
        let name = text.strip_prefix("ref:")?.trim().to_owned();
        let loose = [git.join(&name), common.join(&name)]
            .into_iter()
            .find_map(|p| std::fs::read_to_string(p).ok());
        match loose {
            Some(next) => content = next,
            None => return packed(&common, &name),
        }
    }
    None
}

/// A configuration file's name in the key: relative to the worktree root when under it.
fn config_name(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .map_or_else(|_| slashed(path), slashed)
}

/// The configuration files [`config_hash`] reads, by the names it hashes them under, sorted;
/// empty for a configuration read from standard input.
pub fn config_files(config: &Config, root: &Path) -> Vec<String> {
    let mut names: Vec<String> = config.files.iter().map(|p| config_name(p, root)).collect();
    names.sort();
    names
}

/// SHA-256 of the configuration: every file the load read, by path relative to the worktree
/// root, in path order; a configuration read from standard input hashes its canonical form.
pub fn config_hash(config: &Config, root: &Path) -> String {
    let mut files: Vec<(String, Vec<u8>)> = config
        .files
        .iter()
        .map(|p| (config_name(p, root), std::fs::read(p).unwrap_or_default()))
        .collect();
    if files.is_empty() {
        let canonical = serde_json::to_vec(&config.canonical).unwrap_or_default();
        files.push(("canonical".to_owned(), canonical));
    }
    files.sort();
    hash_files(files.iter().map(|(n, b)| (n.clone(), b.as_slice())))
}

/// Whether a path, relative to the root, is inside a `.graph` folder: the cache and saved
/// results, which the fingerprint leaves out so writing an entry does not change its own key.
fn under_graph(path: &str) -> bool {
    path.split('/').any(|part| part == ".graph")
}

/// The working-tree fingerprint: what differs from `HEAD` (or, outside a repository, every
/// file), and the manifests and configuration-named files the extractors read.
pub fn inputs(root: &Path, cwd: &Path, config: &Config) -> String {
    let mut parts: Vec<(String, Vec<u8>)> = git_status(root)
        .unwrap_or_else(|| walk(root))
        .into_iter()
        .map(|(n, b)| (format!("tree:{n}"), b))
        .collect();
    parts.extend(
        manifests(root, cwd, config)
            .into_iter()
            .map(|(n, b)| (format!("manifest:{n}"), b)),
    );
    parts.sort();
    parts.dedup();
    hash_files(parts.iter().map(|(n, b)| (n.clone(), b.as_slice())))
}

/// Each entry of `git status --porcelain -z --untracked-files=all` in `root`, with the bytes of
/// the file it names when that file exists; `None` when git cannot answer (no repository).
pub fn git_status(root: &Path) -> Option<Vec<(String, Vec<u8>)>> {
    let output = Command::new("git")
        .args([
            "-c",
            "core.quotepath=off",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
            "--ignore-submodules=none",
        ])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(parse_status(root, &output.stdout))
}

/// The entries of `git status --porcelain=v1 -z` output: `XY path`, each with the bytes of
/// `root/path` when it is a file. Entries under `.graph/` are left out.
fn parse_status(root: &Path, stdout: &[u8]) -> Vec<(String, Vec<u8>)> {
    stdout
        .split(|b| *b == 0)
        .filter(|e| e.len() > 3)
        .map(|e| String::from_utf8_lossy(e).into_owned())
        .filter(|e| e.get(3..).is_some_and(|p| !under_graph(p)))
        .map(|entry| {
            let path = entry.get(3..).unwrap_or_default();
            let bytes = std::fs::read(root.join(path)).unwrap_or_default();
            (entry, bytes)
        })
        .collect()
}

/// Every file under `root` but `.git` and `.graph`, by relative path, with its size and
/// modification time as the bytes: the fingerprint outside a repository.
pub fn walk(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name == ".git" || name == ".graph" {
                continue;
            }
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                folders.push(path);
            } else if let Ok(meta) = entry.metadata() {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                let mut stamp = meta.len().to_be_bytes().to_vec();
                stamp.extend_from_slice(&modified.to_be_bytes());
                out.push((config_name(&path, root), stamp));
            }
        }
    }
    out.sort();
    out
}

/// The manifests in `root` and `cwd`, and the files the configuration's `tsConfig`,
/// `webpackConfig` and `babelConfig` options name, each with its bytes (empty when absent, so
/// creating one changes the key too).
fn manifests(root: &Path, cwd: &Path, config: &Config) -> Vec<(String, Vec<u8>)> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for folder in [root, cwd] {
        paths.extend(MANIFESTS.iter().map(|n| folder.join(n)));
        if let Ok(entries) = std::fs::read_dir(folder) {
            paths.extend(entries.flatten().map(|e| e.path()).filter(|p| {
                p.extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| MANIFEST_EXTENSIONS.contains(&x))
            }));
        }
    }
    for option in ["tsConfig", "webpackConfig", "babelConfig"] {
        if let Some(file) = config
            .canonical
            .get("options")
            .and_then(|o| o.get(option))
            .and_then(|o| o.get("fileName"))
            .and_then(Value::as_str)
        {
            paths.push(cwd.join(file));
        }
    }
    let mut out: Vec<(String, Vec<u8>)> = paths
        .iter()
        .map(|p| (config_name(p, root), std::fs::read(p).unwrap_or_default()))
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0123456789abcdef0123456789abcdef01234567";
    const B: &str = "89abcdef0123456789abcdef0123456789abcdef";

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rb-cache-key-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, text);
    }

    #[test]
    fn head_resolves_detached_loose_packed_and_worktree_files() {
        let dir = scratch("head");
        write(&dir.join(".git/HEAD"), &format!("{A}\n"));
        assert_eq!(head_from_files(&dir).as_deref(), Some(A), "detached");

        write(&dir.join(".git/HEAD"), "ref: refs/heads/main\n");
        write(&dir.join(".git/refs/heads/main"), &format!("{B}\n"));
        assert_eq!(head_from_files(&dir).as_deref(), Some(B), "loose");

        let _ = std::fs::remove_file(dir.join(".git/refs/heads/main"));
        write(
            &dir.join(".git/packed-refs"),
            &format!("# pack-refs with: peeled\n{A} refs/heads/other\n^{B}\n{B} refs/heads/main\n"),
        );
        assert_eq!(head_from_files(&dir).as_deref(), Some(B), "packed");

        // A linked worktree: `.git` is a file, HEAD is its own, refs are the common directory's.
        let linked = dir.join("linked");
        write(&linked.join(".git"), "gitdir: ../.git/worktrees/linked\n");
        write(
            &dir.join(".git/worktrees/linked/HEAD"),
            "ref: refs/heads/feature\n",
        );
        write(&dir.join(".git/worktrees/linked/commondir"), "../..\n");
        write(&dir.join(".git/refs/heads/feature"), &format!("{A}\n"));
        assert_eq!(head_from_files(&linked).as_deref(), Some(A), "worktree");

        write(&dir.join(".git/HEAD"), "ref: refs/heads/unborn\n");
        assert_eq!(head_from_files(&dir), None, "an unborn branch");
        write(&dir.join(".git/HEAD"), "garbage\n");
        assert_eq!(head_from_files(&dir), None);
        assert_eq!(head_from_files(&dir.join("nowhere")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn symbolic_refs_stop_after_five_hops() {
        let dir = scratch("loop");
        write(&dir.join(".git/HEAD"), "ref: refs/heads/a\n");
        write(&dir.join(".git/refs/heads/a"), "ref: refs/heads/a\n");
        assert_eq!(head_from_files(&dir), None);
        let absolute = scratch("absolute");
        write(
            &absolute.join(".git"),
            &format!("gitdir: {}\n", dir.join(".git").display()),
        );
        write(&dir.join(".git/HEAD"), &format!("{B}\n"));
        assert_eq!(head_from_files(&absolute).as_deref(), Some(B));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&absolute);
    }

    #[test]
    fn each_input_changes_the_name_and_the_name_is_sixteen_hex_digits() {
        let key = CacheKey {
            root: "/repo".into(),
            head: A.into(),
            config_hash: "c".into(),
            config_files: vec!["rulebearing.yaml".into()],
            version: VERSION.into(),
            inputs: "i".into(),
        };
        assert_eq!(key.name().len(), 16);
        assert!(key.name().bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(key.digest().starts_with(&key.name()));
        assert_eq!(key.name(), key.clone().name(), "deterministic");
        for other in [
            CacheKey {
                root: "/repo2".into(),
                ..key.clone()
            },
            CacheKey {
                head: B.into(),
                ..key.clone()
            },
            CacheKey {
                config_hash: "d".into(),
                ..key.clone()
            },
            CacheKey {
                version: "0.0.0-older".into(),
                ..key.clone()
            },
            CacheKey {
                inputs: "j".into(),
                ..key.clone()
            },
        ] {
            assert_ne!(other.name(), key.name(), "{other:?}");
        }
        let json = key.to_json();
        assert_eq!(json["root"], "/repo");
        assert_eq!(json["head"], A);
        assert_eq!(json["configHash"], "c");
        assert_eq!(json["configFiles"], serde_json::json!(["rulebearing.yaml"]));
        assert_eq!(json["version"], VERSION);
        assert_eq!(json["inputs"], "i");
        assert_eq!(
            key.directory(Path::new("/w")),
            Path::new("/w/.graph/cache").join(key.name())
        );
    }

    #[test]
    fn the_configuration_hash_follows_the_files_or_the_canonical_form() {
        let dir = scratch("config");
        let file = dir.join("rulebearing.yaml");
        write(&file, "forbidden: []\n");
        let config = Config {
            files: vec![file.clone()],
            ..Config::default()
        };
        let first = config_hash(&config, &dir);
        write(&file, "forbidden: [] # changed\n");
        assert_ne!(config_hash(&config, &dir), first);
        let piped = Config::default();
        assert_eq!(config_hash(&piped, &dir), config_hash(&piped, &dir));
        assert_ne!(config_hash(&piped, &dir), first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn outside_a_repository_the_root_is_the_folder() {
        let dir = scratch("plain");
        let key = CacheKey::compute(&dir, &Config::default());
        // The temporary folder may sit inside a repository on a developer's machine; either
        // way the root is a folder that contains `dir`.
        let canonical = dir.canonicalize().unwrap_or_else(|_| dir.clone());
        assert!(slashed(&canonical).starts_with(&key.root), "{key:?}");
        assert!(!is_object_name("xyz"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_verbatim_windows_prefix_is_stripped() {
        for (given, plain) in [
            (r"\\?\C:\repo", r"C:\repo"),
            ("//?/C:/repo", "C:/repo"),
            (r"\\?\UNC\server\share\repo", r"\\server\share\repo"),
            ("//?/UNC/server/share", "//server/share"),
            ("/home/me/repo", "/home/me/repo"),
            (r"C:\repo", r"C:\repo"),
            ("", ""),
        ] {
            assert_eq!(strip_verbatim(given), plain, "{given}");
        }
    }

    #[test]
    fn the_status_entries_carry_the_bytes_and_leave_out_the_cache() {
        let dir = scratch("status");
        write(&dir.join("src/a.ts"), "export const a = 1;\n");
        let stdout =
            b" M src/a.ts\0?? .graph/cache/x/graph.json\0 D src/gone.ts\0?? sub/.graph/y\0x\0";
        let entries = parse_status(&dir, stdout);
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, [" M src/a.ts", " D src/gone.ts"]);
        assert_eq!(entries[0].1, b"export const a = 1;\n");
        assert!(entries[1].1.is_empty(), "a deleted file has no bytes");
        let before = parse_status(&dir, stdout);
        write(&dir.join("src/a.ts"), "export const a = 2;\n");
        assert_ne!(
            parse_status(&dir, stdout),
            before,
            "an edit changes the entry"
        );
        assert!(under_graph(".graph/cache"));
        assert!(under_graph("a/.graph/b"));
        assert!(!under_graph("a/graph/b.graph"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn outside_a_repository_the_walk_sees_every_edit_but_the_cache() {
        let dir = scratch("walk");
        write(&dir.join("src/a.ts"), "export const a = 1;\n");
        write(&dir.join(".git/HEAD"), "ignored\n");
        let first = walk(&dir);
        let names: Vec<&str> = first.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["src/a.ts"]);
        write(&dir.join(".graph/cache/e/graph.json"), "{}");
        assert_eq!(walk(&dir), first, "writing an entry leaves the key alone");
        write(&dir.join("src/b.ts"), "export const b = 1;\n");
        assert_ne!(walk(&dir), first, "a new file changes it");
        write(&dir.join("src/a.ts"), "export const a = 10;\n");
        let _ = std::fs::remove_file(dir.join("src/b.ts"));
        assert_ne!(walk(&dir), first, "a longer file changes it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_manifests_count_whether_or_not_git_sees_them() {
        let dir = scratch("manifests");
        let config = Config {
            canonical: serde_json::from_value(serde_json::json!({
                "options": { "tsConfig": { "fileName": "config/tsconfig.app.json" } }
            }))
            .unwrap_or_default(),
            ..Config::default()
        };
        let first = inputs(&dir, &dir, &config);
        assert_eq!(first, inputs(&dir, &dir, &config), "deterministic");
        write(&dir.join("package.json"), "{}");
        let second = inputs(&dir, &dir, &config);
        assert_ne!(second, first, "a package.json appears");
        write(&dir.join("config/tsconfig.app.json"), "{}");
        let third = inputs(&dir, &dir, &config);
        assert_ne!(third, second, "the tsconfig the options name");
        write(&dir.join("App.csproj"), "<Project/>");
        assert_ne!(inputs(&dir, &dir, &config), third, "a project file");
        let names: Vec<String> = manifests(&dir, &dir, &config)
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert!(names.contains(&"App.csproj".to_owned()), "{names:?}");
        assert!(names.contains(&"config/tsconfig.app.json".to_owned()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_configuration_files_are_named_relative_to_the_root() {
        let config = Config {
            files: vec![
                PathBuf::from("/r/b.yaml"),
                PathBuf::from("/elsewhere/a.yaml"),
            ],
            ..Config::default()
        };
        assert_eq!(
            config_files(&config, Path::new("/r")),
            ["/elsewhere/a.yaml", "b.yaml"]
        );
        assert!(config_files(&Config::default(), Path::new("/r")).is_empty());
    }
}
