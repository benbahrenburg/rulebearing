//! Discovery: the import roots, the files under them, and each file's dotted module name.
//!
//! - Plan: [Wave 2, Step 4](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#24-step-4-the-python-extractor-2b)
//!   (`discover.rs`)
//! - Decision: [ADR-0013](../../../docs/adr/0013-ruff-parser-for-python.md) (roots from
//!   `pyproject.toml`, the `src/` layout and a `setup.cfg` fallback; the default excludes)
//! - Requirement: [FR-EXT-PY-01](../../../docs/prd.md#fr-ext-py-01)
//!
//! The roots come from the first of these that names any:
//!
//! | Source | Keys read |
//! | --- | --- |
//! | configuration | `languages.python.roots` |
//! | `pyproject.toml` | `[tool.setuptools] package-dir` (the `""` key, or the parent of a package's folder), `[tool.setuptools.packages.find] where`, `[tool.setuptools] packages` (the working directory) |
//! | the `src/` layout | a `src/` folder |
//! | `setup.cfg` | `[options] package_dir` |
//! | nothing | the working directory |
//!
//! `[project.scripts]` and `[project.gui-scripts]` targets are recorded as entry points, which an
//! orphan rule must not report, and `[project] requires-python` gives the default version.
//! `__init__.py` is its package's module identity; a folder of `.py` files without one is a
//! namespace package and still names the modules inside it.

use std::io;
use std::path::{Path, PathBuf};

use rb_model::Warning;

/// Folder names never walked: virtual environments, installed packages, bytecode caches and
/// version-control metadata. A folder holding `pyvenv.cfg` is a virtual environment whatever
/// its name, and is skipped too.
pub const EXCLUDED_DIRS: &[&str] = &[".venv", "venv", "site-packages", "__pycache__", ".git"];

/// What discovery found at the working directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    /// Import roots, relative to the working directory, posix-separated; `.` is the working
    /// directory itself. In the order imports are tried.
    pub roots: Vec<String>,
    /// Dotted module names of the console-script and GUI-script targets, sorted.
    pub entry_points: Vec<String>,
    /// The lower bound of `requires-python` as `major.minor`, when there is one.
    pub requires_python: Option<String>,
}

/// Why discovery could not read a project file.
#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    /// `pyproject.toml` is not valid TOML.
    #[error("{path}: not valid TOML ({reason}); fix the file or set languages.python.roots", path = path.display())]
    Toml {
        /// The file.
        path: PathBuf,
        /// The parser's message.
        reason: String,
    },
    /// Reading a file failed.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Finds the roots, entry points and `requires-python` for the project at `base`.
///
/// # Errors
/// [`DiscoverError`] when `pyproject.toml` or `setup.cfg` exists but cannot be read.
pub fn discover(base: &Path, configured: Option<&[String]>) -> Result<Layout, DiscoverError> {
    let mut layout = Layout::default();
    let pyproject = base.join("pyproject.toml");
    let mut from_pyproject = Vec::new();
    if pyproject.is_file() {
        let text = std::fs::read_to_string(&pyproject)?;
        let value: toml::Table = toml::from_str(&text).map_err(|e| DiscoverError::Toml {
            path: pyproject.clone(),
            reason: e.message().to_owned(),
        })?;
        from_pyproject = pyproject_roots(&value);
        layout.entry_points = entry_points(&value);
        layout.requires_python = value
            .get("project")
            .and_then(|p| p.get("requires-python"))
            .and_then(toml::Value::as_str)
            .and_then(lower_bound);
    }
    layout.roots = if let Some(roots) = configured.filter(|r| !r.is_empty()) {
        roots.iter().map(|r| normalise_root(r)).collect()
    } else if !from_pyproject.is_empty() {
        from_pyproject
    } else if base.join("src").is_dir() {
        vec!["src".to_owned()]
    } else {
        let cfg = base.join("setup.cfg");
        let from_cfg = if cfg.is_file() {
            setup_cfg_roots(&std::fs::read_to_string(&cfg)?)
        } else {
            Vec::new()
        };
        if from_cfg.is_empty() {
            vec![".".to_owned()]
        } else {
            from_cfg
        }
    };
    dedup_in_order(&mut layout.roots);
    Ok(layout)
}

fn dedup_in_order(items: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    items.retain(|item| seen.insert(item.clone()));
}

/// A root as written in a configuration, posix-separated with no leading `./` or trailing `/`;
/// the working directory is `.`.
pub fn normalise_root(root: &str) -> String {
    let mut text = root.replace('\\', "/");
    while let Some(rest) = text.strip_prefix("./") {
        text = rest.to_owned();
    }
    let trimmed = text.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        ".".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn parent_root(folder: &str) -> String {
    let folder = normalise_root(folder);
    match folder.rsplit_once('/') {
        Some((parent, _)) => normalise_root(parent),
        None => ".".to_owned(),
    }
}

/// The roots `pyproject.toml` names through setuptools' keys.
fn pyproject_roots(value: &toml::Table) -> Vec<String> {
    let Some(setuptools) = value
        .get("tool")
        .and_then(|t| t.get("setuptools"))
        .and_then(toml::Value::as_table)
    else {
        return Vec::new();
    };
    let mut roots = Vec::new();
    if let Some(dirs) = setuptools
        .get("package-dir")
        .and_then(toml::Value::as_table)
    {
        // `"" = "src"` makes `src` a root; `"pkg" = "lib/pkg"` makes `lib` one.
        if let Some(root) = dirs.get("").and_then(toml::Value::as_str) {
            roots.push(normalise_root(root));
        }
        for (package, folder) in dirs {
            if package.is_empty() {
                continue;
            }
            if let Some(folder) = folder.as_str() {
                roots.push(parent_root(folder));
            }
        }
    }
    if let Some(find) = setuptools
        .get("packages")
        .and_then(|p| p.get("find"))
        .and_then(toml::Value::as_table)
    {
        let find_roots = find
            .get("where")
            .and_then(toml::Value::as_array)
            .map_or_else(
                || vec![".".to_owned()],
                |w| {
                    w.iter()
                        .filter_map(toml::Value::as_str)
                        .map(normalise_root)
                        .collect()
                },
            );
        roots.extend(find_roots);
    } else if roots.is_empty()
        && setuptools
            .get("packages")
            .is_some_and(toml::Value::is_array)
    {
        roots.push(".".to_owned());
    }
    roots
}

/// `[project.scripts]` and `[project.gui-scripts]` targets as dotted module names, sorted.
fn entry_points(value: &toml::Table) -> Vec<String> {
    let mut targets: Vec<String> = ["scripts", "gui-scripts"]
        .iter()
        .filter_map(|key| value.get("project")?.get(*key)?.as_table())
        .flat_map(|table| table.values())
        .filter_map(toml::Value::as_str)
        .filter_map(|target| {
            let module = target.split(':').next()?.trim();
            (!module.is_empty()).then(|| module.to_owned())
        })
        .collect();
    targets.sort();
    targets.dedup();
    targets
}

/// The `[options] package_dir` roots of a `setup.cfg`: `=src` or `pkg = lib/pkg` entries.
pub fn setup_cfg_roots(text: &str) -> Vec<String> {
    let mut roots = Vec::new();
    let mut in_options = false;
    let mut in_package_dir = false;
    for raw in text.lines() {
        let line = raw.split(['#', ';']).next().unwrap_or_default();
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_options = trimmed == "[options]";
            in_package_dir = false;
            continue;
        }
        if !in_options || trimmed.is_empty() {
            continue;
        }
        let continuation = line.starts_with([' ', '\t']);
        let entry = if continuation && in_package_dir {
            trimmed
        } else if let Some((key, value)) = trimmed.split_once('=') {
            in_package_dir = key.trim() == "package_dir";
            if !in_package_dir {
                continue;
            }
            value.trim()
        } else {
            in_package_dir = false;
            continue;
        };
        if entry.is_empty() {
            continue;
        }
        let (package, folder) = entry.split_once('=').unwrap_or(("", entry));
        let folder = folder.trim();
        if folder.is_empty() {
            continue;
        }
        roots.push(if package.trim().is_empty() {
            normalise_root(folder)
        } else {
            parent_root(folder)
        });
    }
    dedup_in_order(&mut roots);
    roots
}

/// The lower bound of a `requires-python` specifier set as `major.minor` (`>=3.9,<4` is
/// `3.9`, `~=3.10` is `3.10`, `>3.8` is `3.9`), or `None` when it has none.
pub fn lower_bound(requires: &str) -> Option<String> {
    let mut best: Option<(u32, u32)> = None;
    for clause in requires.split(',') {
        let clause = clause.trim();
        let (strict, version) = if let Some(v) = clause.strip_prefix(">=") {
            (false, v)
        } else if let Some(v) = clause.strip_prefix("~=") {
            (false, v)
        } else if let Some(v) = clause.strip_prefix("==") {
            (false, v)
        } else if let Some(v) = clause.strip_prefix('>') {
            (true, v)
        } else {
            continue;
        };
        let mut parts = version.trim().trim_end_matches(".*").split('.');
        let Some(Ok(major)) = parts.next().map(str::parse::<u32>) else {
            continue;
        };
        let minor = parts
            .next()
            .and_then(|m| m.parse::<u32>().ok())
            .unwrap_or(0);
        let patch_given = parts.next().is_some();
        let bound = if strict && !patch_given {
            // `>3.4294967295` has no next minor version; the clause bounds nothing.
            let Some(next) = minor.checked_add(1) else {
                continue;
            };
            (major, next)
        } else {
            (major, minor)
        };
        best = Some(best.map_or(bound, |b| b.max(bound)));
    }
    best.map(|(major, minor)| format!("{major}.{minor}"))
}

/// The dotted module name of a file path relative to its root (`pkg/sub/mod.py` is
/// `pkg.sub.mod`, `pkg/__init__.py` is `pkg`), or `None` for a path that is not a module or a
/// root-level `__init__.py`.
pub fn module_name(relative: &str) -> Option<String> {
    let stem = relative
        .strip_suffix(".py")
        .or_else(|| relative.strip_suffix(".pyi"))?;
    let stem = stem
        .strip_suffix("/__init__")
        .or_else(|| (stem == "__init__").then_some(""))
        .unwrap_or(stem);
    if stem.is_empty() || stem.split('/').any(|part| !is_identifier(part)) {
        return None;
    }
    Some(stem.replace('/', "."))
}

/// Whether a path component can be part of a dotted module name.
pub fn is_identifier(part: &str) -> bool {
    let mut chars = part.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// The two files that could hold a dotted module, relative to its root: the module file, then
/// the package's `__init__`, with the given extension (`py` or `pyi`).
pub fn candidate_paths(dotted: &str, extension: &str) -> [String; 2] {
    let path = dotted.replace('.', "/");
    [
        format!("{path}.{extension}"),
        format!("{path}/__init__.{extension}"),
    ]
}

/// What a walk found: the source files and the problems met on the way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Walked {
    /// Every source file, relative to the working directory, posix-separated, sorted.
    pub files: Vec<String>,
    /// A broken symbolic link named like a source file, one warning each.
    pub warnings: Vec<Warning>,
}

/// Every `.py` file (and `.pyi` when `stubs`) under `inputs`, relative to `base`,
/// posix-separated, sorted. Excluded folders and symbolic links to folders are not entered; a
/// symbolic link to a file is followed, and a broken one named like a source file is a warning.
///
/// # Errors
/// An input that does not exist, or a folder that cannot be read.
pub fn walk(base: &Path, inputs: &[PathBuf], stubs: bool) -> io::Result<Walked> {
    let mut walked = Walked::default();
    let default_input = [PathBuf::from(".")];
    let inputs = if inputs.is_empty() {
        &default_input[..]
    } else {
        inputs
    };
    for input in inputs {
        let path = if input.is_absolute() {
            input.clone()
        } else {
            base.join(input)
        };
        let meta = std::fs::metadata(&path)?;
        if meta.is_dir() {
            walk_dir(base, &path, stubs, &mut walked)?;
        } else if is_source(&path, stubs) {
            walked.files.push(relative(base, &path));
        }
    }
    walked.files.sort();
    walked.files.dedup();
    walked.warnings.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(walked)
}

fn is_source(path: &Path, stubs: bool) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some("py") => true,
        Some("pyi") => stubs,
        _ => false,
    }
}

fn walk_dir(base: &Path, dir: &Path, stubs: bool, walked: &mut Walked) -> io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            let name = entry.file_name();
            let excluded = name.to_str().is_some_and(|n| EXCLUDED_DIRS.contains(&n));
            if !excluded && !path.join("pyvenv.cfg").is_file() {
                walk_dir(base, &path, stubs, walked)?;
            }
        } else if kind.is_file() && is_source(&path, stubs) {
            walked.files.push(relative(base, &path));
        } else if kind.is_symlink() && is_source(&path, stubs) {
            // A link to a file is followed; a link to a folder is never entered, so a cycle
            // cannot be walked.
            match std::fs::metadata(&path) {
                Ok(target) if target.is_file() => walked.files.push(relative(base, &path)),
                Ok(_) => {}
                Err(error) => walked.warnings.push(Warning::about(
                    relative(base, &path),
                    format!(
                        "is a symbolic link whose target cannot be read ({error}); it is not analysed. Fix or remove the link"
                    ),
                )),
            }
        }
    }
    Ok(())
}

/// `path` relative to `base`, posix-separated; the path itself when it is not under `base`.
pub fn relative(base: &Path, path: &Path) -> String {
    let Ok(rel) = path.strip_prefix(base) else {
        return path.to_string_lossy().replace('\\', "/");
    };
    let text: Vec<String> = rel
        .components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    text.join("/")
}

/// The root a file belongs to (the longest root that contains it) and its path relative to
/// that root; files under no root are named relative to the working directory.
pub fn split_root<'a>(roots: &'a [String], file: &'a str) -> (Option<&'a str>, &'a str) {
    let mut best: Option<(&str, &str)> = None;
    for root in roots {
        let rest = if root == "." {
            Some(file)
        } else {
            file.strip_prefix(root.as_str())
                .and_then(|r| r.strip_prefix('/'))
        };
        if let Some(rest) = rest {
            let longer =
                best.is_none_or(|(b, _)| b == "." || (root != "." && root.len() > b.len()));
            if longer {
                best = Some((root.as_str(), rest));
            }
        }
    }
    match best {
        Some((root, rest)) => (Some(root), rest),
        None => (None, file),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rb-py-discover-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn write(dir: &Path, file: &str, text: &str) {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, text);
    }

    #[test]
    fn roots_follow_the_documented_order() -> Result<(), DiscoverError> {
        let dir = scratch("order");
        assert_eq!(discover(&dir, None)?.roots, ["."]);
        write(
            &dir,
            "setup.cfg",
            "[metadata]\nname = x\n[options]\npackage_dir =\n    =lib\n",
        );
        assert_eq!(discover(&dir, None)?.roots, ["lib"]);
        std::fs::create_dir_all(dir.join("src"))?;
        assert_eq!(discover(&dir, None)?.roots, ["src"]);
        write(
            &dir,
            "pyproject.toml",
            "[project]\nrequires-python = \">=3.10\"\n[project.scripts]\nb = \"pkg.cli:main\"\na = \"pkg.tool:run\"\n[project.gui-scripts]\ng = \"pkg.cli:gui\"\n[tool.setuptools.packages.find]\nwhere = [\"code\"]\n",
        );
        let layout = discover(&dir, None)?;
        assert_eq!(layout.roots, ["code"]);
        assert_eq!(layout.entry_points, ["pkg.cli", "pkg.tool"]);
        assert_eq!(layout.requires_python.as_deref(), Some("3.10"));
        let configured = vec!["./a/".to_owned(), "a".to_owned(), "b".to_owned()];
        assert_eq!(discover(&dir, Some(&configured))?.roots, ["a", "b"]);
        assert_eq!(discover(&dir, Some(&[]))?.roots, ["code"]);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn setuptools_package_dir_and_packages() {
        let table = |text: &str| toml::from_str::<toml::Table>(text).unwrap_or_default();
        assert_eq!(
            pyproject_roots(&table("[tool.setuptools.package-dir]\n\"\" = \"src\"\n")),
            ["src"]
        );
        assert_eq!(
            pyproject_roots(&table("[tool.setuptools.package-dir]\npkg = \"lib/pkg\"\n")),
            ["lib"]
        );
        assert_eq!(
            pyproject_roots(&table("[tool.setuptools.package-dir]\npkg = \"pkg\"\n")),
            ["."]
        );
        assert_eq!(
            pyproject_roots(&table("[tool.setuptools]\npackages = [\"pkg\"]\n")),
            ["."]
        );
        assert_eq!(
            pyproject_roots(&table(
                "[tool.setuptools.packages.find]\ninclude = [\"x*\"]\n"
            )),
            ["."]
        );
        assert!(pyproject_roots(&table("[tool.poetry]\nname = \"x\"\n")).is_empty());
        assert!(pyproject_roots(&table("[tool.setuptools]\nzip-safe = false\n")).is_empty());
    }

    #[test]
    fn a_broken_pyproject_is_a_named_error() {
        let dir = scratch("broken");
        write(&dir, "pyproject.toml", "[project\n");
        let error = discover(&dir, None).err().map(|e| e.to_string());
        assert!(
            error.as_deref().is_some_and(
                |e| e.contains("pyproject.toml") && e.contains("languages.python.roots")
            ),
            "{error:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn setup_cfg_package_dir_forms() {
        assert_eq!(setup_cfg_roots("[options]\npackage_dir = =src\n"), ["src"]);
        assert_eq!(
            setup_cfg_roots("[options]\npackage_dir =\n    = src\n    pkg = lib/pkg ; note\n"),
            ["src", "lib"]
        );
        assert!(setup_cfg_roots("[metadata]\npackage_dir = =src\n").is_empty());
        assert!(setup_cfg_roots("[options]\nzip_safe = False\n  indented\n").is_empty());
        assert!(setup_cfg_roots("[options]\npackage_dir =\n    =\n").is_empty());
        assert!(setup_cfg_roots("[options]\nnot a pair\n").is_empty());
    }

    #[test]
    fn requires_python_lower_bounds() {
        let table: &[(&str, Option<&str>)] = &[
            (">=3.9", Some("3.9")),
            (">=3.8, <4", Some("3.8")),
            ("~=3.10", Some("3.10")),
            (">3.8", Some("3.9")),
            (">3.8.1", Some("3.8")),
            ("==3.11.*", Some("3.11")),
            (">=3", Some("3.0")),
            ("<3.12", None),
            ("!=3.9", None),
            (">=x", None),
            ("", None),
            (">3.4294967295", None),
            (">3.4294967295, >=3.10", Some("3.10")),
            (">=3.4294967295", Some("3.4294967295")),
        ];
        for &(spec, expected) in table {
            assert_eq!(lower_bound(spec).as_deref(), expected, "{spec}");
        }
    }

    #[test]
    fn module_names_from_paths() {
        assert_eq!(
            module_name("pkg/sub/mod.py").as_deref(),
            Some("pkg.sub.mod")
        );
        assert_eq!(module_name("pkg/__init__.py").as_deref(), Some("pkg"));
        assert_eq!(module_name("pkg/x.pyi").as_deref(), Some("pkg.x"));
        assert_eq!(module_name("top.py").as_deref(), Some("top"));
        assert_eq!(module_name("__init__.py"), None);
        assert_eq!(module_name("my-scripts/x.py"), None);
        assert_eq!(module_name("pkg/README.md"), None);
        assert!(is_identifier("_x1") && !is_identifier("1x") && !is_identifier(""));
    }

    #[test]
    fn roots_are_normalised_and_split() {
        assert_eq!(normalise_root("./src/"), "src");
        assert_eq!(normalise_root("."), ".");
        assert_eq!(normalise_root(""), ".");
        assert_eq!(normalise_root("a\\b"), "a/b");
        let roots = vec![".".to_owned(), "src".to_owned(), "src/inner".to_owned()];
        assert_eq!(
            split_root(&roots, "src/inner/a.py"),
            (Some("src/inner"), "a.py")
        );
        assert_eq!(split_root(&roots, "src/b.py"), (Some("src"), "b.py"));
        assert_eq!(split_root(&roots, "tests/t.py"), (Some("."), "tests/t.py"));
        let only_src = vec!["src".to_owned()];
        assert_eq!(split_root(&only_src, "tests/t.py"), (None, "tests/t.py"));
        assert_eq!(split_root(&only_src, "srcx/t.py"), (None, "srcx/t.py"));
    }

    #[test]
    fn walk_skips_excluded_folders_and_stubs() -> io::Result<()> {
        let dir = scratch("walk");
        for file in [
            "src/pkg/__init__.py",
            "src/pkg/a.py",
            "src/pkg/a.pyi",
            "src/pkg/__pycache__/a.py",
            ".venv/lib/x.py",
            "venv/lib/x.py",
            "env/pyvenv.cfg",
            "env/lib/x.py",
            "site-packages/y.py",
            "notes.txt",
            "top.py",
        ] {
            write(&dir, file, "");
        }
        assert_eq!(
            walk(&dir, &[], false)?.files,
            ["src/pkg/__init__.py", "src/pkg/a.py", "top.py"]
        );
        assert_eq!(
            walk(&dir, &[PathBuf::from("src")], true)?.files,
            ["src/pkg/__init__.py", "src/pkg/a.py", "src/pkg/a.pyi"]
        );
        assert_eq!(
            walk(
                &dir,
                &[dir.join("top.py"), PathBuf::from("notes.txt")],
                false
            )?
            .files,
            ["top.py"]
        );
        assert!(walk(&dir, &[PathBuf::from("missing")], false).is_err());
        assert_eq!(relative(Path::new("/a"), Path::new("/b/c.py")), "/b/c.py");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_to_files_are_followed_and_broken_ones_are_warnings() -> io::Result<()> {
        use std::os::unix::fs::symlink;
        let dir = scratch("links");
        write(&dir, "real/x.py", "");
        write(&dir, "pkg/__init__.py", "");
        symlink(dir.join("real/x.py"), dir.join("pkg/linked.py"))?;
        symlink(dir.join("real/gone.py"), dir.join("pkg/broken.py"))?;
        symlink(dir.join("real/gone.txt"), dir.join("pkg/broken.txt"))?;
        symlink(dir.join("real"), dir.join("pkg/folder"))?;
        symlink(dir.join("pkg"), dir.join("pkg/cycle"))?;
        let walked = walk(&dir, &[], false)?;
        assert_eq!(
            walked.files,
            ["pkg/__init__.py", "pkg/linked.py", "real/x.py"]
        );
        let warned: Vec<(Option<PathBuf>, bool)> = walked
            .warnings
            .iter()
            .map(|w| (w.path.clone(), w.message.contains("symbolic link")))
            .collect();
        assert_eq!(warned, [(Some(PathBuf::from("pkg/broken.py")), true)]);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    proptest! {
        /// A dotted name maps to its candidate files and back again.
        #[test]
        fn dotted_names_round_trip(parts in proptest::collection::vec("[a-z_][a-z0-9_]{0,8}", 1..5)) {
            prop_assume!(!parts.iter().any(|p| p == "__init__"));
            let dotted = parts.join(".");
            for candidate in candidate_paths(&dotted, "py") {
                prop_assert_eq!(module_name(&candidate), Some(dotted.clone()));
            }
            for candidate in candidate_paths(&dotted, "pyi") {
                prop_assert_eq!(module_name(&candidate), Some(dotted.clone()));
            }
        }

        /// Normalising a root is idempotent.
        #[test]
        fn normalise_root_is_idempotent(root in "[./a-z\\\\]{0,12}") {
            let once = normalise_root(&root);
            prop_assert_eq!(normalise_root(&once), once);
        }
    }
}
