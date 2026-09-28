//! The worktree-aware graph cache the query commands answer from, and the `--cache` entry
//! `cruise` extracts through.
//!
//! - Architecture: [Agent surface](../../../../docs/architecture.md#agent-surface),
//!   [Performance model](../../../../docs/architecture.md#performance-model)
//! - Plan: [Wave 2, Step 13](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#213-step-13-worktree-aware-cache-and-the-eslint-plugin-2g)
//!   (the key; `can-import`, `propose`, `place` and `impact` read it; a miss re-extracts),
//!   [Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//!   ("`propose` and `place` read the worktree-aware cache ... so they answer in the same time
//!   as `can-import`")
//! - Plan: [Wave 3, Steps 1 and 2](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)
//!   (`--cache`, both strategies, `compress`; incremental extraction)
//! - Decisions: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md),
//!   [ADR-0008](../../../../docs/adr/0008-exit-code-contract.md) (the cache is only a speed-up: a
//!   doubtful entry is a miss), [ADR-0010](../../../../docs/adr/0010-crate-layout-and-extractor-boundary.md)
//!   (the extractors are asked for a subset through `rb_model::ExtractRequest`)
//! - Requirement: [FR-CLI-05](../../../../docs/prd.md#fr-cli-05)
//!
//! An entry is `.graph/cache/<key>/graph.json`, the extracted graph document before evaluation,
//! beside `key.json`, the inputs the name hashes ([`key`]: the plan's worktree root, `HEAD` and
//! configuration hash, extended with the build's version and a working-tree fingerprint). A query
//! command given `--graph FILE` reads that file; otherwise it reads the entry for its key, and on
//! a miss extracts, writes the entry (to a temporary name, then renamed, so a reader never sees
//! half an entry) and answers. An entry that does not read back as a graph document is a miss, so
//! a truncated or foreign file is re-extracted rather than answered from. After a write, only the
//! newest [`KEEP`] entries of each worktree root are kept, so `.graph/cache` does not grow with
//! every edit. `--no-cache` extracts without reading or writing.
//!
//! `cruise --cache` ([`extract_cached`]) keeps one entry per cache folder, `manifest.json` and
//! the stored extraction ([`manifest`]), keyed on the build's version, the configuration and
//! extraction settings ([`key::extraction_hash`]) and the worktree, which the query entries'
//! key also hashes; the working-tree fingerprint is replaced by the manifest's per-file inputs,
//! so an edit updates the entry instead of starting a new one. [`changes`] decides what changed
//! by the strategy. Then:
//!
//! | What changed | What runs |
//! | --- | --- |
//! | nothing an extractor reads | nothing: the stored parts are merged (`summary.cache.hit: true`) |
//! | TypeScript or Python files only | those files are read again; the rest come from the stored parts and the walk replays over them (`rb_extract_ts::extract_incremental`, `rb_extract_python::extract_incremental`) |
//! | an assembly or a PDB | the .NET graph is read again whole, so edges across assemblies stay exact; the other languages are reused |
//! | a file added or deleted, a manifest, git unable to say | everything is read again |
//!
//! Every path ends in [`pipeline::merge`], which a cold run goes through too, so the document is
//! the cold run's byte for byte.

pub mod changes;
pub mod key;
pub mod manifest;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rb_config::Config;
use rb_model::{CacheOptions, CacheStrategy, CacheSummary, ExtractError, GraphDocument};

use crate::context::Context;
use crate::pipeline::{self, Parts, Plan, Plans};

pub use key::CacheKey;

/// The graph document inside an entry.
pub const GRAPH_FILE: &str = "graph.json";
/// The inputs of an entry's name, for a reader who wants to know what it holds.
pub const KEY_FILE: &str = "key.json";
/// How many entries of one worktree root are kept after a write.
pub const KEEP: usize = 8;

/// Where a query command's graph came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// `--graph FILE`.
    File(String),
    /// The cache entry, already written.
    Hit(PathBuf),
    /// A fresh extraction, now written to the entry.
    Written(PathBuf),
    /// A fresh extraction, not cached (`--no-cache`).
    Extracted,
}

/// A graph as JSON text, with where it came from.
#[derive(Debug, Clone)]
pub struct Graph {
    /// The document's JSON.
    pub text: String,
    /// Where it came from.
    pub origin: Origin,
}

/// The graph a query command answers from: `file` when given, else the cache entry, else a fresh
/// extraction that is written to the entry unless `no_cache`.
///
/// # Errors
/// A message naming the file that cannot be read, or why the extraction cannot be trusted.
pub fn graph(
    ctx: &Context<'_>,
    config: &Config,
    file: Option<&str>,
    no_cache: bool,
) -> Result<Graph, String> {
    if let Some(file) = file {
        let path = ctx.resolve(file);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read the graph {}: {e}", path.display()))?;
        return Ok(Graph {
            text,
            origin: Origin::File(file.to_owned()),
        });
    }
    let extract = || -> Result<String, String> {
        let document = pipeline::extract(ctx, config, &[]).map_err(|e| e.to_string())?;
        serde_json::to_string(&document).map_err(|e| e.to_string())
    };
    if no_cache {
        return Ok(Graph {
            text: extract()?,
            origin: Origin::Extracted,
        });
    }
    let key = CacheKey::compute(&ctx.cwd, config);
    let directory = key.directory(&ctx.cwd);
    let cached = directory.join(GRAPH_FILE);
    if let Ok(text) = std::fs::read_to_string(&cached)
        && serde_json::from_str::<GraphDocument>(&text).is_ok()
    {
        return Ok(Graph {
            text,
            origin: Origin::Hit(directory),
        });
    }
    let text = extract()?;
    write_entry(&directory, &key, &text)?;
    prune(&ctx.cwd.join(key::CACHE_DIR), KEEP);
    Ok(Graph {
        text,
        origin: Origin::Written(directory),
    })
}

/// Writes an entry: the key, then the graph under a temporary name renamed into place.
fn write_entry(directory: &Path, key: &CacheKey, text: &str) -> Result<(), String> {
    std::fs::create_dir_all(directory)
        .map_err(|e| format!("cannot create {}: {e}", directory.display()))?;
    let mut key_text = serde_json::to_string_pretty(&key.to_json()).map_err(|e| e.to_string())?;
    key_text.push('\n');
    let key_file = directory.join(KEY_FILE);
    std::fs::write(&key_file, key_text)
        .map_err(|e| format!("cannot write {}: {e}", key_file.display()))?;
    let temporary = directory.join(format!("{GRAPH_FILE}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, text)
        .map_err(|e| format!("cannot write {}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, directory.join(GRAPH_FILE))
        .map_err(|e| format!("cannot write {}: {e}", directory.join(GRAPH_FILE).display()))
}

/// Removes all but the newest `keep` entries of each worktree root under `folder`, by the
/// modification time of their graph (an entry without one counts as oldest). An entry whose
/// `key.json` cannot be read is grouped on its own root, `""`. Removal is best effort: another
/// process may be reading or removing the same entry.
pub fn prune(folder: &Path, keep: usize) {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return;
    };
    let mut by_root: BTreeMap<String, Vec<(SystemTime, PathBuf)>> = BTreeMap::new();
    for entry in listing.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let root = std::fs::read_to_string(path.join(KEY_FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("root").and_then(|r| r.as_str()).map(str::to_owned))
            .unwrap_or_default();
        let modified = std::fs::metadata(path.join(GRAPH_FILE))
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        by_root.entry(root).or_default().push((modified, path));
    }
    for entries in by_root.values_mut() {
        // Newest first; the name breaks a tie so the choice does not depend on listing order.
        entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        for (_, path) in entries.iter().skip(keep) {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

/// The graph as a document ready to evaluate: read through [`graph`], annotations cleared.
///
/// # Errors
/// As [`graph`], or a message when the text is not a graph document.
pub fn document(
    ctx: &Context<'_>,
    config: &Config,
    file: Option<&str>,
    no_cache: bool,
) -> Result<GraphDocument, String> {
    let graph = graph(ctx, config, file, no_cache)?;
    let mut document = rb_ingest::dependency_cruiser::read(&graph.text).map_err(|e| {
        let name = match &graph.origin {
            Origin::File(f) => f.clone(),
            Origin::Hit(d) | Origin::Written(d) => d.join(GRAPH_FILE).display().to_string(),
            Origin::Extracted => "the extraction".to_owned(),
        };
        format!("{name} is not a cruise result: {e}")
    })?;
    pipeline::reset(&mut document);
    Ok(document)
}

/// How `cruise --cache` got its extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Served {
    /// Every input unchanged: the stored extraction, no file read.
    Hit,
    /// Only what changed was read: how many TypeScript and Python files, and whether the .NET
    /// graph was read again (the assembly is the unit of change, and the graph is re-read whole
    /// so edges across assemblies stay exact).
    Incremental {
        /// TypeScript and JavaScript files read again.
        typescript: usize,
        /// Python files read again.
        python: usize,
        /// Whether the .NET graph was read again.
        dotnet: bool,
    },
    /// Everything read, and why.
    Full(String),
}

impl std::fmt::Display for Served {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hit => f.write_str("from the cache"),
            Self::Incremental {
                typescript,
                python,
                dotnet,
            } => write!(
                f,
                "incremental: {typescript} TypeScript and {python} Python files read again, .NET {}",
                if *dotnet { "read again" } else { "reused" }
            ),
            Self::Full(reason) => write!(f, "in full: {reason}"),
        }
    }
}

/// The entry being written while the run evaluates: the next run needs it, this one does not.
/// Waited for by [`Writing::wait`], or when dropped, so a process never exits with it half done.
#[derive(Debug, Default)]
pub struct Writing(Option<std::thread::JoinHandle<Result<(), String>>>);

impl Writing {
    /// Waits for the write; the reason when it failed.
    pub fn wait(mut self) -> Option<String> {
        let handle = self.0.take()?;
        match handle.join() {
            Ok(result) => result.err(),
            Err(_) => Some("the cache writer stopped".to_owned()),
        }
    }
}

impl Drop for Writing {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            let _ = handle.join();
        }
    }
}

/// The extraction `cruise --cache` evaluates, and what the cache did.
#[derive(Debug)]
pub struct Cached {
    /// The document, before evaluation.
    pub document: GraphDocument,
    /// The extractors' warnings, as a cold run reports them.
    pub warnings: Vec<rb_model::Warning>,
    /// `summary.cache`.
    pub summary: CacheSummary,
    /// How it was served.
    pub served: Served,
    /// The entry being written for the next run.
    pub writing: Writing,
}

/// A full extraction, keeping the per-file states, and why.
fn full(reason: String) -> (Plans, Served) {
    (
        Plans {
            keep_file_states: true,
            ..Plans::default()
        },
        Served::Full(reason),
    )
}

/// The name a TypeScript module's source is recorded under: relative to `baseDir` when set.
fn typescript_name(scope: &changes::Scope, config: &Config, source: &str) -> String {
    let mut path = scope.base.clone();
    let base_dir = config
        .languages
        .typescript
        .base_dir
        .as_deref()
        .unwrap_or("");
    for component in Path::new(base_dir).join(source).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                path.pop();
            }
            other => path.push(other),
        }
    }
    scope.name(&path)
}

/// Whether a recorded input is an assembly or a PDB.
fn is_assembly(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| x.eq_ignore_ascii_case("dll") || x.eq_ignore_ascii_case("pdb"))
}

/// What the stored parts (with their per-file states) and the changes call for: a partial
/// extraction or a full one, or `None` for a hit. A change to a file no extractor read (a file of
/// the presence set whose content alone changed) changes nothing extracted.
fn decide(
    config: &Config,
    scope: &changes::Scope,
    parts: &Parts,
    found: &changes::Changes,
) -> Option<(Plans, Served)> {
    if let Some(reason) = &found.structural {
        return Some(full(reason.clone()));
    }
    let typescript: BTreeMap<String, &String> = parts
        .typescript
        .iter()
        .flat_map(|p| p.files.keys())
        .map(|source| (typescript_name(scope, config, source), source))
        .collect();
    let python: BTreeMap<String, &String> = parts
        .python
        .iter()
        .flat_map(|p| p.files.keys())
        .map(|file| (scope.name(&scope.base.join(file)), file))
        .collect();
    let (mut ts_changed, mut py_changed, mut dotnet) = (Vec::new(), Vec::new(), false);
    for name in &found.modified {
        if changes::is_manifest(name) {
            return Some(full(format!("{name} changed")));
        }
        if is_assembly(name) {
            dotnet = true;
        } else if let Some(source) = typescript.get(name) {
            ts_changed.push(PathBuf::from(source.as_str()));
        } else if let Some(file) = python.get(name) {
            py_changed.push(PathBuf::from(file.as_str()));
        }
    }
    if ts_changed.is_empty() && py_changed.is_empty() && !dotnet {
        return None;
    }
    let plan = |changed: &[PathBuf],
                files: &BTreeMap<String, &String>,
                part: &Option<rb_model::Extraction>| {
        if changed.is_empty() {
            return Plan::Reuse(part.clone());
        }
        Plan::Incremental(rb_model::ExtractRequest {
            changed: changed.to_vec(),
            unchanged: files
                .values()
                .map(|s| PathBuf::from(s.as_str()))
                .filter(|s| !changed.contains(s))
                .collect(),
            previous: part.clone().unwrap_or_default(),
        })
    };
    let plans = Plans {
        typescript: plan(&ts_changed, &typescript, &parts.typescript),
        python: plan(&py_changed, &python, &parts.python),
        dotnet: if dotnet {
            Plan::Full
        } else {
            Plan::Reuse(parts.dotnet.clone())
        },
        keep_file_states: true,
    };
    Some((
        plans,
        Served::Incremental {
            typescript: ts_changed.len(),
            python: py_changed.len(),
            dotnet,
        },
    ))
}

/// Every input an entry written for `parts` records (the module doc of [`changes`] lists them).
fn inputs(
    cwd: &Path,
    config: &Config,
    scope: &changes::Scope,
    parts: &Parts,
    strategy: CacheStrategy,
) -> BTreeSet<String> {
    let mut inputs: BTreeSet<String> = BTreeSet::new();
    let typescript: BTreeSet<String> = parts
        .typescript
        .iter()
        .flat_map(|p| p.files.keys())
        .map(|source| typescript_name(scope, config, source))
        .collect();
    inputs.extend(changes::package_manifests(
        scope,
        typescript.iter().map(String::as_str),
    ));
    inputs.extend(typescript);
    inputs.extend(
        parts
            .python
            .iter()
            .flat_map(|p| p.files.keys())
            .map(|file| scope.name(&scope.base.join(file))),
    );
    #[cfg(feature = "extract-dotnet")]
    if parts.dotnet.is_some() {
        let options = config.languages.dotnet.clone().unwrap_or_default();
        if let Ok(assemblies) = rb_extract_dotnet::assembly_inputs(cwd, &options) {
            inputs.extend(assemblies.iter().map(|p| scope.name(p)));
        }
    }
    inputs.extend(
        key::manifest_paths(&scope.root, cwd, config)
            .iter()
            .filter(|p| p.is_file())
            .map(|p| scope.name(p)),
    );
    inputs.extend(changes::presence(strategy, scope));
    inputs
}

/// Everything the background write of an entry needs, owned.
struct Pending {
    cwd: PathBuf,
    config: Config,
    scope: changes::Scope,
    folder: PathBuf,
    options: CacheOptions,
    manifest: manifest::Manifest,
    verified: BTreeMap<String, String>,
    parts: Parts,
}

impl Pending {
    /// Records the inputs and writes the entry.
    fn write(self) -> Result<(), String> {
        let recorded = inputs(
            &self.cwd,
            &self.config,
            &self.scope,
            &self.parts,
            self.options.strategy,
        );
        let (hashes, stamps) = changes::record(
            &self.scope,
            &recorded,
            &self.verified,
            self.options.strategy,
        );
        let manifest = manifest::Manifest {
            inputs: hashes,
            stamps,
            ..self.manifest
        };
        manifest::store(&self.folder, manifest, &self.parts, &self.options)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// The extraction `cruise` evaluates under `--cache` or `options.cache`
/// ([Wave 3, Steps 1 and 2](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)):
/// the entry in `options.folder` when every input is unchanged, a partial extraction when only
/// files an extractor can re-read alone changed, a full one otherwise; then the entry is
/// written again for the next run, on a thread of its own while this run evaluates
/// ([`Cached::writing`]). The document is what a cold run extracts, byte for byte: the parts are
/// the ones a cold run would produce and [`pipeline::merge`] joins them as it does for a cold
/// run.
///
/// # Errors
/// [`ExtractError`] from an extractor, as a cold run fails; never from the cache itself, whose
/// every doubtful entry is a miss and whose write failure [`Writing::wait`] reports.
pub fn extract_cached(
    ctx: &Context<'_>,
    config: &Config,
    paths: &[String],
    options: &CacheOptions,
) -> Result<Cached, ExtractError> {
    let root = key::worktree_root(&ctx.cwd);
    let head = Some(key::head(&root)).filter(|h| !h.is_empty());
    let folder = ctx.resolve(&options.folder);
    let scope = changes::Scope {
        base: ctx.cwd.canonicalize().unwrap_or_else(|_| ctx.cwd.clone()),
        root: root.clone(),
        head: head.clone(),
        extra_extensions: config
            .languages
            .typescript
            .extra_extensions_to_scan
            .clone()
            .unwrap_or_default(),
        cache_folder: folder.canonicalize().unwrap_or_else(|_| folder.clone()),
    };
    let fresh = manifest::Manifest {
        tool_version: key::VERSION.to_owned(),
        config_hash: key::extraction_hash(config, &root, &ctx.cwd, paths),
        worktree: key::slashed(&root),
        head,
        strategy: options.strategy,
        inputs: BTreeMap::new(),
        stamps: BTreeMap::new(),
        extraction: String::new(),
    };
    let summary = |served: &Served| CacheSummary {
        hit: *served == Served::Hit,
        strategy: options.strategy,
    };
    let extract = |(plans, served): (Plans, Served), verified: BTreeMap<String, String>| {
        let parts = pipeline::extract_parts(ctx, config, paths, &plans)?;
        let (document, warnings) = pipeline::merge(config, &parts)?;
        let pending = Pending {
            cwd: ctx.cwd.clone(),
            config: config.clone(),
            scope: scope.clone(),
            folder: folder.clone(),
            options: options.clone(),
            manifest: fresh.clone(),
            verified,
            parts,
        };
        Ok(Cached {
            document,
            warnings,
            summary: summary(&served),
            served,
            writing: Writing(Some(std::thread::spawn(move || pending.write()))),
        })
    };
    let entry = match manifest::load(&folder, &fresh.key(), options) {
        Ok(entry) => entry,
        Err(miss) => return extract(full(miss.to_string()), BTreeMap::new()),
    };
    let found = changes::detect(&entry.manifest, options.strategy, &scope);
    if !found.is_empty() {
        let decision = match entry.with_states() {
            Ok(parts) => decide(config, &scope, &parts, &found),
            Err(miss) => Some(full(miss.to_string())),
        };
        if let Some(decision) = decision {
            return extract(decision, found.hashes);
        }
    }
    let (document, warnings) = pipeline::merge(config, &entry.parts)?;
    let refreshed = manifest::Manifest {
        inputs: found.hashes,
        stamps: if options.strategy == CacheStrategy::Metadata {
            found.stamps
        } else {
            BTreeMap::new()
        },
        extraction: entry.manifest.extraction.clone(),
        ..fresh.clone()
    };
    let writing = if refreshed == entry.manifest {
        Writing::default()
    } else {
        Writing(Some(std::thread::spawn(move || {
            manifest::store_manifest(&folder, &refreshed).map_err(|e| e.to_string())
        })))
    };
    Ok(Cached {
        document,
        warnings,
        summary: summary(&Served::Hit),
        served: Served::Hit,
        writing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context<'a>(dir: &std::path::Path, stdin: &'a mut &'static [u8]) -> Context<'a> {
        Context {
            cwd: dir.to_path_buf(),
            stdin,
            today: chrono::NaiveDate::default(),
            timestamp: String::new(),
            color_terminal: false,
        }
    }

    fn scope(dir: &Path) -> changes::Scope {
        changes::Scope {
            base: dir.to_path_buf(),
            root: dir.to_path_buf(),
            head: None,
            extra_extensions: Vec::new(),
            cache_folder: dir.join(".graph/cache"),
        }
    }

    fn part(sources: &[&str]) -> rb_model::Extraction {
        rb_model::Extraction {
            modules: sources.iter().map(|s| rb_model::Module::new(*s)).collect(),
            files: sources
                .iter()
                .map(|s| ((*s).to_owned(), rb_model::FileState::default()))
                .collect(),
            ..rb_model::Extraction::default()
        }
    }

    #[test]
    fn how_a_run_was_served_reads_as_a_sentence() {
        assert_eq!(Served::Hit.to_string(), "from the cache");
        assert_eq!(
            Served::Incremental {
                typescript: 2,
                python: 1,
                dotnet: true
            }
            .to_string(),
            "incremental: 2 TypeScript and 1 Python files read again, .NET read again"
        );
        assert_eq!(
            Served::Full("no entry".into()).to_string(),
            "in full: no entry"
        );
        assert_eq!(Writing::default().wait(), None);
        let failed = Writing(Some(std::thread::spawn(|| Err("disk full".to_owned()))));
        assert_eq!(failed.wait().as_deref(), Some("disk full"));
        drop(Writing(Some(std::thread::spawn(|| Ok(())))));
    }

    #[test]
    fn names_follow_base_dir_and_assemblies_are_told_by_extension() {
        let dir = Path::new("/repo");
        let config = Config::default();
        assert_eq!(
            typescript_name(&scope(dir), &config, "src/a.ts"),
            "src/a.ts"
        );
        let mut based = Config::default();
        based.languages.typescript.base_dir = Some("web/./app".into());
        assert_eq!(
            typescript_name(&scope(dir), &based, "src/a.ts"),
            "web/app/src/a.ts"
        );
        based.languages.typescript.base_dir = Some("web".into());
        assert_eq!(
            typescript_name(&scope(dir), &based, "../lib/b.ts"),
            "lib/b.ts"
        );
        for (name, assembly) in [
            ("bin/A.dll", true),
            ("bin/A.PDB", true),
            ("src/a.ts", false),
            ("dll", false),
        ] {
            assert_eq!(is_assembly(name), assembly, "{name}");
        }
    }

    #[test]
    fn the_changes_decide_what_is_read_again() {
        let dir = Path::new("/repo");
        let config = Config::default();
        let parts = Parts {
            typescript: Some(part(&["src/a.ts", "src/b.ts"])),
            dotnet: Some(rb_model::Extraction::default()),
            python: Some(part(&["app/m.py"])),
        };
        let changed = |names: &[&str]| changes::Changes {
            modified: names.iter().map(|n| (*n).to_owned()).collect(),
            ..changes::Changes::default()
        };
        assert!(decide(&config, &scope(dir), &parts, &changed(&[])).is_none());
        assert!(
            decide(&config, &scope(dir), &parts, &changed(&["notes/x.json"])).is_none(),
            "a file no extractor read"
        );
        let structural = changes::Changes {
            structural: Some("src/c.ts was added".into()),
            ..changed(&["src/a.ts"])
        };
        let Some((plans, served)) = decide(&config, &scope(dir), &parts, &structural) else {
            unreachable!("a structural change extracts");
        };
        assert_eq!(served, Served::Full("src/c.ts was added".into()));
        assert_eq!(plans.typescript, Plan::Full);
        assert!(plans.keep_file_states);
        let manifest = decide(
            &config,
            &scope(dir),
            &parts,
            &changed(&["web/package.json"]),
        );
        assert_eq!(
            manifest.map(|(_, s)| s),
            Some(Served::Full("web/package.json changed".into()))
        );
        let Some((plans, served)) = decide(&config, &scope(dir), &parts, &changed(&["src/b.ts"]))
        else {
            unreachable!("an edit extracts");
        };
        assert_eq!(
            served,
            Served::Incremental {
                typescript: 1,
                python: 0,
                dotnet: false
            }
        );
        let Plan::Incremental(request) = &plans.typescript else {
            unreachable!("{:?}", plans.typescript);
        };
        assert_eq!(request.changed, [PathBuf::from("src/b.ts")]);
        assert_eq!(request.unchanged, [PathBuf::from("src/a.ts")]);
        assert_eq!(plans.python, Plan::Reuse(parts.python.clone()));
        assert_eq!(plans.dotnet, Plan::Reuse(parts.dotnet.clone()));
        let Some((plans, served)) = decide(
            &config,
            &scope(dir),
            &parts,
            &changed(&["app/m.py", "bin/A.dll"]),
        ) else {
            unreachable!("an edit extracts");
        };
        assert_eq!(
            served,
            Served::Incremental {
                typescript: 0,
                python: 1,
                dotnet: true
            }
        );
        assert!(matches!(plans.python, Plan::Incremental(_)));
        assert_eq!(plans.dotnet, Plan::Full);
        assert_eq!(plans.typescript, Plan::Reuse(parts.typescript.clone()));
    }

    #[test]
    fn a_named_file_is_read_and_a_missing_one_is_named() {
        let dir = std::env::temp_dir().join(format!("rb-cache-file-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("g.json"), r#"{"modules":[],"summary":{}}"#);
        let mut empty: &'static [u8] = &[];
        let ctx = context(&dir, &mut empty);
        let config = Config::default();
        let read = graph(&ctx, &config, Some("g.json"), false);
        assert!(read.is_ok_and(|g| g.origin == Origin::File("g.json".into())));
        let missing = graph(&ctx, &config, Some("nope.json"), false);
        assert!(missing.is_err_and(|m| m.contains("nope.json")));
        let _ = std::fs::write(dir.join("bad.json"), "[1]");
        assert!(
            document(&ctx, &config, Some("bad.json"), false).is_err_and(|m| m.contains("bad.json"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_entry_is_a_miss() {
        let dir = std::env::temp_dir().join(format!("rb-cache-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let config = Config::default();
        let key = CacheKey::compute(&dir, &config);
        let _ = std::fs::create_dir_all(key.directory(&dir));
        let _ = std::fs::write(key.directory(&dir).join(GRAPH_FILE), "{ half");
        let mut empty: &'static [u8] = &[];
        let ctx = context(&dir, &mut empty);
        // Nothing to extract in an empty folder, so the miss surfaces the extraction's reason.
        assert!(graph(&ctx, &config, None, false).is_err());
        // Valid JSON that is not a graph document is a miss too, never an answer.
        for foreign in [
            "[1]",
            r#"{"modules":7}"#,
            r#"{"modules":[{"source":3}]}"#,
            "null",
        ] {
            let _ = std::fs::write(key.directory(&dir).join(GRAPH_FILE), foreign);
            assert!(graph(&ctx, &config, None, false).is_err(), "{foreign}");
        }
        let empty_graph = serde_json::to_string(&GraphDocument::default()).unwrap_or_default();
        let _ = std::fs::write(key.directory(&dir).join(GRAPH_FILE), empty_graph);
        let hit = graph(&ctx, &config, None, false);
        assert!(hit.is_ok_and(|g| g.origin == Origin::Hit(key.directory(&dir))));
        assert!(
            graph(&ctx, &config, None, true).is_err(),
            "--no-cache never reads"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pruning_keeps_the_newest_entries_of_each_root() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("rb-cache-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let base = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let entry = |name: &str, root: &str, age: u64| -> std::io::Result<()> {
            let folder = dir.join(name);
            std::fs::create_dir_all(&folder)?;
            std::fs::write(folder.join(KEY_FILE), format!("{{\"root\":\"{root}\"}}"))?;
            let graph = folder.join(GRAPH_FILE);
            std::fs::write(&graph, "{}")?;
            std::fs::File::options()
                .write(true)
                .open(&graph)?
                .set_modified(base + std::time::Duration::from_secs(age))
        };
        for n in 0..10u64 {
            entry(&format!("a{n:02}"), "/one", n)?;
        }
        for n in 0..3u64 {
            entry(&format!("b{n:02}"), "/two", n)?;
        }
        std::fs::create_dir_all(dir.join("stray"))?;
        std::fs::write(dir.join("loose-file"), "")?;
        prune(&dir, 8);
        let mut left: Vec<String> = std::fs::read_dir(&dir)?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "a02",
                "a03",
                "a04",
                "a05",
                "a06",
                "a07",
                "a08",
                "a09",
                "b00",
                "b01",
                "b02",
                "loose-file",
                "stray"
            ],
            "the two oldest of /one go; /two and the rootless entry are under the limit"
        );
        prune(&dir.join("absent"), 8);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
