//! The files an extractor reads: the disk, or a tree of files held in memory.
//!
//! - Decision: [ADR-0073](../../../docs/adr/0073-extractors-read-files-through-rb-model.md)
//!   (every read an extractor makes goes through [`Files`]; the disk is the default and reads
//!   exactly as before), [ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md)
//!   (the extractors depend on `rb-model` only, so the seam they share lives here)
//! - Plan: [Wave 4, Step 2](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables)
//!   ("an in-memory file system adapter for `rb-extract-ts` and `rb-extract-python`")
//! - Requirement: [FR-REACH-01](../../../docs/prd.md#fr-reach-01) (the playground extracts a
//!   dropped small repository in the browser, which has no file system)
//!
//! [`Files::disk`] calls `std::fs` and nothing else, so a native run reads what it read before
//! this seam existed. [`Files::memory`] answers from a [`MemoryTree`]: absolute paths, files
//! only, every folder implied by the files below it, and no symbolic links, so a path's
//! canonical form is its lexical one ([`lexical`]).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// What a path is. The disk can say all three of a symbolic link (`symlink_metadata`); a
/// [`MemoryTree`] holds no links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileKind {
    file: bool,
    dir: bool,
    symlink: bool,
}

impl FileKind {
    /// A file.
    pub const FILE: Self = Self {
        file: true,
        dir: false,
        symlink: false,
    };
    /// A folder.
    pub const DIR: Self = Self {
        file: false,
        dir: true,
        symlink: false,
    };

    fn of(kind: std::fs::FileType) -> Self {
        Self {
            file: kind.is_file(),
            dir: kind.is_dir(),
            symlink: kind.is_symlink(),
        }
    }

    /// Whether it is a file.
    pub fn is_file(self) -> bool {
        self.file
    }

    /// Whether it is a folder.
    pub fn is_dir(self) -> bool {
        self.dir
    }

    /// Whether it is a symbolic link (only from [`Files::symlink_metadata`] or
    /// [`DirEntry::file_type`]).
    pub fn is_symlink(self) -> bool {
        self.symlink
    }
}

/// One entry of a folder, as [`Files::read_dir`] lists it.
#[derive(Debug)]
pub struct DirEntry(Entry);

#[derive(Debug)]
enum Entry {
    Disk(Box<std::fs::DirEntry>),
    Memory {
        name: OsString,
        path: PathBuf,
        kind: FileKind,
    },
}

impl DirEntry {
    /// The entry's name in its folder.
    pub fn file_name(&self) -> OsString {
        match &self.0 {
            Entry::Disk(entry) => entry.file_name(),
            Entry::Memory { name, .. } => name.clone(),
        }
    }

    /// The folder's path joined with the entry's name.
    pub fn path(&self) -> PathBuf {
        match &self.0 {
            Entry::Disk(entry) => entry.path(),
            Entry::Memory { path, .. } => path.clone(),
        }
    }

    /// What the entry is, a symbolic link not followed.
    ///
    /// # Errors
    /// As `std::fs::DirEntry::file_type` on the disk; never in memory.
    pub fn file_type(&self) -> io::Result<FileKind> {
        match &self.0 {
            Entry::Disk(entry) => entry.file_type().map(FileKind::of),
            Entry::Memory { kind, .. } => Ok(*kind),
        }
    }
}

/// Why a path cannot join a [`MemoryTree`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TreeError {
    /// A path in the tree is absolute, so it is the same path wherever the tree is read.
    #[error("{path}: a path in the file tree must be absolute", path = path.display())]
    NotAbsolute {
        /// The path given.
        path: PathBuf,
    },
    /// A file and a folder cannot share a path.
    #[error("{path}: is both a file and a folder of the file tree", path = path.display())]
    FileAndFolder {
        /// The path both would have.
        path: PathBuf,
    },
}

/// Files held in memory, by absolute path. Every folder above a file exists; nothing else does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryTree {
    files: BTreeMap<PathBuf, Vec<u8>>,
    folders: BTreeSet<PathBuf>,
}

impl MemoryTree {
    /// An empty tree.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file at `path` (absolute; `.` and `..` are taken lexically), replacing one already
    /// there.
    ///
    /// # Errors
    /// [`TreeError`] when the path is relative, is a folder of the tree, or has a file of the
    /// tree above it.
    pub fn insert(&mut self, path: &Path, contents: impl Into<Vec<u8>>) -> Result<(), TreeError> {
        if !path.has_root() {
            return Err(TreeError::NotAbsolute {
                path: path.to_path_buf(),
            });
        }
        let path = lexical(path);
        let clash = |at: &Path| TreeError::FileAndFolder {
            path: at.to_path_buf(),
        };
        if self.folders.contains(&path) || path.parent().is_none() {
            return Err(clash(&path));
        }
        let above: Vec<PathBuf> = path.ancestors().skip(1).map(Path::to_path_buf).collect();
        if let Some(file) = above.iter().find(|p| self.files.contains_key(*p)) {
            return Err(clash(file));
        }
        self.folders.extend(above);
        self.files.insert(path, contents.into());
        Ok(())
    }

    /// How many files the tree holds.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether the tree holds no file.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Every file's path, in order.
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.files.keys().map(PathBuf::as_path)
    }

    fn kind(&self, path: &Path) -> Option<FileKind> {
        let path = lexical(path);
        if self.files.contains_key(&path) {
            Some(FileKind::FILE)
        } else if self.folders.contains(&path) {
            Some(FileKind::DIR)
        } else {
            None
        }
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let normal = lexical(path);
        match self.files.get(&normal) {
            Some(bytes) => Ok(bytes.clone()),
            None if self.folders.contains(&normal) => Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                format!("{} is a folder of the file tree", path.display()),
            )),
            None => Err(not_found(path)),
        }
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<io::Result<DirEntry>>> {
        let folder = lexical(path);
        if self.files.contains_key(&folder) {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!("{} is a file of the file tree", path.display()),
            ));
        }
        if !self.folders.contains(&folder) {
            return Err(not_found(path));
        }
        let children = |paths: Vec<&PathBuf>, kind: FileKind| {
            paths
                .into_iter()
                .filter(|p| p.parent() == Some(folder.as_path()))
                .filter_map(|p| {
                    let name = p.file_name()?.to_os_string();
                    Some((name, kind))
                })
                .collect::<Vec<_>>()
        };
        let mut entries = children(
            self.folders.range(folder.clone()..).collect(),
            FileKind::DIR,
        );
        entries.extend(children(
            self.files.range(folder.clone()..).map(|(p, _)| p).collect(),
            FileKind::FILE,
        ));
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(entries
            .into_iter()
            .map(|(name, kind)| {
                Ok(DirEntry(Entry::Memory {
                    path: path.join(&name),
                    name,
                    kind,
                }))
            })
            .collect())
    }
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("{} is not in the file tree", path.display()),
    )
}

/// `path` with `.` dropped and each `..` taking off the component before it, as a path with no
/// symbolic link resolves; a `..` at the root stays at the root, and one at the start of a
/// relative path is kept.
pub fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let last = out.components().next_back();
                match last {
                    Some(Component::Normal(_)) => {
                        out.pop();
                    }
                    Some(Component::RootDir | Component::Prefix(_)) => {}
                    Some(Component::ParentDir | Component::CurDir) | None => out.push(".."),
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Where an extractor's reads go: the disk ([`Files::disk`], the default) or a [`MemoryTree`]
/// ([`Files::memory`]). Cloning shares the tree.
#[derive(Debug, Clone, Default)]
pub struct Files {
    tree: Option<Arc<MemoryTree>>,
}

impl Files {
    /// The disk, through `std::fs`.
    pub fn disk() -> Self {
        Self::default()
    }

    /// The files of `tree`, and nothing else.
    pub fn memory(tree: MemoryTree) -> Self {
        Self {
            tree: Some(Arc::new(tree)),
        }
    }

    /// The tree, when these are files in memory.
    pub fn tree(&self) -> Option<&MemoryTree> {
        self.tree.as_deref()
    }

    /// Whether these are files in memory, where nothing outside the tree (a process, the real
    /// disk) can be reached.
    pub fn is_memory(&self) -> bool {
        self.tree.is_some()
    }

    /// `std::fs::read`.
    ///
    /// # Errors
    /// As `std::fs::read`; in memory, a path that is not a file of the tree.
    pub fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        match &self.tree {
            None => std::fs::read(path),
            Some(tree) => tree.read(path),
        }
    }

    /// `std::fs::read_to_string`.
    ///
    /// # Errors
    /// As [`Files::read`], and text that is not UTF-8.
    pub fn read_to_string(&self, path: &Path) -> io::Result<String> {
        match &self.tree {
            None => std::fs::read_to_string(path),
            Some(tree) => String::from_utf8(tree.read(path)?).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} is not valid UTF-8", path.display()),
                )
            }),
        }
    }

    /// `std::fs::metadata`: what `path` is, a symbolic link followed.
    ///
    /// # Errors
    /// As `std::fs::metadata`; in memory, a path the tree does not hold.
    pub fn metadata(&self, path: &Path) -> io::Result<FileKind> {
        match &self.tree {
            None => std::fs::metadata(path).map(|m| FileKind::of(m.file_type())),
            Some(tree) => tree.kind(path).ok_or_else(|| not_found(path)),
        }
    }

    /// `std::fs::symlink_metadata`: what `path` is, a symbolic link not followed.
    ///
    /// # Errors
    /// As [`Files::metadata`].
    pub fn symlink_metadata(&self, path: &Path) -> io::Result<FileKind> {
        match &self.tree {
            None => std::fs::symlink_metadata(path).map(|m| FileKind::of(m.file_type())),
            Some(tree) => tree.kind(path).ok_or_else(|| not_found(path)),
        }
    }

    /// `std::fs::read_dir`, collected: each entry, or the error reading it. In memory the
    /// entries are in name order; on the disk in the order the system gives.
    ///
    /// # Errors
    /// As `std::fs::read_dir`; in memory, a path that is not a folder of the tree.
    pub fn read_dir(&self, path: &Path) -> io::Result<Vec<io::Result<DirEntry>>> {
        match &self.tree {
            None => Ok(std::fs::read_dir(path)?
                .map(|entry| entry.map(|e| DirEntry(Entry::Disk(Box::new(e)))))
                .collect()),
            Some(tree) => tree.read_dir(path),
        }
    }

    /// `std::fs::canonicalize`; in memory, the [`lexical`] form of a path the tree holds.
    ///
    /// # Errors
    /// As `std::fs::canonicalize`; in memory, a path the tree does not hold.
    pub fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        match &self.tree {
            None => std::fs::canonicalize(path),
            Some(tree) => tree
                .kind(path)
                .map(|_| lexical(path))
                .ok_or_else(|| not_found(path)),
        }
    }

    /// `Path::is_file`: a file, a symbolic link followed; false on any error.
    pub fn is_file(&self, path: &Path) -> bool {
        self.metadata(path).is_ok_and(FileKind::is_file)
    }

    /// `Path::is_dir`: a folder, a symbolic link followed; false on any error.
    pub fn is_dir(&self, path: &Path) -> bool {
        self.metadata(path).is_ok_and(FileKind::is_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    fn tree() -> Result<Files> {
        let mut tree = MemoryTree::new();
        tree.insert(Path::new("/p/src/a.ts"), "import './b';")?;
        tree.insert(Path::new("/p/src/b.ts"), Vec::from(*b"export {};"))?;
        tree.insert(Path::new("/p/src/deep/c.ts"), "")?;
        tree.insert(Path::new("/p/package.json"), "{}")?;
        tree.insert(Path::new("/p/bad.txt"), vec![0xff, 0xfe])?;
        Ok(Files::memory(tree))
    }

    fn names(files: &Files, path: &str) -> Result<Vec<(String, bool)>> {
        let mut out = Vec::new();
        for entry in files.read_dir(Path::new(path))? {
            let entry = entry?;
            assert_eq!(
                entry.path(),
                Path::new(path).join(entry.file_name()),
                "an entry's path is its folder's joined with its name"
            );
            out.push((
                entry.file_name().to_string_lossy().into_owned(),
                entry.file_type()?.is_dir(),
            ));
        }
        Ok(out)
    }

    #[test]
    fn a_memory_tree_reads_its_files_and_lists_its_folders() -> Result {
        let files = tree()?;
        assert!(files.is_memory());
        assert_eq!(files.tree().map(MemoryTree::len), Some(5));
        assert_eq!(
            files.read_to_string(Path::new("/p/src/a.ts"))?,
            "import './b';"
        );
        assert_eq!(
            files.read(Path::new("/p/src/./deep/../b.ts"))?,
            b"export {};"
        );
        assert_eq!(
            names(&files, "/p")?,
            [
                ("bad.txt".to_owned(), false),
                ("package.json".to_owned(), false),
                ("src".to_owned(), true),
            ]
        );
        assert_eq!(
            names(&files, "/p/src")?,
            [
                ("a.ts".to_owned(), false),
                ("b.ts".to_owned(), false),
                ("deep".to_owned(), true),
            ]
        );
        assert_eq!(names(&files, "/")?, [("p".to_owned(), true)]);
        let paths: Vec<&Path> = files
            .tree()
            .into_iter()
            .flat_map(MemoryTree::paths)
            .collect();
        assert_eq!(paths.len(), 5);
        assert_eq!(paths.first().copied(), Some(Path::new("/p/bad.txt")));
        Ok(())
    }

    #[test]
    fn a_memory_tree_says_what_each_path_is() -> Result {
        let files = tree()?;
        for (path, file, dir) in [
            ("/p/src/a.ts", true, false),
            ("/p/src", false, true),
            ("/p/src/", false, true),
            ("/", false, true),
            ("/p/missing.ts", false, false),
            ("/p/src/a.ts/x", false, false),
        ] {
            let path = Path::new(path);
            assert_eq!(files.is_file(path), file, "{}", path.display());
            assert_eq!(files.is_dir(path), dir, "{}", path.display());
            for kind in [files.metadata(path), files.symlink_metadata(path)] {
                match kind {
                    Ok(kind) => {
                        assert_eq!((kind.is_file(), kind.is_dir()), (file, dir));
                        assert!(!kind.is_symlink());
                    }
                    Err(e) => {
                        assert!(!file && !dir);
                        assert_eq!(e.kind(), io::ErrorKind::NotFound);
                    }
                }
            }
        }
        assert_eq!(
            files.canonicalize(Path::new("/p/src/deep/../a.ts"))?,
            Path::new("/p/src/a.ts")
        );
        assert_eq!(
            files.canonicalize(Path::new("/p/./src"))?,
            Path::new("/p/src")
        );
        Ok(())
    }

    #[test]
    fn a_memory_tree_refuses_what_it_does_not_hold() -> Result {
        let files = tree()?;
        let kind = |r: io::Result<Vec<u8>>| r.err().map(|e| e.kind());
        assert_eq!(
            kind(files.read(Path::new("/p/nope.ts"))),
            Some(io::ErrorKind::NotFound)
        );
        assert_eq!(
            kind(files.read(Path::new("/p/src"))),
            Some(io::ErrorKind::IsADirectory)
        );
        let invalid = files.read_to_string(Path::new("/p/bad.txt"));
        assert_eq!(
            invalid.err().map(|e| e.kind()),
            Some(io::ErrorKind::InvalidData)
        );
        let listed = |path: &str| files.read_dir(Path::new(path)).err().map(|e| e.kind());
        assert_eq!(listed("/p/src/a.ts"), Some(io::ErrorKind::NotADirectory));
        assert_eq!(listed("/q"), Some(io::ErrorKind::NotFound));
        let missing = files.canonicalize(Path::new("/p/nope"));
        assert_eq!(
            missing.err().map(|e| e.kind()),
            Some(io::ErrorKind::NotFound)
        );
        let message = files
            .read(Path::new("/p/nope.ts"))
            .err()
            .map(|e| e.to_string());
        assert_eq!(
            message.as_deref(),
            Some("/p/nope.ts is not in the file tree")
        );
        Ok(())
    }

    #[test]
    fn a_path_joins_the_tree_only_when_it_can_be_one_file() -> Result {
        let mut tree = MemoryTree::new();
        assert!(tree.is_empty());
        assert_eq!(
            tree.insert(Path::new("src/a.ts"), ""),
            Err(TreeError::NotAbsolute {
                path: PathBuf::from("src/a.ts")
            })
        );
        tree.insert(Path::new("/p/src/a.ts"), "one")?;
        tree.insert(Path::new("/p/src/a.ts"), "two")?;
        assert_eq!(tree.len(), 1, "a second insert replaces the first");
        assert_eq!(
            tree.insert(Path::new("/p/src"), ""),
            Err(TreeError::FileAndFolder {
                path: PathBuf::from("/p/src")
            })
        );
        assert_eq!(
            tree.insert(Path::new("/p/src/a.ts/x.ts"), ""),
            Err(TreeError::FileAndFolder {
                path: PathBuf::from("/p/src/a.ts")
            })
        );
        assert_eq!(
            tree.insert(Path::new("/"), ""),
            Err(TreeError::FileAndFolder {
                path: PathBuf::from("/")
            })
        );
        assert_eq!(tree.len(), 1);
        let files = Files::memory(tree);
        assert_eq!(files.read_to_string(Path::new("/p/src/a.ts"))?, "two");
        let error = TreeError::NotAbsolute {
            path: PathBuf::from("a"),
        };
        assert_eq!(
            error.to_string(),
            "a: a path in the file tree must be absolute"
        );
        Ok(())
    }

    #[test]
    fn the_disk_reads_through_std_fs() -> Result {
        let dir = std::env::temp_dir().join(format!("rb-model-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub"))?;
        std::fs::write(dir.join("a.txt"), "a")?;
        let files = Files::disk();
        assert!(!files.is_memory());
        assert!(files.tree().is_none());
        assert_eq!(files.read(&dir.join("a.txt"))?, b"a");
        assert_eq!(files.read_to_string(&dir.join("a.txt"))?, "a");
        assert!(files.is_file(&dir.join("a.txt")));
        assert!(files.is_dir(&dir.join("sub")));
        assert!(files.metadata(&dir.join("sub"))?.is_dir());
        assert!(files.symlink_metadata(&dir.join("a.txt"))?.is_file());
        assert_eq!(
            files.canonicalize(&dir)?,
            std::fs::canonicalize(&dir)?,
            "the disk's canonical form, not a lexical one"
        );
        let mut listed = Vec::new();
        for entry in files.read_dir(&dir)? {
            let entry = entry?;
            listed.push((
                entry.file_name().to_string_lossy().into_owned(),
                entry.file_type()?.is_dir(),
                entry.path() == dir.join(entry.file_name()),
            ));
        }
        listed.sort();
        assert_eq!(
            listed,
            [
                ("a.txt".to_owned(), false, true),
                ("sub".to_owned(), true, true)
            ]
        );
        assert!(files.read(&dir.join("missing")).is_err());
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn lexical_form_examples() {
        for (path, expected) in [
            ("/a/b/../c", "/a/c"),
            ("/a/./b/", "/a/b"),
            ("/../a", "/a"),
            ("a/../../b", "../b"),
            ("../a", "../a"),
            ("./a", "a"),
            ("", ""),
        ] {
            assert_eq!(lexical(Path::new(path)), Path::new(expected), "{path}");
        }
    }

    proptest::proptest! {
        /// The lexical form is a fixed point, has no `.` and, for an absolute path, no `..`; and
        /// a segment followed by `..` is as if neither were there.
        #[test]
        fn lexical_form_is_a_fixed_point(
            parts in proptest::collection::vec("(\\.|\\.\\.|[a-z]{1,3})", 0..8),
            rooted in proptest::bool::ANY,
        ) {
            let text = format!("{}{}", if rooted { "/" } else { "" }, parts.join("/"));
            let once = lexical(Path::new(&text));
            proptest::prop_assert_eq!(lexical(&once), once.clone());
            proptest::prop_assert!(!once.components().any(|c| c == Component::CurDir));
            if rooted {
                proptest::prop_assert!(once.has_root());
                proptest::prop_assert!(!once.components().any(|c| c == Component::ParentDir));
            }
            let detour = if text.is_empty() { "zz/..".to_owned() } else { format!("{text}/zz/..") };
            proptest::prop_assert_eq!(lexical(Path::new(&detour)), once);
        }
    }
}
