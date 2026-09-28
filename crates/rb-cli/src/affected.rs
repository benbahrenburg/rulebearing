//! `--affected [revision]`: report only the modules changed since a revision and the modules
//! that reach them, as dependency-cruiser's command line does, with .NET source files mapped to
//! their types' modules.
//!
//! - Architecture: [architecture § Performance model](../../../docs/architecture.md#performance-model)
//! - Plan: [Wave 3, Step 3](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict),
//!   [Wave 3 § 1.4](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#the-cache-and---affected),
//!   [§ 1.7](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#17-quality-attributes) (the receipt)
//! - Coverage: [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options)
//!   row `affected`, [§ Command line](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line)
//!   row `--affected [revision]`
//! - Decisions: [ADR-0052](../../../docs/adr/0052-affected-is-upstreams-reaches-filter.md) (this
//!   module's semantics, with the measurements),
//!   [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//!   (the receipt is additive), [ADR-0008](../../../docs/adr/0008-exit-code-contract.md) (a
//!   revision git does not know is exit 2), [ADR-0011](../../../docs/adr/0011-read-dotnet-assemblies-not-source.md)
//!   (the PDB attribution the .NET mapping reads)
//! - Requirement: [FR-CLI-05](../../../docs/prd.md#fr-cli-05)
//!
//! **The semantics follow the configuration format** ([`Mode`]), as liveness does
//! ([ADR-0032](../../../docs/adr/0032-liveness-follows-the-configuration-format.md)): a
//! dependency-cruiser configuration (or none) gets dependency-cruiser's behaviour byte for byte,
//! described first below; a `rulebearing.*` configuration gets the closure the plan describes,
//! described after it.
//!
//! **With a dependency-cruiser configuration: exactly as upstream, plus two additions.**
//! dependency-cruiser 18.2.0's
//! `normalizeOptions` (`src/cli/normalize-cli-options.mjs`) asks watskeburt 6.0.0 for the files
//! changed since the revision and sets `reaches` to the regular expression it returns; the
//! report then holds the matching modules and every module that reaches them (`filterReaches`),
//! with only the edges between them. This module does the same, with the same `git` commands and
//! the same parsing, so `summary.optionsUsed.reaches`, the modules, the edges and the violations
//! of a TypeScript cruise are dependency-cruiser's byte for byte. Measured on a scratch
//! repository with the pinned upstream binary, that means, and so it means here:
//!
//! - `git diff <revision> --name-status` compares the revision with the working tree (committed,
//!   staged and unstaged changes, no merge base), and `git status --porcelain` adds the untracked
//!   files. Renamed and copied files count by their new name; deleted files, type changes and
//!   unmerged files are not in the expression. An untracked directory is reported by `git status`
//!   as the directory, which has no extension, so its files count only once staged.
//! - Only the extensions upstream lists count (`cjs`, `cjsx`, `coffee`, `csx`, `cts`, `js`,
//!   `json`, `jsx`, `litcoffee`, `ls`, `mjs`, `mts`, `svelte`, `ts`, `tsx`, `vue`, `vuex`).
//! - An edge from a kept module to a module outside the closure is dropped with the module, so a
//!   violation on such an edge is not reported; neither is a violation that belongs to no kept
//!   module. A cruise with nothing changed reports no modules.
//! - `affected` in a configuration file is ignored by dependency-cruiser ("a command line only
//!   option"); a dependency-cruiser configuration here warns and ignores it, a native one applies
//!   it as the flag would. The flag wins over both.
//!
//! The two additions, neither of which changes a TypeScript cruise:
//!
//! - **Other languages.** A changed file that is a Python or .NET module, and a changed file the
//!   .NET extractor attributed a type to (through the PDB's `Document` table: the type's primary
//!   file or, for a partial type, any of its other files), add their modules to the expression
//!   ([`pdb_documents_to_modules`]). A graph ingested from the C# fallback carries the same
//!   `attribution` and `files` fields, so the mapping is the same.
//! - **Depth.** `--affected-depth N` keeps only the modules that reach a changed one in at most
//!   `N` steps; the default, 0, keeps them all, as upstream. The depth is never written to
//!   `optionsUsed`, whose `reaches` upstream's schema closes; the receipt records it.
//!
//! **With a `rulebearing.*` configuration: the closure.** The rules are evaluated over the whole
//! graph, so cycles, reachability, dependents and instability are what a full cruise finds; the
//! report then keeps what touches the closure ([`Selection::narrow`]):
//!
//! - The changes are listed with `git status --porcelain --untracked-files=all`, so each file of
//!   a new, untracked folder counts. Every changed file that is a module counts, whatever its
//!   extension and language, plus the .NET mapping. A deleted file is no module of the current
//!   graph and its importers' edges no longer resolve to it, so its dependents are read from the
//!   saved graph (`.graph/cruise.json`) when there is one; without one they are not known.
//! - The closure is those modules and the modules that reach them (to `--affected-depth`). The
//!   report keeps the closure's modules with every edge they have, so a violation on an edge that
//!   leaves the closure (an edited file importing an unchanged, forbidden module) is reported.
//! - Which violations touch the closure ([`touches`]): a dependency, instability or module-level
//!   violation (orphan, `required`, `numberOfDependentsLessThan`) when its `from` module is in
//!   it; a cycle or reachability violation when any module of its path, or its `to`, is (without
//!   a depth the `from` module then is too, since it reaches every module of the path; with a
//!   depth, the `from` module joins the report); a folder violation when a closure module sits in
//!   the folder; an element or slice violation when an end names a closure module or a type
//!   declared in a closure file, and a slice violation whose ends name neither is kept.
//! - `reaches` is not set, so `optionsUsed` carries no expression.
//!
//! One divergence in both modes, on purpose: paths are made relative to the cruise's base directory, the way
//! module `source` names are. Upstream uses git's repository-relative paths as they are, so a
//! cruise from a subdirectory matches nothing and silently reports an empty graph.
//!
//! The receipt, `summary.affected` ([`rb_model::Affected`]), records the revision, every changed
//! path (deleted files included) and the modules the report kept; `--strict-schema` strips it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;

use rb_config::model::{CompatMode, FilterOption};
use rb_config::{Config, ConfigWarning};
use rb_model::{Affected, ChangeType, GraphDocument, Language};
use serde_json::{Map, Value, json};

/// The extensions dependency-cruiser passes to watskeburt, verbatim.
pub const UPSTREAM_EXTENSIONS: &[&str] = &[
    "cjs",
    "cjsx",
    "coffee",
    "csx",
    "cts",
    "js",
    "json",
    "jsx",
    "litcoffee",
    "ls",
    "mjs",
    "mts",
    "svelte",
    "ts",
    "tsx",
    "vue",
    "vuex",
];

/// The change types watskeburt's `regex` format keeps.
const UPSTREAM_CHANGE_TYPES: &[ChangeType] = &[
    ChangeType::Modified,
    ChangeType::Added,
    ChangeType::Renamed,
    ChangeType::Copied,
    ChangeType::Untracked,
];

/// Why the changed files could not be listed. Each is exit 2: the run cannot say what changed.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AffectedError {
    /// `git` is not on the path.
    #[error(
        "--affected needs git, and no git executable was found on the PATH; install git or drop --affected"
    )]
    GitMissing,
    /// The base directory is not inside a git repository.
    #[error("--affected: '{}' does not seem to be a git repository; run from inside one or drop --affected", dir.display())]
    NotARepository {
        /// The directory.
        dir: PathBuf,
    },
    /// git does not know the revision.
    #[error(
        "--affected: revision '{revision}' unknown; pass a revision git knows, such as HEAD, main or a commit"
    )]
    UnknownRevision {
        /// The revision.
        revision: String,
    },
    /// The revision would be read by git as an option.
    #[error(
        "--affected: '{revision}' is not a revision (it starts with '-'); pass a branch, tag or commit"
    )]
    NotARevision {
        /// What was given.
        revision: String,
    },
    /// The saved graph a native run reads deleted files' dependents from cannot be read.
    #[error("--affected: {reason}; delete or rewrite it (`rulebearing cruise -T json -f {}`)", file.display())]
    SavedGraph {
        /// The file.
        file: PathBuf,
        /// What is wrong with it.
        reason: String,
    },
    /// git failed otherwise.
    #[error("--affected: `git {command}` failed: {detail}")]
    Git {
        /// The git arguments.
        command: String,
        /// The status and what git wrote to stderr.
        detail: String,
    },
}

/// One changed file, as watskeburt lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The path, relative to the cruise's base directory, `/`-separated; the new name of a
    /// renamed or copied file.
    pub path: String,
    /// What happened to it.
    pub kind: ChangeType,
}

/// Which semantics an `--affected` run has: it follows the configuration format, as liveness
/// does ([ADR-0032](../../../docs/adr/0032-liveness-follows-the-configuration-format.md)).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// A dependency-cruiser configuration, or none: dependency-cruiser's `reaches` filter, byte
    /// for byte.
    #[default]
    Upstream,
    /// A `rulebearing.*` configuration: the closure's modules, with every violation that touches
    /// the closure, edges leaving it included.
    Closure,
}

impl Mode {
    /// The mode for a configuration's format.
    pub fn of(compat: CompatMode) -> Self {
        match compat {
            CompatMode::DependencyCruiser => Self::Upstream,
            CompatMode::Native => Self::Closure,
        }
    }
}

/// What an `--affected` run asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The revision to compare the working tree with.
    pub revision: String,
    /// `--affected-depth`, when given and not 0.
    pub depth: Option<u32>,
    /// The semantics, from the configuration format.
    pub mode: Mode,
}

/// The changes an `--affected` run found, carried from the command line to the evaluation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// The revision compared with.
    pub revision: String,
    /// `--affected-depth`, when given and not 0.
    pub depth: Option<u32>,
    /// The changes, in git's order: the diff first, then the untracked files.
    pub changes: Vec<Change>,
    /// The semantics.
    pub mode: Mode,
    /// [`Mode::Closure`]: the modules that depended on a deleted file in the saved graph.
    pub dependents_of_deleted: Vec<String>,
}

/// watskeburt's `mapChangeType`: git's one-letter status to its name.
pub fn change_type(letter: char) -> ChangeType {
    match letter {
        'A' => ChangeType::Added,
        'C' => ChangeType::Copied,
        'D' => ChangeType::Deleted,
        'M' => ChangeType::Modified,
        'R' => ChangeType::Renamed,
        'T' => ChangeType::TypeChanged,
        'U' => ChangeType::Unmerged,
        'B' => ChangeType::PairingBroken,
        ' ' => ChangeType::Unmodified,
        '?' => ChangeType::Untracked,
        '!' => ChangeType::Ignored,
        _ => ChangeType::Unknown,
    }
}

/// A name as watskeburt's patterns take it: one or more characters that are not a space or tab.
fn plain_name(text: &str) -> Option<&str> {
    (!text.is_empty() && !text.contains([' ', '\t'])).then_some(text)
}

/// watskeburt's `parseDiffLine` over one line of `git diff --name-status`: the type, then an
/// optional three-digit similarity, then the name and, for a rename or copy, the new name.
/// A line that does not have that shape is skipped, as upstream skips it.
pub fn parse_diff_line(line: &str) -> Option<(ChangeType, String)> {
    let mut chars = line.chars();
    let letter = chars.next().filter(|c| "ACDMRTUXB".contains(*c))?;
    let rest = chars.as_str();
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let rest = match digits {
        0 => rest,
        3 => &rest[3..],
        _ => return None,
    };
    let trimmed = rest.trim_start_matches([' ', '\t']);
    if trimmed.len() == rest.len() {
        return None;
    }
    let fields: Vec<&str> = trimmed
        .split([' ', '\t'])
        .filter(|f| !f.is_empty())
        .collect();
    // Only single spaces or tabs between the names keep upstream's shape; a name with a space
    // splits into more fields than the pattern allows, and the line is skipped.
    let name = match fields.as_slice() {
        [name] => plain_name(name)?,
        [_, new_name] => plain_name(new_name)?,
        _ => return None,
    };
    Some((change_type(letter), name.to_owned()))
}

/// watskeburt's `parseStatusLine` over one line of `git status --porcelain`: the staged and
/// unstaged letters, the name and, for a rename, ` -> ` and the new name. The type is the staged
/// one unless that is unmodified.
pub fn parse_status_line(line: &str) -> Option<(ChangeType, String)> {
    const LETTERS: &str = " ACDMRTUXB?!";
    let mut chars = line.chars();
    let staged = chars.next().filter(|c| LETTERS.contains(*c))?;
    let unstaged = chars.next().filter(|c| LETTERS.contains(*c))?;
    let rest = chars.as_str();
    let trimmed = rest.trim_start_matches([' ', '\t']);
    if trimmed.len() == rest.len() {
        return None;
    }
    let name = match trimmed.split_once(" -> ") {
        Some((old, new)) => {
            plain_name(old)?;
            plain_name(new)?
        }
        None => plain_name(trimmed)?,
    };
    let staged = change_type(staged);
    let kind = if staged == ChangeType::Unmodified {
        change_type(unstaged)
    } else {
        staged
    };
    Some((kind, name.to_owned()))
}

/// Runs git in `dir` and returns its standard output, or the exit status and standard error.
fn git(dir: &Path, args: &[&str]) -> Result<String, (Option<i32>, String)> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                (None, String::new())
            } else {
                (None, e.to_string())
            }
        })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err((
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
}

/// The error for a failed git command: a missing executable, a directory outside a repository,
/// or the command's own failure.
fn git_error(dir: &Path, args: &[&str], (status, stderr): (Option<i32>, String)) -> AffectedError {
    match status {
        None if stderr.is_empty() => AffectedError::GitMissing,
        Some(128 | 129) if stderr.contains("not a git repository") => {
            AffectedError::NotARepository {
                dir: dir.to_path_buf(),
            }
        }
        _ => AffectedError::Git {
            command: args.join(" "),
            detail: match status {
                Some(code) => format!("exit {code}: {stderr}"),
                None => stderr,
            },
        },
    }
}

/// A repository-relative path made relative to the base directory, `prefix` being the base
/// directory's own repository-relative path (`git rev-parse --show-prefix`, empty or ending in
/// `/`). A path outside the base directory is `None`.
pub fn relative_to_base(path: &str, prefix: &str) -> Option<String> {
    path.strip_prefix(prefix)
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
}

/// The files changed between `revision` and the working tree of the repository `repo` is in,
/// committed or not, untracked included, as watskeburt 6.0.0's `list` finds them, relative to
/// `repo`, in git's order.
///
/// # Errors
/// [`AffectedError`] when git is missing, `repo` is not in a repository, or git does not know
/// the revision.
pub fn changed_since(repo: &Path, revision: &str) -> Result<Vec<Change>, AffectedError> {
    list_changes(repo, revision, false)
}

/// [`changed_since`], with each file of an untracked folder listed (`git status --porcelain
/// --untracked-files=all`) rather than the folder: what a native `--affected` run reads.
///
/// # Errors
/// As [`changed_since`].
pub fn changed_since_all(repo: &Path, revision: &str) -> Result<Vec<Change>, AffectedError> {
    list_changes(repo, revision, true)
}

fn list_changes(
    repo: &Path,
    revision: &str,
    every_untracked_file: bool,
) -> Result<Vec<Change>, AffectedError> {
    if revision.is_empty() || revision.starts_with('-') {
        return Err(AffectedError::NotARevision {
            revision: revision.to_owned(),
        });
    }
    let prefix_args = ["rev-parse", "--show-prefix"];
    let prefix = git(repo, &prefix_args).map_err(|e| git_error(repo, &prefix_args, e))?;
    let prefix = prefix.trim_end_matches(['\n', '\r']);
    let diff_args = ["diff", revision, "--name-status"];
    let diff = git(repo, &diff_args).map_err(|e| match e {
        (Some(128), _) => AffectedError::UnknownRevision {
            revision: revision.to_owned(),
        },
        other => git_error(repo, &diff_args, other),
    })?;
    let status_args: &[&str] = if every_untracked_file {
        &["status", "--porcelain", "--untracked-files=all"]
    } else {
        &["status", "--porcelain"]
    };
    let status = git(repo, status_args).map_err(|e| git_error(repo, status_args, e))?;
    let untracked = status
        .lines()
        .filter_map(parse_status_line)
        .filter(|(kind, _)| *kind == ChangeType::Untracked);
    Ok(diff
        .lines()
        .filter_map(parse_diff_line)
        .chain(untracked)
        .filter_map(|(kind, name)| {
            relative_to_base(&name, prefix).map(|path| Change { path, kind })
        })
        .collect())
}

/// Node's `path.extname`: the last `.` of the base name and what follows it, or empty when the
/// base name has no `.` past its first character.
pub fn extension(path: &str) -> &str {
    let base = path.rsplit('/').next().unwrap_or(path);
    match base.rfind('.') {
        Some(at) if base[..at].bytes().any(|b| b != b'.') => &base[at..],
        _ => "",
    }
}

/// The changed files dependency-cruiser's expression names: the upstream change types with an
/// upstream extension, in git's order.
pub fn upstream_names(changes: &[Change]) -> Vec<String> {
    changes
        .iter()
        .filter(|c| UPSTREAM_CHANGE_TYPES.contains(&c.kind))
        .filter(|c| {
            extension(&c.path)
                .strip_prefix('.')
                .is_some_and(|e| UPSTREAM_EXTENSIONS.contains(&e))
        })
        .map(|c| c.path.clone())
        .collect()
}

/// watskeburt's `formatAsRegex`: each name with `\` doubled and `.` as `[.]`, joined with `|`
/// inside `^(?:...)$`. Nothing else is escaped, as upstream escapes nothing else.
pub fn upstream_pattern<S: AsRef<str>>(names: &[S]) -> String {
    let alternatives: Vec<String> = names
        .iter()
        .map(|n| n.as_ref().replace('\\', "\\\\").replace('.', "[.]"))
        .collect();
    format!("^(?:{})$", alternatives.join("|"))
}

/// The expression with the other languages' modules added: upstream's names first, then each
/// added module, escaped in full, sorted.
pub fn pattern_with(names: &[String], added: &[String]) -> String {
    if added.is_empty() {
        return upstream_pattern(names);
    }
    let mut all: Vec<String> = names
        .iter()
        .map(|n| n.replace('\\', "\\\\").replace('.', "[.]"))
        .collect();
    all.extend(added.iter().map(|m| regex::escape(m)));
    format!("^(?:{})$", all.join("|"))
}

/// The .NET modules a set of changed files affects, through the attribution the .NET extractor
/// wrote: a changed file that is itself a .NET module, and the module of every .NET type whose
/// primary file or other partial files include a changed file. A change to either file of a
/// partial class affects the type. Sorted, without duplicates.
pub fn pdb_documents_to_modules<S: AsRef<str>>(doc: &GraphDocument, changed: &[S]) -> Vec<String> {
    let changed: BTreeSet<&str> = changed.iter().map(AsRef::as_ref).collect();
    let modules: BTreeSet<&str> = doc
        .modules
        .iter()
        .filter(|m| m.language == Some(Language::Dotnet))
        .map(|m| m.source.as_str())
        .collect();
    let mut affected: BTreeSet<String> = changed
        .iter()
        .filter(|path| modules.contains(*path))
        .map(|path| (*path).to_owned())
        .collect();
    for ty in doc.code.iter().flat_map(|c| &c.types) {
        if ty.location.language != Language::Dotnet {
            continue;
        }
        let Some(primary) = ty.location.file.as_deref() else {
            continue;
        };
        let touched =
            changed.contains(primary) || ty.files.iter().any(|f| changed.contains(f.as_str()));
        if touched && modules.contains(primary) {
            affected.insert(primary.to_owned());
        }
    }
    affected.into_iter().collect()
}

/// The modules outside dependency-cruiser's languages that the changes affect: changed Python
/// modules, and the .NET modules [`pdb_documents_to_modules`] maps the changes to. Deleted files
/// count, since a stale graph still holds their types. Sorted, without duplicates.
pub fn other_language_modules(doc: &GraphDocument, changes: &[Change]) -> Vec<String> {
    let paths: Vec<&str> = changes
        .iter()
        .filter(|c| {
            !matches!(
                c.kind,
                ChangeType::Unmodified | ChangeType::Ignored | ChangeType::Unknown
            )
        })
        .map(|c| c.path.as_str())
        .collect();
    let set: BTreeSet<&str> = paths.iter().copied().collect();
    let mut modules: BTreeSet<String> = doc
        .modules
        .iter()
        .filter(|m| m.language == Some(Language::Python) && set.contains(m.source.as_str()))
        .map(|m| m.source.clone())
        .collect();
    modules.extend(pdb_documents_to_modules(doc, &paths));
    modules.into_iter().collect()
}

/// The changed modules and the modules that reach them in at most `depth` steps (0: any
/// number), following each dependency's `resolved` name backwards. Sorted.
pub fn affected_closure<S: AsRef<str>>(
    doc: &GraphDocument,
    changed: &[S],
    depth: u32,
) -> Vec<String> {
    let mut dependents: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for module in &doc.modules {
        for dependency in &module.dependencies {
            dependents
                .entry(dependency.resolved.as_str())
                .or_default()
                .push(module.source.as_str());
        }
    }
    let present: BTreeSet<&str> = doc.modules.iter().map(|m| m.source.as_str()).collect();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut queue: VecDeque<(&str, u32)> = VecDeque::new();
    for name in changed {
        if let Some(&name) = present.get(name.as_ref())
            && seen.insert(name)
        {
            queue.push_back((name, 0));
        }
    }
    while let Some((name, steps)) = queue.pop_front() {
        if depth != 0 && steps >= depth {
            continue;
        }
        for &dependent in dependents.get(name).into_iter().flatten() {
            if seen.insert(dependent) {
                queue.push_back((dependent, steps + 1));
            }
        }
    }
    seen.into_iter().map(str::to_owned).collect()
}

/// What `--affected` asks for: the flag, else a native configuration's `options.affected`. A
/// dependency-cruiser configuration's `options.affected` is ignored with a warning, as
/// dependency-cruiser ignores it.
pub fn request(flag: Option<&str>, depth: Option<u32>, config: &mut Config) -> Option<Request> {
    let depth = depth.filter(|d| *d != 0);
    let mode = Mode::of(config.compat);
    if let Some(revision) = flag {
        return Some(Request {
            revision: revision.to_owned(),
            depth,
            mode,
        });
    }
    let option = config.options.affected.clone()?;
    if config.compat == CompatMode::DependencyCruiser {
        config.warnings.push(ConfigWarning::general(
            "options.affected is a command-line option in dependency-cruiser, which ignores it in a configuration file; so does this run. Pass --affected [revision] instead",
        ));
        return None;
    }
    option.revision().map(|revision| Request {
        revision: revision.to_owned(),
        depth,
        mode,
    })
}

/// The modules that depended on one of `deleted` in the saved graph at `file`, when there is
/// one: the current graph cannot name them, since the deleted module is gone from it and its
/// importers' edges no longer resolve to it. Sorted.
///
/// # Errors
/// [`AffectedError::SavedGraph`] when the file exists and is not a cruise result.
pub fn dependents_in_saved_graph(
    file: &Path,
    deleted: &[&str],
) -> Result<Vec<String>, AffectedError> {
    if deleted.is_empty() || !file.is_file() {
        return Ok(Vec::new());
    }
    let unreadable = |reason: String| AffectedError::SavedGraph {
        file: file.to_path_buf(),
        reason,
    };
    let text = std::fs::read_to_string(file)
        .map_err(|e| unreadable(format!("cannot read {}: {e}", file.display())))?;
    let saved = rb_ingest::dependency_cruiser::read(&text)
        .map_err(|e| unreadable(format!("{} is not a cruise result: {e}", file.display())))?;
    let deleted: BTreeSet<&str> = deleted.iter().copied().collect();
    let found: BTreeSet<String> = saved
        .modules
        .iter()
        .filter(|m| {
            m.dependencies
                .iter()
                .any(|d| deleted.contains(d.resolved.as_str()))
        })
        .map(|m| m.source.clone())
        .collect();
    Ok(found.into_iter().collect())
}

/// Lists the changes for `request`. [`Mode::Upstream`] sets the configuration's `reaches` to
/// dependency-cruiser's expression, so `optionsUsed` and the report are upstream's;
/// [`Mode::Closure`] lists every untracked file and reads the dependents of deleted files from
/// the saved graph `saved`, and leaves `reaches` alone.
///
/// # Errors
/// [`AffectedError`] from [`changed_since`] and [`dependents_in_saved_graph`].
pub fn select(
    repo: &Path,
    saved: &Path,
    request: Request,
    config: &mut Config,
) -> Result<Selection, AffectedError> {
    let (changes, dependents_of_deleted) = match request.mode {
        Mode::Upstream => {
            let changes = changed_since(repo, &request.revision)?;
            config.options.reaches = Some(FilterOption {
                path: Some(upstream_pattern(&upstream_names(&changes))),
                depth: None,
            });
            (changes, Vec::new())
        }
        Mode::Closure => {
            let changes = changed_since_all(repo, &request.revision)?;
            let deleted: Vec<&str> = changes
                .iter()
                .filter(|c| c.kind == ChangeType::Deleted)
                .map(|c| c.path.as_str())
                .collect();
            let dependents = dependents_in_saved_graph(saved, &deleted)?;
            (changes, dependents)
        }
    };
    Ok(Selection {
        revision: request.revision,
        depth: request.depth,
        changes,
        mode: request.mode,
        dependents_of_deleted,
    })
}

/// Whether `violation` touches `closure`, for a native run: its `from` is in it; for a cycle or a
/// reachability violation, any module of its path or its `to` is; for a folder violation, a
/// module of the closure sits in the folder; for an element or slice violation, an end names a
/// module of the closure or a type declared in one of its files (`types`), and a slice violation
/// whose ends name no module and no type (slices are named by their pattern) is kept, since it
/// cannot be placed.
fn touches(
    violation: &rb_model::Violation,
    closure: &BTreeSet<&str>,
    types: &BTreeMap<&str, bool>,
) -> bool {
    use rb_model::ViolationType as T;
    let named = |name: &str| closure.contains(name) || types.get(name) == Some(&true);
    match violation.violation_type {
        Some(T::Cycle) => {
            closure.contains(violation.from.as_str())
                || violation
                    .cycle
                    .iter()
                    .flatten()
                    .any(|step| closure.contains(step.name.as_str()))
        }
        Some(T::Reachability) => {
            closure.contains(violation.from.as_str())
                || closure.contains(violation.to.as_str())
                || violation
                    .via
                    .iter()
                    .flatten()
                    .any(|step| closure.contains(step.name.as_str()))
        }
        Some(T::Folder) => {
            let folder = format!("{}/", violation.from);
            closure.iter().any(|m| m.starts_with(&folder))
        }
        Some(T::Element) => named(&violation.from) || named(&violation.to),
        Some(T::Slice) => {
            named(&violation.from)
                || named(&violation.to)
                || !(types.contains_key(violation.from.as_str())
                    || types.contains_key(violation.to.as_str()))
        }
        _ => closure.contains(violation.from.as_str()),
    }
}

impl Selection {
    /// [`Mode::Closure`]: the modules of `doc` the changes name: a changed file that is a module
    /// in any language, the .NET modules the PDB maps a changed file to, and the modules that
    /// depended on a deleted file in the saved graph. Sorted.
    pub fn seeds(&self, doc: &GraphDocument) -> Vec<String> {
        let present: BTreeSet<&str> = doc.modules.iter().map(|m| m.source.as_str()).collect();
        let paths: Vec<&str> = self
            .changes
            .iter()
            .filter(|c| {
                !matches!(
                    c.kind,
                    ChangeType::Unmodified | ChangeType::Ignored | ChangeType::Unknown
                )
            })
            .map(|c| c.path.as_str())
            .collect();
        let mut seeds: BTreeSet<String> = paths
            .iter()
            .copied()
            .chain(self.dependents_of_deleted.iter().map(String::as_str))
            .filter(|p| present.contains(p))
            .map(str::to_owned)
            .collect();
        seeds.extend(pdb_documents_to_modules(doc, &paths));
        seeds.into_iter().collect()
    }

    /// [`Mode::Closure`]: `doc` (the evaluated, unfiltered graph) narrowed to what a native run
    /// reports, and the receipt. The report keeps the closure's modules with every edge they
    /// have, so a violation on an edge that leaves the closure stays; with `--affected-depth`,
    /// also the `from` module of each cycle or reachability violation whose path touches the
    /// closure. Folders keep the ones a kept module sits in; element and slice violations, which
    /// belong to no module, keep the ones that touch the closure ([`touches`]). The re-summary
    /// then counts the kept modules' violations.
    pub fn narrow(&self, doc: &GraphDocument) -> (GraphDocument, Affected) {
        let closure = affected_closure(doc, &self.seeds(doc), self.depth.unwrap_or(0));
        let set: BTreeSet<&str> = closure.iter().map(String::as_str).collect();
        let mut types: BTreeMap<&str, bool> = BTreeMap::new();
        for ty in doc.code.iter().flat_map(|c| &c.types) {
            let declared = ty
                .location
                .file
                .iter()
                .chain(&ty.files)
                .any(|f| set.contains(f.as_str()));
            *types.entry(ty.full_name.as_str()).or_default() |= declared;
        }
        let mut kept = set.clone();
        for v in &doc.summary.violations {
            if matches!(
                v.violation_type,
                Some(rb_model::ViolationType::Cycle | rb_model::ViolationType::Reachability)
            ) && touches(v, &set, &types)
            {
                kept.insert(v.from.as_str());
            }
        }
        let mut narrowed = doc.clone();
        narrowed
            .modules
            .retain(|m| kept.contains(m.source.as_str()));
        if let Some(folders) = narrowed.folders.as_mut() {
            folders.retain(|f| {
                let folder = format!("{}/", f.name);
                kept.iter().any(|m| m.starts_with(&folder))
            });
        }
        narrowed.summary.violations.retain(|v| {
            !matches!(
                v.violation_type,
                Some(rb_model::ViolationType::Element | rb_model::ViolationType::Slice)
            ) || touches(v, &set, &types)
        });
        let changed: BTreeSet<String> = self.changes.iter().map(|c| c.path.clone()).collect();
        let receipt = Affected {
            revision: self.revision.clone(),
            changed: changed.into_iter().collect(),
            closure,
            depth: self.depth,
        };
        (narrowed, receipt)
    }

    /// The expression for `doc`: upstream's, plus the other languages' affected modules.
    pub fn pattern(&self, doc: &GraphDocument) -> String {
        pattern_with(
            &upstream_names(&self.changes),
            &other_language_modules(doc, &self.changes),
        )
    }

    /// Sets `optionsUsed.reaches` to `pattern`, as upstream records the expression it used.
    pub fn record(options_used: &mut Map<String, Value>, pattern: &str) {
        options_used.insert("reaches".into(), json!({ "path": pattern }));
    }

    /// The receipt: the revision, every changed path sorted, and the modules of `doc` (the
    /// evaluated, unfiltered graph) the report keeps for `pattern`.
    pub fn receipt(&self, doc: &GraphDocument, pattern: &str) -> Affected {
        let changed: BTreeSet<String> = self.changes.iter().map(|c| c.path.clone()).collect();
        let seeds: Vec<&str> = doc
            .modules
            .iter()
            .filter(|m| rb_rules::patterns::test(pattern, &m.source))
            .map(|m| m.source.as_str())
            .collect();
        Affected {
            revision: self.revision.clone(),
            changed: changed.into_iter().collect(),
            closure: affected_closure(doc, &seeds, self.depth.unwrap_or(0)),
            depth: self.depth,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rb_config::model::{AffectedOption, DEFAULT_AFFECTED_REVISION};
    use rb_model::{CodeLayer, Dependency, Location, Module, ModuleSystem, TypeElement};

    #[test]
    fn change_letters_map_as_watskeburt_maps_them() {
        for (letter, kind) in [
            ('A', ChangeType::Added),
            ('C', ChangeType::Copied),
            ('D', ChangeType::Deleted),
            ('M', ChangeType::Modified),
            ('R', ChangeType::Renamed),
            ('T', ChangeType::TypeChanged),
            ('U', ChangeType::Unmerged),
            ('B', ChangeType::PairingBroken),
            (' ', ChangeType::Unmodified),
            ('?', ChangeType::Untracked),
            ('!', ChangeType::Ignored),
            ('X', ChangeType::Unknown),
            ('z', ChangeType::Unknown),
        ] {
            assert_eq!(change_type(letter), kind, "{letter:?}");
        }
    }

    #[test]
    fn diff_lines_parse_as_upstream() {
        let table: &[(&str, Option<(ChangeType, &str)>)] = &[
            ("M\tsrc/a.ts", Some((ChangeType::Modified, "src/a.ts"))),
            ("A\tsrc/new.ts", Some((ChangeType::Added, "src/new.ts"))),
            ("D\tsrc/gone.ts", Some((ChangeType::Deleted, "src/gone.ts"))),
            (
                "R100\tsrc/old.ts\tsrc/new-name.ts",
                Some((ChangeType::Renamed, "src/new-name.ts")),
            ),
            (
                "C075\tsrc/a.ts\tsrc/copy.ts",
                Some((ChangeType::Copied, "src/copy.ts")),
            ),
            (
                "T\tsrc/link.ts",
                Some((ChangeType::TypeChanged, "src/link.ts")),
            ),
            (
                "M src/spaced.ts",
                Some((ChangeType::Modified, "src/spaced.ts")),
            ),
            (
                "M\tsrc/with space.ts",
                Some((ChangeType::Modified, "space.ts")),
            ),
            ("M\tsrc/a b c.ts", None),
            ("R10\tsrc/a.ts\tsrc/b.ts", None),
            ("Msrc/a.ts", None),
            ("Q\tsrc/a.ts", None),
            ("X\tsrc/a.ts", Some((ChangeType::Unknown, "src/a.ts"))),
            ("", None),
            ("M\t", None),
        ];
        for (line, expected) in table {
            let expected = expected.map(|(k, n)| (k, n.to_owned()));
            assert_eq!(parse_diff_line(line), expected, "{line:?}");
        }
    }

    #[test]
    fn status_lines_parse_as_upstream() {
        let table: &[(&str, Option<(ChangeType, &str)>)] = &[
            ("?? src/new.ts", Some((ChangeType::Untracked, "src/new.ts"))),
            (" M src/a.ts", Some((ChangeType::Modified, "src/a.ts"))),
            ("M  src/a.ts", Some((ChangeType::Modified, "src/a.ts"))),
            ("MM src/a.ts", Some((ChangeType::Modified, "src/a.ts"))),
            ("A  src/b.ts", Some((ChangeType::Added, "src/b.ts"))),
            ("D  src/c.ts", Some((ChangeType::Deleted, "src/c.ts"))),
            (
                "R  src/old.ts -> src/new.ts",
                Some((ChangeType::Renamed, "src/new.ts")),
            ),
            (
                "?? src/newdir/",
                Some((ChangeType::Untracked, "src/newdir/")),
            ),
            ("!! build/", Some((ChangeType::Ignored, "build/"))),
            ("?? src/with space.ts", None),
            ("Z  src/a.ts", None),
            ("M", None),
            ("MMsrc/a.ts", None),
        ];
        for (line, expected) in table {
            let expected = expected.map(|(k, n)| (k, n.to_owned()));
            assert_eq!(parse_status_line(line), expected, "{line:?}");
        }
    }

    #[test]
    fn extensions_are_nodes() {
        for (path, ext) in [
            ("src/a.ts", ".ts"),
            ("src/a.d.ts", ".ts"),
            ("a.json", ".json"),
            (".eslintrc", ""),
            ("src/.eslintrc", ""),
            (".a.mjs", ".mjs"),
            ("src/newdir/", ""),
            ("Makefile", ""),
            ("dir.v2/file", ""),
            ("a.", "."),
            ("..", ""),
        ] {
            assert_eq!(extension(path), ext, "{path}");
        }
    }

    fn change(path: &str, kind: ChangeType) -> Change {
        Change {
            path: path.into(),
            kind,
        }
    }

    #[test]
    fn upstream_names_keep_its_types_and_extensions_in_order() {
        let changes = [
            change("src/b.ts", ChangeType::Modified),
            change("src/gone.ts", ChangeType::Deleted),
            change("src/new-name.mjs", ChangeType::Renamed),
            change("docs/x.md", ChangeType::Modified),
            change("src/Order.cs", ChangeType::Modified),
            change("src/link.ts", ChangeType::TypeChanged),
            change("src/copy.vue", ChangeType::Copied),
            change("out.json", ChangeType::Untracked),
            change("src/a.ts", ChangeType::Added),
            change("src/newdir/", ChangeType::Untracked),
        ];
        assert_eq!(
            upstream_names(&changes),
            [
                "src/b.ts",
                "src/new-name.mjs",
                "src/copy.vue",
                "out.json",
                "src/a.ts"
            ]
        );
    }

    #[test]
    fn the_pattern_is_watskeburts() {
        assert_eq!(
            upstream_pattern(&["src/a.mjs", "src/new-name.mjs", "out.json"]),
            "^(?:src/a[.]mjs|src/new-name[.]mjs|out[.]json)$"
        );
        assert_eq!(upstream_pattern::<&str>(&[]), "^(?:)$");
        assert_eq!(upstream_pattern(&["a\\b.ts"]), "^(?:a\\\\b[.]ts)$");
        assert_eq!(
            upstream_pattern(&["src/a+b.ts"]),
            "^(?:src/a+b[.]ts)$",
            "upstream escapes nothing else"
        );
        assert_eq!(
            pattern_with(&["src/a.ts".into()], &[]),
            upstream_pattern(&["src/a.ts"])
        );
        assert_eq!(
            pattern_with(&["src/a.ts".into()], &["src/Order+Lines.cs".into()]),
            "^(?:src/a[.]ts|src/Order\\+Lines\\.cs)$"
        );
    }

    #[test]
    fn paths_are_made_relative_to_the_base_directory() {
        assert_eq!(
            relative_to_base("src/a.ts", "").as_deref(),
            Some("src/a.ts")
        );
        assert_eq!(
            relative_to_base("web/src/a.ts", "web/").as_deref(),
            Some("src/a.ts")
        );
        assert_eq!(relative_to_base("api/b.ts", "web/"), None);
        assert_eq!(relative_to_base("web/", "web/"), None);
    }

    fn module(source: &str, to: &[&str], language: Option<Language>) -> Module {
        Module {
            dependencies: to
                .iter()
                .map(|t| Dependency::new(*t, *t, ModuleSystem::Es6))
                .collect(),
            language,
            ..Module::new(source)
        }
    }

    fn chain() -> GraphDocument {
        // d -> c -> b -> a, e -> a, f alone.
        GraphDocument {
            modules: vec![
                module("src/a.ts", &[], None),
                module("src/b.ts", &["src/a.ts"], None),
                module("src/c.ts", &["src/b.ts"], None),
                module("src/d.ts", &["src/c.ts"], None),
                module("src/e.ts", &["src/a.ts"], None),
                module("src/f.ts", &[], None),
            ],
            ..GraphDocument::default()
        }
    }

    #[test]
    fn the_closure_follows_dependents_to_the_depth() {
        let doc = chain();
        assert_eq!(
            affected_closure(&doc, &["src/a.ts"], 0),
            ["src/a.ts", "src/b.ts", "src/c.ts", "src/d.ts", "src/e.ts"]
        );
        assert_eq!(
            affected_closure(&doc, &["src/a.ts"], 1),
            ["src/a.ts", "src/b.ts", "src/e.ts"]
        );
        assert_eq!(
            affected_closure(&doc, &["src/a.ts"], 2),
            ["src/a.ts", "src/b.ts", "src/c.ts", "src/e.ts"]
        );
        assert_eq!(
            affected_closure(&doc, &["src/c.ts", "src/f.ts", "src/gone.ts"], 0),
            ["src/c.ts", "src/d.ts", "src/f.ts"],
            "a changed file that is no module adds nothing"
        );
        assert!(affected_closure::<&str>(&doc, &[], 0).is_empty());
    }

    fn dotnet_type(full_name: &str, file: &str, files: &[&str]) -> TypeElement {
        let mut ty = TypeElement::new(
            full_name,
            full_name,
            "class",
            Location {
                language: Language::Dotnet,
                file: Some(file.into()),
                line: None,
                column: None,
            },
        );
        ty.files = files.iter().map(|f| (*f).to_owned()).collect();
        ty
    }

    fn partial_class() -> GraphDocument {
        let dotnet = Some(Language::Dotnet);
        GraphDocument {
            modules: vec![
                module("src/Order.cs", &["src/Customer.cs"], dotnet),
                module("src/Order.Lines.cs", &[], dotnet),
                module("src/Customer.cs", &[], dotnet),
                module("src/Billing.cs", &["src/Order.cs"], dotnet),
                module("py/app.py", &[], Some(Language::Python)),
                module("web/a.ts", &[], Some(Language::Typescript)),
            ],
            code: Some(CodeLayer {
                types: vec![
                    dotnet_type("Sample.Order", "src/Order.cs", &["src/Order.Parts.cs"]),
                    dotnet_type("Sample.Order.Line", "src/Order.Lines.cs", &[]),
                    dotnet_type("Sample.Customer", "src/Customer.cs", &[]),
                ],
                ..CodeLayer::default()
            }),
            ..GraphDocument::default()
        }
    }

    #[test]
    fn a_partial_type_is_affected_through_any_of_its_files() {
        let doc = partial_class();
        assert_eq!(
            pdb_documents_to_modules(&doc, &["src/Order.Parts.cs"]),
            ["src/Order.cs"],
            "the other part of a partial class maps to the type's module"
        );
        assert_eq!(
            pdb_documents_to_modules(&doc, &["src/Order.cs"]),
            ["src/Order.cs"]
        );
        assert_eq!(
            pdb_documents_to_modules(&doc, &["src/Customer.cs", "src/Order.Lines.cs"]),
            ["src/Customer.cs", "src/Order.Lines.cs"]
        );
        assert!(pdb_documents_to_modules(&doc, &["web/a.ts", "py/app.py"]).is_empty());
        assert!(pdb_documents_to_modules::<&str>(&doc, &[]).is_empty());
    }

    #[test]
    fn other_languages_add_their_changed_modules() {
        let doc = partial_class();
        let changes = [
            change("src/Order.Parts.cs", ChangeType::Modified),
            change("py/app.py", ChangeType::Modified),
            change("web/a.ts", ChangeType::Modified),
            change("src/Customer.cs", ChangeType::Deleted),
            change("src/Billing.cs", ChangeType::Unmodified),
        ];
        assert_eq!(
            other_language_modules(&doc, &changes),
            ["py/app.py", "src/Customer.cs", "src/Order.cs"]
        );
        let selection = Selection {
            revision: "HEAD".into(),
            depth: None,
            changes: changes.to_vec(),
            ..Selection::default()
        };
        let pattern = selection.pattern(&doc);
        assert_eq!(
            pattern,
            "^(?:web/a[.]ts|py/app\\.py|src/Customer\\.cs|src/Order\\.cs)$"
        );
        let receipt = selection.receipt(&doc, &pattern);
        assert_eq!(receipt.revision, "HEAD");
        assert_eq!(
            receipt.changed,
            [
                "py/app.py",
                "src/Billing.cs",
                "src/Customer.cs",
                "src/Order.Parts.cs",
                "web/a.ts"
            ]
        );
        assert_eq!(
            receipt.closure,
            [
                "py/app.py",
                "src/Billing.cs",
                "src/Customer.cs",
                "src/Order.cs",
                "web/a.ts"
            ]
        );
        assert_eq!(receipt.depth, None);
    }

    #[test]
    fn the_request_comes_from_the_flag_then_a_native_configuration() {
        let mut native = Config {
            compat: CompatMode::Native,
            ..Config::default()
        };
        assert_eq!(request(None, None, &mut native), None);
        assert_eq!(
            request(Some("HEAD"), Some(0), &mut native),
            Some(Request {
                revision: "HEAD".into(),
                depth: None,
                mode: Mode::Closure,
            })
        );
        native.options.affected = Some(AffectedOption::Enabled(true));
        assert_eq!(
            request(None, Some(2), &mut native),
            Some(Request {
                revision: DEFAULT_AFFECTED_REVISION.into(),
                depth: Some(2),
                mode: Mode::Closure,
            })
        );
        assert_eq!(
            request(Some("dev"), None, &mut native).map(|r| r.revision),
            Some("dev".into()),
            "the flag wins"
        );
        native.options.affected = Some(AffectedOption::Enabled(false));
        assert_eq!(request(None, None, &mut native), None);
        assert!(native.warnings.is_empty());
        let mut cruiser = Config {
            compat: CompatMode::DependencyCruiser,
            ..Config::default()
        };
        assert_eq!(
            request(Some("HEAD"), None, &mut cruiser).map(|r| r.mode),
            Some(Mode::Upstream),
            "a dependency-cruiser configuration keeps upstream's semantics"
        );
        cruiser.options.affected = Some(AffectedOption::Revision("dev".into()));
        assert_eq!(request(None, None, &mut cruiser), None);
        assert_eq!(cruiser.warnings.len(), 1);
        assert!(cruiser.warnings[0].message.contains("--affected"));
    }

    #[test]
    fn the_mode_follows_the_configuration_format() {
        assert_eq!(Mode::of(CompatMode::DependencyCruiser), Mode::Upstream);
        assert_eq!(Mode::of(CompatMode::Native), Mode::Closure);
        assert_eq!(Mode::default(), Mode::Upstream);
    }

    fn rule(name: &str) -> rb_model::RuleSummary {
        rb_model::RuleSummary {
            name: name.into(),
            severity: rb_model::Severity::Error,
        }
    }

    fn violation(kind: rb_model::ViolationType, from: &str, to: &str) -> rb_model::Violation {
        rb_model::Violation {
            from: from.into(),
            to: to.into(),
            unresolved_to: None,
            dependency_types: None,
            violation_type: Some(kind),
            rule: rule("r"),
            cycle: None,
            via: None,
            metrics: None,
            comment: None,
            id: None,
            fix: None,
            decision: None,
        }
    }

    fn steps(names: &[&str]) -> Vec<rb_model::MiniDependency> {
        names
            .iter()
            .map(|n| rb_model::MiniDependency {
                name: (*n).into(),
                dependency_types: Vec::new(),
            })
            .collect()
    }

    #[test]
    fn a_violation_touches_the_closure_by_its_shape() {
        use rb_model::ViolationType as T;
        let closure: BTreeSet<&str> = ["src/b.ts", "src/Order.cs"].into_iter().collect();
        let types: BTreeMap<&str, bool> = [("Sample.Order", true), ("Sample.Other", false)]
            .into_iter()
            .collect();
        let yes = |v: &rb_model::Violation| touches(v, &closure, &types);
        assert!(yes(&violation(T::Dependency, "src/b.ts", "src/outside.ts")));
        assert!(!yes(&violation(T::Dependency, "src/a.ts", "src/b.ts")));
        assert!(yes(&violation(T::Module, "src/b.ts", "src/b.ts")));
        assert!(!yes(&violation(T::Instability, "src/c.ts", "src/b.ts")));
        let mut cycle = violation(T::Cycle, "src/a.ts", "src/c.ts");
        assert!(!yes(&cycle));
        cycle.cycle = Some(steps(&["src/c.ts", "src/b.ts", "src/a.ts"]));
        assert!(yes(&cycle), "a module of the cycle is in the closure");
        let mut reach = violation(T::Reachability, "src/a.ts", "src/z.ts");
        reach.via = Some(steps(&["src/m.ts", "src/z.ts"]));
        assert!(!yes(&reach));
        reach.via = Some(steps(&["src/b.ts", "src/z.ts"]));
        assert!(yes(&reach), "the path runs through the closure");
        assert!(yes(&violation(T::Reachability, "src/a.ts", "src/b.ts")));
        assert!(yes(&violation(T::Folder, "src", "src")));
        assert!(!yes(&violation(T::Folder, "lib", "lib")));
        assert!(!yes(&violation(T::Folder, "sr", "sr")));
        assert!(yes(&violation(T::Element, "Sample.Order", "Sample.Order")));
        assert!(!yes(&violation(T::Element, "Sample.Other", "Sample.Other")));
        assert!(yes(&violation(T::Slice, "Sample.Other", "Sample.Order")));
        assert!(!yes(&violation(T::Slice, "Sample.Other", "Sample.Other")));
        assert!(
            yes(&violation(T::Slice, "orders", "billing")),
            "a slice violation that names no module or type cannot be placed, and stays"
        );
    }

    #[test]
    fn a_native_run_keeps_edges_that_leave_the_closure() {
        use rb_model::ViolationType as T;
        // a (changed) -> forbidden; b -> a; c alone with a cycle c <-> d.
        let mut doc = GraphDocument {
            modules: vec![
                module("src/a.ts", &["src/forbidden.ts"], None),
                module("src/b.ts", &["src/a.ts"], None),
                module("src/c.ts", &["src/d.ts"], None),
                module("src/d.ts", &["src/c.ts", "src/b.ts"], None),
                module("src/forbidden.ts", &[], None),
                module("py/app.py", &[], Some(Language::Python)),
            ],
            folders: Some(vec![rb_model::Folder {
                name: "src".into(),
                dependents: None,
                dependencies: None,
                module_count: 5,
                afferent_couplings: None,
                efferent_couplings: None,
                instability: None,
                experimental_stats: None,
            }]),
            ..GraphDocument::default()
        };
        let mut cycle = violation(T::Cycle, "src/c.ts", "src/d.ts");
        cycle.cycle = Some(steps(&["src/d.ts", "src/c.ts"]));
        doc.summary.violations = vec![
            violation(T::Dependency, "src/a.ts", "src/forbidden.ts"),
            cycle,
            violation(T::Slice, "orders", "billing"),
        ];
        let selection = Selection {
            revision: "HEAD".into(),
            depth: None,
            changes: vec![
                change("src/a.ts", ChangeType::Modified),
                change("src/gone.ts", ChangeType::Deleted),
                change("notes.md", ChangeType::Untracked),
            ],
            mode: Mode::Closure,
            dependents_of_deleted: vec!["src/c.ts".into(), "src/removed-too.ts".into()],
        };
        assert_eq!(selection.seeds(&doc), ["src/a.ts", "src/c.ts"]);
        let (narrowed, receipt) = selection.narrow(&doc);
        assert_eq!(
            receipt.closure,
            ["src/a.ts", "src/b.ts", "src/c.ts", "src/d.ts"]
        );
        assert_eq!(receipt.changed, ["notes.md", "src/a.ts", "src/gone.ts"]);
        let kept: Vec<&str> = narrowed.modules.iter().map(|m| m.source.as_str()).collect();
        assert_eq!(kept, ["src/a.ts", "src/b.ts", "src/c.ts", "src/d.ts"]);
        assert_eq!(
            narrowed.modules[0].dependencies[0].resolved, "src/forbidden.ts",
            "the edge that leaves the closure stays"
        );
        assert_eq!(narrowed.folders.as_ref().map(Vec::len), Some(1));
        assert_eq!(narrowed.summary.violations.len(), 3, "the slice stays");

        // With a depth of 1 from src/a.ts alone, the cycle is outside; a cycle path through the
        // closure brings its `from` module in.
        let shallow = Selection {
            depth: Some(1),
            dependents_of_deleted: Vec::new(),
            ..selection
        };
        let (narrowed, receipt) = shallow.narrow(&doc);
        assert_eq!(receipt.closure, ["src/a.ts", "src/b.ts"]);
        let kept: Vec<&str> = narrowed.modules.iter().map(|m| m.source.as_str()).collect();
        assert_eq!(kept, ["src/a.ts", "src/b.ts"]);
        let mut through = violation(T::Cycle, "src/d.ts", "src/b.ts");
        through.cycle = Some(steps(&["src/b.ts", "src/d.ts"]));
        doc.summary.violations.push(through);
        let (narrowed, _) = shallow.narrow(&doc);
        let kept: Vec<&str> = narrowed.modules.iter().map(|m| m.source.as_str()).collect();
        assert_eq!(kept, ["src/a.ts", "src/b.ts", "src/d.ts"]);
    }

    #[test]
    fn deleted_files_dependents_come_from_the_saved_graph() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = std::env::temp_dir().join(format!("rb-affected-saved-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let file = dir.join("cruise.json");
        assert!(
            dependents_in_saved_graph(&file, &["src/gone.ts"])?.is_empty(),
            "no file"
        );
        let saved = GraphDocument {
            modules: vec![
                module("src/user.ts", &["src/gone.ts"], None),
                module("src/other.ts", &["src/user.ts"], None),
                module("src/also.ts", &["src/gone.ts"], None),
                module("src/gone.ts", &[], None),
            ],
            ..GraphDocument::default()
        };
        std::fs::write(&file, serde_json::to_string(&saved)?)?;
        assert_eq!(
            dependents_in_saved_graph(&file, &["src/gone.ts"])?,
            ["src/also.ts", "src/user.ts"]
        );
        assert!(dependents_in_saved_graph(&file, &[])?.is_empty());
        std::fs::write(&file, "not json")?;
        let error = dependents_in_saved_graph(&file, &["src/gone.ts"]);
        assert!(
            matches!(error, Err(AffectedError::SavedGraph { .. })),
            "{error:?}"
        );
        assert!(
            error
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default()
                .contains("is not a cruise result")
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn option_like_revisions_are_refused_before_git_runs() {
        let dir = std::env::temp_dir();
        for revision in ["", "--output=/tmp/x", "-p"] {
            for list in [changed_since, changed_since_all] {
                assert_eq!(
                    list(&dir, revision),
                    Err(AffectedError::NotARevision {
                        revision: revision.into()
                    })
                );
            }
        }
    }

    #[test]
    fn git_failures_are_named() {
        let dir = Path::new("/nowhere");
        assert_eq!(
            git_error(dir, &["status"], (None, String::new())),
            AffectedError::GitMissing
        );
        assert_eq!(
            git_error(
                dir,
                &["status"],
                (
                    Some(128),
                    "fatal: not a git repository (or any parent)".into()
                )
            ),
            AffectedError::NotARepository { dir: dir.into() }
        );
        let other = git_error(dir, &["status", "--porcelain"], (Some(1), "boom".into()));
        assert_eq!(
            other.to_string(),
            "--affected: `git status --porcelain` failed: exit 1: boom"
        );
        let spawn = git_error(dir, &["status"], (None, "denied".into()));
        assert!(spawn.to_string().ends_with("failed: denied"));
    }

    proptest! {
        #[test]
        fn a_pattern_matches_exactly_its_plain_names(
            names in proptest::collection::vec("[a-z]{1,6}(/[a-z]{1,6}){0,2}\\.(ts|mjs|json)", 1..6),
            other in "[a-z]{1,6}/[a-z]{1,6}\\.tsx",
        ) {
            let pattern = upstream_pattern(&names);
            for name in &names {
                prop_assert!(rb_rules::patterns::test(&pattern, name), "{pattern} misses {name}");
            }
            prop_assert!(!rb_rules::patterns::test(&pattern, &other));
            let prefixed = format!("x{}", names[0]);
            prop_assert!(!rb_rules::patterns::test(&pattern, &prefixed));
        }

        #[test]
        fn the_closure_grows_with_the_depth(start in 0usize..6, depth in 1u32..5) {
            let doc = chain();
            let seed = doc.modules[start].source.clone();
            let shallow = affected_closure(&doc, &[seed.as_str()], depth);
            let deeper = affected_closure(&doc, &[seed.as_str()], depth + 1);
            let all = affected_closure(&doc, &[seed.as_str()], 0);
            prop_assert!(shallow.contains(&seed));
            prop_assert!(shallow.iter().all(|m| deeper.contains(m)));
            prop_assert!(deeper.iter().all(|m| all.contains(m)));
            let mut sorted = all.clone();
            sorted.sort();
            prop_assert_eq!(sorted, all);
        }
    }
}
