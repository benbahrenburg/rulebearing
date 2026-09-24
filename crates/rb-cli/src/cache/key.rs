//! The cache key: which worktree, at which commit, under which configuration.
//!
//! - Plan: [Wave 2, Step 13](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#213-step-13-worktree-aware-cache-and-the-eslint-plugin-2g)
//!   (`cache/<sha256(worktree root, HEAD, config hash)[..16]>/`)
//! - Source: [design § The agentic engineering hat](../../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two)
//!   ("parallel agents in separate worktrees do not invalidate each other's caches or share
//!   stale graphs")
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Requirement: [FR-CLI-05](../../../../docs/prd.md#fr-cli-05)
//!
//! The worktree root comes from `git rev-parse --show-toplevel`, else the working directory.
//! `HEAD` is resolved from the files under the git directory without spawning git: a `.git`
//! directory or a `.git` file naming the worktree's git directory (`gitdir: ...`), a symbolic
//! `HEAD` followed through loose refs in the worktree's and the common directory (`commondir`),
//! then `packed-refs`. Only when those files cannot answer is `git rev-parse HEAD` asked; with no
//! git at all `HEAD` is empty, and the key still separates configurations and folders.

use std::path::{Path, PathBuf};
use std::process::Command;

use rb_config::Config;

use crate::cmd::attest::hash_files;

/// The folder under the base directory that holds every cache entry.
pub const CACHE_DIR: &str = ".graph/cache";

/// The three inputs of a cache entry's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheKey {
    /// The worktree root, `/`-separated.
    pub root: String,
    /// The commit `HEAD` names, or empty outside a repository or on an unborn branch.
    pub head: String,
    /// SHA-256 of the configuration.
    pub config_hash: String,
}

impl CacheKey {
    /// The key for a run in `cwd` under `config`.
    pub fn compute(cwd: &Path, config: &Config) -> Self {
        let root = worktree_root(cwd);
        Self {
            head: head(&root),
            config_hash: config_hash(config, &root),
            root: slashed(&root),
        }
    }

    /// SHA-256 over the three inputs, each length-prefixed, as lowercase hex.
    pub fn digest(&self) -> String {
        hash_files(
            [
                ("root".to_owned(), self.root.as_bytes()),
                ("head".to_owned(), self.head.as_bytes()),
                ("config".to_owned(), self.config_hash.as_bytes()),
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
}

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// The root of the worktree `cwd` is in, from git, else `cwd` itself; canonical when it exists.
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
    root.canonicalize().unwrap_or(root)
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

/// SHA-256 of the configuration: every file the load read, by path relative to the worktree
/// root, in path order; a configuration read from standard input hashes its canonical form.
pub fn config_hash(config: &Config, root: &Path) -> String {
    let mut files: Vec<(String, Vec<u8>)> = config
        .files
        .iter()
        .map(|p| {
            let name = p.strip_prefix(root).map_or_else(|_| slashed(p), slashed);
            (name, std::fs::read(p).unwrap_or_default())
        })
        .collect();
    if files.is_empty() {
        let canonical = serde_json::to_vec(&config.canonical).unwrap_or_default();
        files.push(("canonical".to_owned(), canonical));
    }
    files.sort();
    hash_files(files.iter().map(|(n, b)| (n.clone(), b.as_slice())))
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
        ] {
            assert_ne!(other.name(), key.name(), "{other:?}");
        }
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
}
