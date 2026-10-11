//! The resolver's file system over [`rb_model::Files`]: `oxc_resolver`'s own `FileSystemOs` for
//! the disk, so a native resolution is the one it always was, and the file tree for files held
//! in memory.
//!
//! - Decision: [ADR-0073](../../../docs/adr/0073-extractors-read-files-through-rb-model.md)
//!   (every read the extractor makes goes through [`rb_model::Files`]),
//!   [ADR-0012](../../../docs/adr/0012-oxc-for-typescript.md) (`oxc_resolver`)
//! - Plan: [Wave 4, Step 2](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables)
//! - Requirement: [FR-REACH-01](../../../docs/prd.md#fr-reach-01)
//!
//! A tree in memory holds no symbolic links, so `read_link` there answers as the disk does for a
//! path that is not a link; the resolver asks only after `symlink_metadata` said it is one.

use std::io;
use std::path::{Path, PathBuf};

use oxc_resolver::{FileMetadata, FileSystem, FileSystemOs, ResolveError};
use rb_model::{FileKind, Files};

/// `oxc_resolver`'s file system for one run's [`Files`].
#[derive(Debug, Clone)]
pub struct ResolverFiles {
    files: Files,
    os: FileSystemOs,
}

impl ResolverFiles {
    /// The resolver's view of `files`; `yarn_pnp` as `FileSystemOs` takes it, for the disk.
    pub fn new(files: Files, yarn_pnp: bool) -> Self {
        Self {
            files,
            os: <FileSystemOs as FileSystem>::new(yarn_pnp),
        }
    }
}

fn metadata(kind: FileKind) -> FileMetadata {
    FileMetadata::new(kind.is_file(), kind.is_dir(), kind.is_symlink())
}

impl FileSystem for ResolverFiles {
    fn new(yarn_pnp: bool) -> Self {
        Self::new(Files::disk(), yarn_pnp)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        match self.files.tree() {
            None => self.os.read(path),
            Some(_) => self.files.read(path),
        }
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        match self.files.tree() {
            None => self.os.read_to_string(path),
            Some(_) => FileSystemOs::validate_string(self.files.read(path)?),
        }
    }

    fn metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        match self.files.tree() {
            None => FileSystem::metadata(&self.os, path),
            Some(_) => self.files.metadata(path).map(metadata),
        }
    }

    fn symlink_metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        match self.files.tree() {
            None => FileSystem::symlink_metadata(&self.os, path),
            Some(_) => self.files.symlink_metadata(path).map(metadata),
        }
    }

    fn read_link(&self, path: &Path) -> Result<PathBuf, ResolveError> {
        match self.files.tree() {
            None => FileSystem::read_link(&self.os, path),
            Some(_) => Err(ResolveError::from(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is not a symbolic link", path.display()),
            ))),
        }
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        match self.files.tree() {
            None => FileSystem::canonicalize(&self.os, path),
            Some(_) => self.files.canonicalize(path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_model::MemoryTree;

    type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    #[test]
    fn files_in_memory_answer_the_resolver() -> Result {
        let mut tree = MemoryTree::new();
        tree.insert(Path::new("/p/src/a.ts"), "export {};")?;
        tree.insert(Path::new("/p/bad.ts"), vec![0xff])?;
        let fs = ResolverFiles::new(Files::memory(tree), false);
        assert_eq!(fs.read(Path::new("/p/src/a.ts"))?, b"export {};");
        assert_eq!(fs.read_to_string(Path::new("/p/src/a.ts"))?, "export {};");
        assert!(fs.read_to_string(Path::new("/p/bad.ts")).is_err());
        let file = FileSystem::metadata(&fs, Path::new("/p/src/a.ts"))?;
        assert!(file.is_file() && !file.is_dir() && !file.is_symlink());
        let folder = FileSystem::symlink_metadata(&fs, Path::new("/p/src"))?;
        assert!(folder.is_dir() && !folder.is_file() && !folder.is_symlink());
        assert!(FileSystem::metadata(&fs, Path::new("/p/none")).is_err());
        assert!(FileSystem::read_link(&fs, Path::new("/p/src/a.ts")).is_err());
        assert_eq!(
            FileSystem::canonicalize(&fs, Path::new("/p/src/../src/a.ts"))?,
            Path::new("/p/src/a.ts")
        );
        Ok(())
    }

    #[test]
    fn the_disk_answers_as_oxc_resolver_s_own() -> Result {
        let dir = std::env::temp_dir().join(format!("rb-ts-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("a.ts"), "x")?;
        let ours = <ResolverFiles as FileSystem>::new(false);
        let theirs = <FileSystemOs as FileSystem>::new(false);
        let file = dir.join("a.ts");
        assert_eq!(ours.read(&file)?, theirs.read(&file)?);
        assert_eq!(ours.read_to_string(&file)?, theirs.read_to_string(&file)?);
        let (a, b) = (
            FileSystem::metadata(&ours, &file)?,
            FileSystem::metadata(&theirs, &file)?,
        );
        assert_eq!((a.is_file(), a.is_dir()), (b.is_file(), b.is_dir()));
        let (a, b) = (
            FileSystem::symlink_metadata(&ours, &dir)?,
            FileSystem::symlink_metadata(&theirs, &dir)?,
        );
        assert_eq!((a.is_dir(), a.is_symlink()), (b.is_dir(), b.is_symlink()));
        assert_eq!(
            FileSystem::canonicalize(&ours, &file)?,
            FileSystem::canonicalize(&theirs, &file)?
        );
        assert!(FileSystem::read_link(&ours, &file).is_err());
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
