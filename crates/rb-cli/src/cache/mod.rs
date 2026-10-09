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
//! | a CoffeeScript or LiveScript file, under `--sidecar node` | the sidecar extracts the changed files again, the rest are reused as TypeScript files are (the flag is in the key, so an entry written without it is never reused with it, nor the reverse) |
//! | an assembly or a PDB | the .NET graph is read again whole, so edges across assemblies stay exact; the other languages are reused |
//! | anything else recorded: a file added or deleted, a manifest or any other configuration file the run read, a folder's entries, a probe (the .NET assemblies, the Python environment), an input recorded as unsettled because it moved during the run, git unable to say | everything is read again ([`changes`] lists the inputs) |
//!
//! Every path ends in [`pipeline::merge`], which a cold run goes through too, so the document is
//! the cold run's byte for byte. On a full hit the entry can also hold the evaluated run
//! ([`evaluated`]), keyed on the extraction and everything evaluation reads; when its key
//! matches, the extraction is not even read and only the reporter runs.

pub mod changes;
pub mod evaluated;
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

/// The graph a query command answers from: `file` when given, else a server's warm graph, else
/// the cache entry, else a fresh extraction that is written to the entry unless `no_cache`.
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
    if let Some(warm) = ctx.warm
        && let Some(text) = warm.text()
    {
        let name = match warm.source() {
            Some(crate::serve::graph::Source::File(name)) => name.clone(),
            _ => "the graph the server re-checked".to_owned(),
        };
        return Ok(Graph {
            text: text.to_owned(),
            origin: Origin::File(name),
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

/// A writer thread's result: the digest of the extraction the entry holds.
type Written = std::thread::JoinHandle<Result<String, String>>;

/// The entry being written while the run evaluates: the next run needs it, this one does not.
/// Waited for by [`Writing::wait`], or when dropped, so a process never exits with it half done.
#[derive(Debug, Default)]
pub struct Writing {
    /// The writer, when anything is being written.
    handle: Option<Written>,
    /// The extraction's digest when nothing needs writing to know it.
    known: Option<String>,
}

impl Writing {
    /// A writer on its own thread.
    fn spawn(write: impl FnOnce() -> Result<String, String> + Send + 'static) -> Self {
        Self {
            handle: Some(std::thread::spawn(write)),
            known: None,
        }
    }

    /// Nothing to write; the entry holds the extraction with `digest`.
    fn written(digest: String) -> Self {
        Self {
            handle: None,
            known: Some(digest),
        }
    }

    /// Waits for the write: the digest of the extraction the entry now holds (none when the run
    /// wrote no entry), or the reason it failed.
    ///
    /// # Errors
    /// Why the entry was not written.
    pub fn wait(mut self) -> Result<Option<String>, String> {
        match self.handle.take() {
            None => Ok(self.known.take()),
            Some(handle) => match handle.join() {
                Ok(result) => result.map(Some),
                Err(_) => Err("the cache writer stopped".to_owned()),
            },
        }
    }

    /// After this write, stores `verdict` as the evaluation of the written extraction under
    /// `partial` ([`evaluated::key`]), in `folder`.
    #[must_use]
    pub fn then_remember(
        self,
        folder: PathBuf,
        partial: String,
        verdict: evaluated::Verdict,
        compressed: bool,
    ) -> Self {
        Self::spawn(move || {
            let Some(extraction) = self.wait()? else {
                return Err("no extraction was written".to_owned());
            };
            let key = evaluated::key(&partial, &extraction);
            evaluated::store(&folder, &key, &verdict, compressed).map_err(|e| e.to_string())?;
            Ok(extraction)
        })
    }
}

impl Writing {
    /// After this write, stores the rendered `output` of a served verdict and its `tail` under
    /// `key` ([`evaluated::render_key`]), in `folder`.
    #[must_use]
    pub fn then_render(
        self,
        folder: PathBuf,
        key: String,
        output: String,
        tail: evaluated::Tail,
    ) -> Self {
        Self::spawn(move || {
            let written = self.wait()?;
            evaluated::store_rendered(&folder, &key, &output, tail).map_err(|e| e.to_string())?;
            written.ok_or_else(|| "no extraction was written".to_owned())
        })
    }
}

impl Drop for Writing {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// What the cache gave a run.
#[derive(Debug)]
pub enum Content {
    /// The extraction, still to be evaluated, and the extractors' warnings.
    Extracted(Box<GraphDocument>, Vec<rb_model::Warning>),
    /// The evaluated run of an unchanged extraction under an unchanged evaluation key, stored
    /// and not yet read.
    Evaluated(evaluated::Stored),
}

/// The extraction `cruise --cache` evaluates, or the evaluated run, and what the cache did.
#[derive(Debug)]
pub struct Cached {
    /// What the cache gave.
    pub content: Content,
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
/// extraction or a full one, or the parts back for a hit. Only a changed source is read again
/// alone and only an assembly reads the .NET graph again; any other changed input is structural.
/// The parts are taken, so an earlier extraction moves into its request and is not copied: on a
/// large repository the copy was a tenth of a warm run (NFR-PERF-02).
fn decide(
    config: &Config,
    scope: &changes::Scope,
    parts: Parts,
    found: &changes::Changes,
) -> Decided {
    if let Some(reason) = &found.structural {
        return Decided::extract(full(reason.clone()));
    }
    let typescript: BTreeMap<String, String> = parts
        .typescript
        .iter()
        .flat_map(|p| p.files.keys())
        .map(|source| (typescript_name(scope, config, source), source.clone()))
        .collect();
    let python: BTreeMap<String, String> = parts
        .python
        .iter()
        .flat_map(|p| p.files.keys())
        .map(|file| (scope.name(&scope.base.join(file)), file.clone()))
        .collect();
    // Source mode keeps each `.cs` file's parse; compiled mode keeps none.
    let dotnet_sources: BTreeMap<String, String> = parts
        .dotnet
        .iter()
        .flat_map(|p| p.files.keys())
        .map(|file| (scope.name(&scope.base.join(file)), file.clone()))
        .collect();
    let (mut ts_changed, mut py_changed, mut dotnet) = (Vec::new(), Vec::new(), false);
    let mut cs_changed = Vec::new();
    // A source is read again alone and an assembly reads the .NET graph again; any other input
    // (a manifest, a tsconfig and its chain, a Babel configuration, a project file, a folder's
    // entries, a file of the presence set) can move what an unchanged file resolves to.
    for name in &found.modified {
        if is_assembly(name) {
            dotnet = true;
        } else if let Some(source) = typescript.get(name) {
            ts_changed.push(PathBuf::from(source.as_str()));
        } else if let Some(file) = python.get(name) {
            py_changed.push(PathBuf::from(file.as_str()));
        } else if let Some(file) = dotnet_sources.get(name) {
            cs_changed.push(PathBuf::from(file.as_str()));
            dotnet = true;
        } else {
            return Decided::extract(full(format!("{name} changed")));
        }
    }
    if ts_changed.is_empty() && py_changed.is_empty() && !dotnet {
        return Decided::Hit(Box::new(parts));
    }
    let plan = |changed: &[PathBuf],
                files: &BTreeMap<String, String>,
                part: Option<rb_model::Extraction>| {
        if changed.is_empty() {
            return Plan::Reuse(part);
        }
        Plan::Incremental(rb_model::ExtractRequest {
            changed: changed.to_vec(),
            unchanged: files
                .values()
                .map(|s| PathBuf::from(s.as_str()))
                .filter(|s| !changed.contains(s))
                .collect(),
            previous: part.unwrap_or_default(),
            // The cache sees a folder's entries only as one of its inputs, and keeps no walk.
            walk_unchanged: false,
        })
    };
    let plans = Plans {
        typescript: plan(&ts_changed, &typescript, parts.typescript),
        python: plan(&py_changed, &python, parts.python),
        dotnet: if !cs_changed.is_empty() {
            plan(&cs_changed, &dotnet_sources, parts.dotnet)
        } else if dotnet {
            Plan::Full
        } else {
            Plan::Reuse(parts.dotnet)
        },
        keep_file_states: true,
        keep_walk: false,
    };
    Decided::extract((
        plans,
        Served::Incremental {
            typescript: ts_changed.len(),
            python: py_changed.len(),
            dotnet,
        },
    ))
}

/// What a stored entry and the changes found call for: the entry's parts when nothing changed
/// or nothing extracted did, else what to extract. An entry whose states do not parse is
/// extracted in full.
fn stored(
    entry: manifest::Entry,
    config: &Config,
    scope: &changes::Scope,
    found: &changes::Changes,
) -> Decided {
    if found.is_empty() {
        return Decided::Hit(Box::new(entry.parts));
    }
    entry.with_states().map_or_else(
        |miss| Decided::extract(full(miss.to_string())),
        |parts| decide(config, scope, parts, found),
    )
}

/// What [`decide`] found: something to extract, or the stored parts, which answer as they are.
enum Decided {
    /// The plans to extract with, and how the run is served.
    Extract(Box<(Plans, Served)>),
    /// Nothing extracted changed: the parts back.
    Hit(Box<Parts>),
}

impl Decided {
    fn extract(decision: (Plans, Served)) -> Self {
        Self::Extract(Box::new(decision))
    }

    /// The plans and how the run is served, when something is extracted.
    #[cfg(test)]
    fn extraction(self) -> Option<(Plans, Served)> {
        match self {
            Self::Extract(decision) => Some(*decision),
            Self::Hit(_) => None,
        }
    }
}

/// The files that change nothing when absent and must be seen when they appear: the manifests
/// the key names, and the package managers' records of an installation under `node_modules`,
/// in the worktree root and the working directory.
fn optional_inputs(cwd: &Path, config: &Config, scope: &changes::Scope) -> BTreeSet<String> {
    let mut optional: BTreeSet<String> = key::manifest_paths(&scope.root, cwd, config)
        .iter()
        .map(|p| scope.name(p))
        .collect();
    for folder in [scope.root.as_path(), cwd] {
        for marker in [
            "node_modules/.package-lock.json",
            "node_modules/.modules.yaml",
            "node_modules/.yarn-state.yml",
        ] {
            optional.insert(scope.name(&folder.join(marker)));
        }
    }
    optional
}

/// Every input an entry written for `parts` records (the module doc of [`changes`] lists them),
/// and those of them recorded only for their presence.
fn inputs(
    cwd: &Path,
    config: &Config,
    scope: &changes::Scope,
    parts: &Parts,
    strategy: CacheStrategy,
) -> (BTreeSet<String>, BTreeSet<String>) {
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
    let imports: Vec<(String, &str)> = parts
        .typescript
        .iter()
        .flat_map(|p| &p.modules)
        .filter(|m| m.language.is_some())
        .flat_map(|m| {
            let name = typescript_name(scope, config, &m.source);
            m.dependencies
                .iter()
                .map(move |d| (name.clone(), d.module.as_str()))
        })
        .collect();
    inputs.extend(changes::target_folders(
        scope,
        imports
            .iter()
            .map(|(file, specifier)| (file.as_str(), *specifier)),
    ));
    inputs.extend(typescript);
    #[cfg(feature = "extract-ts")]
    if parts.typescript.is_some() {
        inputs.extend(
            rb_extract_ts::configuration_files(&config.languages.typescript, cwd)
                .iter()
                .map(|p| scope.name(p)),
        );
    }
    inputs.extend(
        parts
            .python
            .iter()
            .flat_map(|p| p.files.keys())
            .map(|file| scope.name(&scope.base.join(file))),
    );
    // Source mode's inputs are the `.cs` files it read; compiled mode's the built assemblies.
    inputs.extend(
        parts
            .dotnet
            .iter()
            .flat_map(|p| p.files.keys())
            .map(|file| scope.name(&scope.base.join(file))),
    );
    #[cfg(feature = "extract-dotnet")]
    if parts.dotnet.is_some() {
        let options = config.languages.dotnet.clone().unwrap_or_default();
        if options.mode() == rb_model::DotnetMode::Compiled
            && let Ok(assemblies) = rb_extract_dotnet::assembly_inputs(cwd, &options)
        {
            inputs.extend(assemblies.iter().map(|p| scope.name(p)));
        }
        // Source mode kept the solution and project files it read.
        let projects = pipeline::dotnet_project_files(cwd, &options, parts.dotnet.as_ref());
        inputs.extend(projects.iter().map(|p| scope.name(p)));
    }
    inputs.extend(
        key::manifest_paths(&scope.root, cwd, config)
            .iter()
            .filter(|p| p.is_file())
            .map(|p| scope.name(p)),
    );
    let present = changes::presence(strategy, scope, &inputs);
    // A manifest by name is never only watched, wherever it sits.
    let watched: BTreeSet<String> = present
        .difference(&inputs)
        .filter(|name| !changes::is_manifest(name))
        .cloned()
        .collect();
    inputs.extend(present);
    (inputs, watched)
}

/// The probes of a run ([`changes`]): computed before anything is extracted, so an environment
/// that changes during the run is seen by the next one.
#[cfg_attr(
    not(any(feature = "extract-dotnet", feature = "extract-python")),
    expect(
        unused_variables,
        unused_mut,
        reason = "only the .NET and Python extractors have probes"
    )
)]
pub fn probes(ctx: &Context<'_>, config: &Config) -> BTreeMap<String, String> {
    let mut probes = BTreeMap::new();
    // Source mode reads no assembly, so a build since the entry was written changes nothing.
    #[cfg(feature = "extract-dotnet")]
    if pipeline::dotnet_enabled(ctx, config) && !pipeline::dotnet_source_mode(config) {
        let options = config.languages.dotnet.clone().unwrap_or_default();
        let value = match rb_extract_dotnet::assembly_inputs(&ctx.cwd, &options) {
            Ok(found) => found
                .iter()
                .map(|p| key::slashed(p))
                .collect::<Vec<_>>()
                .join("\n"),
            Err(e) => format!("error: {e}"),
        };
        probes.insert(".NET assemblies".to_owned(), value);
    }
    #[cfg(feature = "extract-python")]
    if pipeline::python_enabled(ctx, config) {
        let options = config.languages.python.clone().unwrap_or_default();
        let virtual_env = std::env::var_os("VIRTUAL_ENV").map(PathBuf::from);
        let value = rb_extract_python::environment(&ctx.cwd, &options, virtual_env.as_deref())
            .unwrap_or_else(|e| format!("error: {e}"));
        probes.insert("Python environment".to_owned(), value);
    }
    probes
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
    /// When the run started, nanoseconds since the epoch: an input modified after it is
    /// recorded as unsettled.
    started: u64,
}

impl Pending {
    /// Records the inputs and writes the entry.
    fn write(self) -> Result<String, String> {
        let (recorded, watched) = inputs(
            &self.cwd,
            &self.config,
            &self.scope,
            &self.parts,
            self.options.strategy,
        );
        let optional = optional_inputs(&self.cwd, &self.config, &self.scope);
        #[cfg(test)]
        tests::between_extraction_and_record(&self.folder);
        let (hashes, stamps) = changes::record(
            &self.scope,
            &recorded,
            &optional,
            &self.verified,
            self.options.strategy,
            self.started,
        );
        let manifest = manifest::Manifest {
            inputs: hashes,
            stamps,
            watched,
            ..self.manifest
        };
        manifest::store(&self.folder, manifest, &self.parts, &self.options)
            .map(|written| written.extraction)
            .map_err(|e| e.to_string())
    }
}

/// Where a `--cache` run is: the working directory, the worktree and its `HEAD`, the extensions
/// scanned and the cache folder.
fn scope_of(ctx: &Context<'_>, config: &Config, folder: &Path) -> changes::Scope {
    let root = key::worktree_root(&ctx.cwd);
    changes::Scope {
        // Without Windows' verbatim prefix, as the root from git is: a path joined to the root is
        // then inside the base by its spelling, whether or not the file still exists.
        base: rb_model::without_verbatim(
            &ctx.cwd.canonicalize().unwrap_or_else(|_| ctx.cwd.clone()),
        ),
        head: Some(key::head(&root)).filter(|h| !h.is_empty()),
        root,
        extra_extensions: {
            let mut extra = config
                .languages
                .typescript
                .extra_extensions_to_scan
                .clone()
                .unwrap_or_default();
            // In source mode a `.cs` file appearing or disappearing changes the .NET graph.
            if pipeline::dotnet_source_mode(config) {
                extra.push(".cs".to_owned());
            }
            extra
        },
        cache_folder: rb_model::without_verbatim(
            &folder
                .canonicalize()
                .unwrap_or_else(|_| folder.to_path_buf()),
        ),
    }
}

/// The manifest of a hit rewritten when its stamps, hashes or `HEAD` moved; nothing written
/// when it is as recorded.
fn refresh(
    folder: PathBuf,
    recorded: &manifest::Manifest,
    refreshed: &manifest::Manifest,
) -> Writing {
    if refreshed == recorded {
        return Writing::written(recorded.extraction.clone());
    }
    let refreshed = refreshed.clone();
    Writing::spawn(move || {
        manifest::store_manifest(&folder, &refreshed)
            .map(|()| refreshed.extraction)
            .map_err(|e| e.to_string())
    })
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
/// With `evaluation` (the [`evaluated::partial_key`] of the run), a full hit first looks for the
/// evaluated run stored under that key and this extraction, and names it without reading the
/// extraction ([`Content::Evaluated`]); the caller reads it, or its rendered output.
///
/// # Errors
/// [`ExtractError`] from an extractor, as a cold run fails; never from the cache itself, whose
/// every doubtful entry is a miss and whose write failure [`Writing::wait`] reports.
pub fn extract_cached(
    ctx: &Context<'_>,
    config: &Config,
    paths: &[String],
    options: &CacheOptions,
    evaluation: Option<&str>,
) -> Result<Cached, ExtractError> {
    // Before anything is looked at: an input modified after this is recorded as unsettled.
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
    let folder = ctx.resolve(&options.folder);
    let scope = scope_of(ctx, config, &folder);
    let (root, head) = (scope.root.clone(), scope.head.clone());
    let probes = probes(ctx, config);
    let fresh = manifest::Manifest {
        tool_version: key::VERSION.to_owned(),
        config_hash: key::extraction_hash(config, &root, &ctx.cwd, paths),
        worktree: key::slashed(&root),
        head,
        strategy: options.strategy,
        inputs: BTreeMap::new(),
        stamps: BTreeMap::new(),
        extraction: String::new(),
        probes: probes.clone(),
        watched: BTreeSet::new(),
    };
    let summary = |served: &Served| CacheSummary {
        hit: *served == Served::Hit,
        strategy: options.strategy,
    };
    let extract = |(plans, served): (Plans, Served), verified: BTreeMap<String, String>| {
        let parts = pipeline::extract_parts(ctx, config, paths, plans)?;
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
            started,
        };
        Ok(Cached {
            content: Content::Extracted(Box::new(document), warnings),
            summary: summary(&served),
            served,
            writing: Writing::spawn(move || pending.write()),
        })
    };
    let recorded = match manifest::load_manifest(&folder, &fresh.key(), options) {
        Ok(recorded) => recorded,
        Err(miss) => return extract(full(miss.to_string()), BTreeMap::new()),
    };
    let mut found = changes::detect(&recorded, options.strategy, &scope, &probes);
    // A file recorded only for its presence changes nothing extracted when its bytes change.
    found
        .modified
        .retain(|name| !recorded.watched.contains(name));
    let refreshed = manifest::Manifest {
        inputs: found.hashes.clone(),
        stamps: if options.strategy == CacheStrategy::Metadata {
            found.stamps.clone()
        } else {
            BTreeMap::new()
        },
        extraction: recorded.extraction.clone(),
        watched: recorded.watched.clone(),
        ..fresh.clone()
    };
    let refresh = |folder: PathBuf| refresh(folder, &recorded, &refreshed);
    if found.is_empty()
        && let Some(partial) = evaluation
        && let key = evaluated::key(partial, &recorded.extraction)
        && evaluated::stored_under(&folder, &key)
    {
        return Ok(Cached {
            content: Content::Evaluated(evaluated::Stored {
                folder: folder.clone(),
                key,
                compressed: options.compressed(),
            }),
            summary: summary(&Served::Hit),
            served: Served::Hit,
            writing: refresh(folder.clone()),
        });
    }
    let entry = match manifest::load_extraction(&folder, recorded.clone(), options) {
        Ok(entry) => entry,
        Err(miss) => return extract(full(miss.to_string()), BTreeMap::new()),
    };
    let parts = match stored(entry, config, &scope, &found) {
        Decided::Extract(decision) => return extract(*decision, found.hashes),
        Decided::Hit(parts) => *parts,
    };
    let (document, warnings) = pipeline::merge(config, &parts)?;
    Ok(Cached {
        content: Content::Extracted(Box::new(document), warnings),
        summary: summary(&Served::Hit),
        served: Served::Hit,
        writing: refresh(folder.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test's action between an extraction and the recording of its inputs, by cache folder.
    type Hook = (PathBuf, Box<dyn Fn() + Send>);

    static HOOKS: std::sync::Mutex<Vec<Hook>> = std::sync::Mutex::new(Vec::new());

    /// Runs the action a test registered for `folder`, the moment the writer is about to record.
    pub(super) fn between_extraction_and_record(folder: &Path) {
        let hooks = HOOKS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (registered, action) in hooks.iter() {
            if registered == folder {
                action();
            }
        }
    }

    fn context<'a>(dir: &std::path::Path, stdin: &'a mut &'static [u8]) -> Context<'a> {
        Context {
            cwd: dir.to_path_buf(),
            stdin,
            today: chrono::NaiveDate::default(),
            timestamp: String::new(),
            color_terminal: false,
            warm: None,
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
        assert_eq!(Writing::default().wait(), Ok(None));
        assert_eq!(
            Writing::written("sha256:x".into()).wait(),
            Ok(Some("sha256:x".into()))
        );
        let failed = Writing::spawn(|| Err("disk full".to_owned()));
        assert_eq!(failed.wait(), Err("disk full".to_owned()));
        drop(Writing::spawn(|| Ok(String::new())));
        let nothing = Writing::default().then_remember(
            std::env::temp_dir().join("rb-never-written"),
            "p".into(),
            evaluated::Verdict {
                document: GraphDocument::default(),
                expired: Vec::new(),
                vacuous: Vec::new(),
                ratchets: crate::ratchets::Ratchets::default(),
                warnings: Vec::new(),
            },
            false,
        );
        assert_eq!(nothing.wait(), Err("no extraction was written".to_owned()));
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

    /// Review item 3: an input edited between the extraction and the recording of its digest is
    /// not trusted by the next run, and one deleted then forces a full run.
    #[test]
    fn an_edit_during_the_run_is_read_again_on_the_next() -> Result<(), Box<dyn std::error::Error>>
    {
        for (strategy, deleted) in [
            (CacheStrategy::Metadata, false),
            (CacheStrategy::Content, false),
            (CacheStrategy::Metadata, true),
        ] {
            let dir = std::env::temp_dir().join(format!(
                "rb-cache-midrun-{}-{deleted}-{}",
                strategy.as_str(),
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("src"))?;
            let dir = rb_model::without_verbatim(&dir.canonicalize()?);
            std::fs::write(
                dir.join("src/a.ts"),
                "import { b } from \"./b\";\nexport const a = b;\n",
            )?;
            std::fs::write(dir.join("src/b.ts"), "export const b = 1;\n")?;
            std::fs::write(dir.join("src/c.ts"), "export const c = 1;\n")?;
            let options = CacheOptions {
                strategy,
                ..CacheOptions::in_folder(".rbc")
            };
            let config = Config::default();
            let paths = ["src".to_owned()];
            let mut empty: &'static [u8] = &[];
            let ctx = context(&dir, &mut empty);
            // While the writer records the inputs, `b.ts` gains an import (or goes).
            let folder = ctx.resolve(".rbc");
            let target = dir.join("src/b.ts");
            HOOKS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((
                    folder.clone(),
                    Box::new(move || {
                        let _ = if deleted {
                            std::fs::remove_file(&target)
                        } else {
                            std::fs::write(
                                &target,
                                "import { c } from \"./c\";\nexport const b = c;\n",
                            )
                        };
                    }),
                ));
            let first = extract_cached(&ctx, &config, &paths, &options, None)?;
            assert!(matches!(first.served, Served::Full(_)));
            assert!(first.writing.wait()?.is_some(), "the entry is written");
            HOOKS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .retain(|(registered, _)| *registered != folder);
            let second = extract_cached(&ctx, &config, &paths, &options, None)?;
            match &second.served {
                Served::Full(reason) if deleted => {
                    assert_eq!(reason, "src/b.ts was deleted");
                }
                Served::Incremental { typescript: 1, .. } if !deleted => {}
                other => unreachable!("{strategy:?} deleted={deleted}: {other:?}"),
            }
            let cold = pipeline::extract(&ctx, &config, &paths)?;
            let Content::Extracted(document, _) = second.content else {
                unreachable!("the second run extracts");
            };
            assert_eq!(
                serde_json::to_string(&*document)?,
                serde_json::to_string(&cold)?,
                "{strategy:?} deleted={deleted}"
            );
            let _ = second.writing.wait();
            let _ = std::fs::remove_dir_all(&dir);
        }
        Ok(())
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
        assert!(
            decide(&config, &scope(dir), parts.clone(), &changed(&[]))
                .extraction()
                .is_none()
        );
        for other in ["notes/x.json", ".babelrc", "configs/base.json", "src/"] {
            assert_eq!(
                decide(&config, &scope(dir), parts.clone(), &changed(&[other]))
                    .extraction()
                    .map(|(_, s)| s),
                Some(Served::Full(format!("{other} changed"))),
                "any input other than a source or an assembly is structural"
            );
        }
        let structural = changes::Changes {
            structural: Some("src/c.ts was added".into()),
            ..changed(&["src/a.ts"])
        };
        let Some((plans, served)) =
            decide(&config, &scope(dir), parts.clone(), &structural).extraction()
        else {
            unreachable!("a structural change extracts");
        };
        assert_eq!(served, Served::Full("src/c.ts was added".into()));
        assert_eq!(plans.typescript, Plan::Full);
        assert!(plans.keep_file_states);
        let manifest = decide(
            &config,
            &scope(dir),
            parts.clone(),
            &changed(&["web/package.json"]),
        )
        .extraction();
        assert_eq!(
            manifest.map(|(_, s)| s),
            Some(Served::Full("web/package.json changed".into()))
        );
        let Some((plans, served)) =
            decide(&config, &scope(dir), parts.clone(), &changed(&["src/b.ts"])).extraction()
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
            parts.clone(),
            &changed(&["app/m.py", "bin/A.dll"]),
        )
        .extraction() else {
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
