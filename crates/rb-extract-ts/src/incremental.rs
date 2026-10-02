//! An incremental extraction that keeps the earlier extraction's modules: the changed files are
//! read, the walk is replayed over the unchanged files' modules as they stand, and those modules,
//! their file states and, when no file's code layer changed, the linked code layer are moved into
//! the result instead of being rebuilt.
//!
//! - Plan: [Wave 3, Step 16](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)
//!   (`guard --watch` re-checks a saved file on the private-monorepo-sized tree),
//!   [Wave 3, Step 2](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)
//!   (incremental extraction equals full extraction)
//! - Requirements: [NFR-PERF-03](../../../docs/prd.md#nfr-perf-03) (a saved file re-checked in
//!   under 100 ms), [FR-CLI-05](../../../docs/prd.md#fr-cli-05)
//! - Decision: [ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) (the
//!   earlier extraction arrives as `rb_model` types and leaves as them)
//!
//! [`replay`] gives what [`crate::pipeline::extract_reusing_from`] followed by
//! [`crate::to_extraction`] give for the same request, byte for byte, under the same rule: every
//! unchanged file's earlier module is what reading it again would give. The steps are the same
//! ones in the same order: settle the extension list on the initial sources, walk depth first
//! (`crate::pipeline::depth_first`), put each file's unfollowed dependencies after it, compute
//! the statistics, link the code layer in module order. What differs is only where a reused
//! file's parts come from: its module (dependencies, statistics, language) and its file state are
//! the earlier ones, moved; the document's dependency is never turned back into the pipeline's
//! and out again. The code layer is relinked from every file's state unless the files are the
//! same, in the same order, and each file read again has the code layer it had: then linking
//! would give the earlier layer, which is moved instead.
//!
//! It applies when [`applies`] says so: without the sidecar (whose answers arrive in a loop of
//! their own), and when [`crate::reuse_refused`] has no reason (`exclude.dynamic`, `maxDepth`).

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

use rayon::prelude::*;
use rb_model::{ExtractError, Extraction, FileState, Module, Receipt};

use crate::codelayer::{self, FileCode};
use crate::pipeline::{self, Extracted, PipelineError, Settings};
use crate::resolve::ResolveConfig;

/// Whether [`replay`] answers for a run with `settings`.
pub fn applies(settings: &Settings) -> bool {
    settings.sidecar.is_none() && crate::reuse_refused(settings).is_none()
}

/// A file read again: its dependencies and code layer.
struct Read {
    dependencies: Vec<Extracted>,
    code: Option<FileCode>,
}

/// Where a visited file's result comes from.
enum Visited {
    /// The earlier extraction's module at this index.
    Previous(usize),
    /// Read now.
    Read(Box<Read>),
}

/// The resolutions an earlier module's walk followed, in order, as [`pipeline::follows`] gives
/// them for the dependencies it was written from.
fn followed(module: &Module) -> Vec<Cow<'_, str>> {
    module
        .dependencies
        .iter()
        .filter(|d| d.followable && !d.matches_do_not_follow.unwrap_or(false))
        .map(|d| Cow::Borrowed(d.resolved.as_str()))
        .collect()
}

/// What the module standing for a dependency is written from, in either form.
struct Unfollowed<'d> {
    resolved: &'d str,
    /// `followable`, `coreModule`, `couldNotResolve`, `matchesDoNotFollow`.
    flags: (bool, bool, bool, bool),
    dependency_types: &'d [rb_model::DependencyType],
}

impl<'d> Unfollowed<'d> {
    fn of_extracted(d: &'d Extracted) -> Self {
        Self {
            resolved: &d.resolved,
            flags: (
                d.followable,
                d.core_module,
                d.could_not_resolve,
                d.matches_do_not_follow,
            ),
            dependency_types: &d.dependency_types,
        }
    }

    fn of_dependency(d: &'d rb_model::Dependency) -> Self {
        Self {
            resolved: &d.resolved,
            flags: (
                d.followable,
                d.core_module,
                d.could_not_resolve,
                d.matches_do_not_follow.unwrap_or(false),
            ),
            dependency_types: &d.dependency_types,
        }
    }
}

/// The modules a file's unfollowed dependencies stand for, as [`crate::to_extraction`] writes
/// them after the file: each one no module before it (in `sources`) already is.
fn unfollowed_modules<'d>(
    dependencies: impl Iterator<Item = Unfollowed<'d>>,
    sources: &HashSet<String>,
) -> Vec<Module> {
    dependencies
        .filter(|d| !d.flags.0 && !sources.contains(d.resolved))
        .map(|d| {
            let (followable, core_module, could_not_resolve, matches_do_not_follow) = d.flags;
            let mut node = Module::new(d.resolved.to_owned());
            node.followable = Some(followable);
            node.core_module = Some(core_module);
            node.could_not_resolve = Some(could_not_resolve);
            node.matches_do_not_follow = Some(matches_do_not_follow);
            node.dependency_types = Some(d.dependency_types.to_vec());
            node
        })
        .collect()
}

/// A file's code layer as its earlier state kept it; `Err` when the state cannot be read back.
fn kept_code(state: Option<&FileState>) -> Result<Option<FileCode>, ()> {
    match state.and_then(|s| s.code.as_ref()) {
        Some(value) => serde::Deserialize::deserialize(value)
            .map(Some)
            .map_err(|_: serde_json::Error| ()),
        None => Ok(None),
    }
}

/// The earlier walk's files, sorted out.
struct Earlier<'e> {
    /// Every file, in the earlier walk's order.
    files: Vec<&'e str>,
    /// The files that stand for themselves, with their module's index.
    reused: HashMap<&'e str, usize>,
    /// The files read again.
    reread: Vec<&'e str>,
}

/// Sorts the earlier modules' files: reused when `reusable` accepts the module and, with the code
/// layer on, its state is kept; read again otherwise, as the pipeline's reuse decides.
fn sort_out<'e>(
    modules: &'e [Module],
    states: &BTreeMap<String, FileState>,
    settings: &Settings,
    reusable: impl Fn(&Module) -> bool,
) -> Earlier<'e> {
    let mut earlier = Earlier {
        files: Vec::new(),
        // Asked only, never iterated.
        reused: HashMap::new(),
        reread: Vec::new(),
    };
    for (at, module) in modules.iter().enumerate() {
        if module.language.is_none() {
            continue;
        }
        earlier.files.push(&module.source);
        let with_state = !settings.code_layer || states.contains_key(&module.source);
        if with_state && reusable(module) {
            earlier.reused.insert(&module.source, at);
        } else {
            earlier.reread.push(&module.source);
        }
    }
    earlier
}

/// The walk from `initial` over the earlier modules and the files read again, in visiting order.
fn walk(
    initial: &[String],
    settings: &Settings,
    config: &ResolveConfig,
    modules: &[Module],
    earlier: &Earlier<'_>,
) -> Result<Vec<(String, Visited)>, PipelineError> {
    let extract = |file: &&str| {
        (
            (*file).to_owned(),
            pipeline::extract_file(file, settings, config),
        )
    };
    // As in the full walk, a failure counts only if the walk reaches the file. One file (a save)
    // is read on this thread: handing it to the pool would only wait for a thread.
    let mut read: BTreeMap<String, pipeline::FileResult> = if earlier.reread.len() > 1 {
        earlier.reread.par_iter().map(extract).collect()
    } else {
        earlier.reread.iter().map(extract).collect()
    };
    let visit = |(dependencies, code): (Vec<Extracted>, Option<FileCode>)| {
        (
            pipeline::follows(&dependencies),
            Visited::Read(Box::new(Read { dependencies, code })),
        )
    };
    pipeline::depth_first(initial, |file, _| {
        if let Some(result) = read.remove(file) {
            return result.map(visit);
        }
        if let Some(&at) = earlier.reused.get(file)
            && let Some(module) = modules.get(at)
        {
            return Ok((followed(module), Visited::Previous(at)));
        }
        // A file the earlier walk did not reach.
        pipeline::extract_file(file, settings, config).map(visit)
    })
}

/// The file states of the result: the earlier ones, those of the files no longer reached left
/// out, as the full run writes them (no warnings, and no code layer when it is off); the files
/// read again are restated as they are assembled ([`restate`]).
fn file_states(
    settings: &Settings,
    mut states: BTreeMap<String, FileState>,
    visited: &[(String, Visited)],
    same_files: bool,
) -> BTreeMap<String, FileState> {
    if !settings.keep_file_states {
        return BTreeMap::new();
    }
    if !same_files {
        let reached: HashSet<&str> = visited.iter().map(|(s, _)| s.as_str()).collect();
        states.retain(|source, _| reached.contains(source.as_str()));
    }
    for state in states.values_mut() {
        state.warnings.clear();
        if !settings.code_layer {
            state.code = None;
        }
    }
    if !settings.code_layer {
        for (source, visit) in visited {
            if matches!(visit, Visited::Previous(_)) {
                states.entry(source.clone()).or_default();
            }
        }
    }
    states
}

/// The state of a file read again, as its full read gives it, or none when its code layer cannot
/// be written, as the full run's states leave it out.
fn restate(
    settings: &Settings,
    states: &mut BTreeMap<String, FileState>,
    source: &str,
    code: Option<&FileCode>,
) {
    if !settings.keep_file_states {
        return;
    }
    match code.map(serde_json::to_value).transpose() {
        Ok(value) => {
            states.insert(
                source.to_owned(),
                FileState {
                    code: value,
                    warnings: Vec::new(),
                },
            );
        }
        Err(_) => {
            states.remove(source);
        }
    }
}

/// What [`assemble`] puts together.
struct Assembled {
    modules: Vec<Module>,
    /// The files' code layers in module order, when the layer is linked again.
    codes: Vec<FileCode>,
    files: u64,
}

/// The modules in visiting order, each file's unfollowed dependencies after it, with each reused
/// file's earlier module moved in; and, when `relink`, every file's code layer.
fn assemble(
    (settings, config): (&Settings, &ResolveConfig),
    visited: Vec<(String, Visited)>,
    earlier: Vec<Module>,
    (kept, statistics): (Vec<KeptCode>, Statistics),
    states: &mut BTreeMap<String, FileState>,
    relink: bool,
) -> Result<Assembled, ExtractError> {
    let mut slots: Vec<Option<Module>> = earlier.into_iter().map(Some).collect();
    let mut out = Assembled {
        modules: Vec::with_capacity(slots.len()),
        codes: Vec::new(),
        files: 0,
    };
    // Asked only, never iterated.
    let mut sources: HashSet<String> = HashSet::new();
    for (((source, visit), kept), statistics) in visited.into_iter().zip(kept).zip(statistics) {
        let experimental_stats = statistics.transpose()?;
        let (node, code, unfollowed) = match visit {
            Visited::Previous(at) => {
                let Some(mut node) = slots.get_mut(at).and_then(Option::take) else {
                    continue;
                };
                node.experimental_stats = experimental_stats;
                let code = match kept {
                    Some(Ok(code)) => code,
                    // A state that does not read back: the file's own code layer, read again.
                    Some(Err(())) => {
                        let code = pipeline::extract_file(&source, settings, config)?.1;
                        restate(settings, states, &source, code.as_ref());
                        code
                    }
                    None => None,
                };
                let unfollowed = unfollowed_modules(
                    node.dependencies.iter().map(Unfollowed::of_dependency),
                    &sources,
                );
                (node, code, unfollowed)
            }
            Visited::Read(read) => {
                let Read { dependencies, code } = *read;
                restate(settings, states, &source, code.as_ref());
                let by_sidecar = crate::needs_sidecar(&source);
                let mut node = Module::new(source.clone());
                node.dependencies = dependencies
                    .iter()
                    .map(|d| crate::to_dependency(d, by_sidecar))
                    .collect();
                node.experimental_stats = experimental_stats;
                node.language = Some(codelayer::language_of(&source));
                let unfollowed =
                    unfollowed_modules(dependencies.iter().map(Unfollowed::of_extracted), &sources);
                (node, code, unfollowed)
            }
        };
        out.files += 1;
        if relink && let Some(code) = code {
            out.codes.push(code);
        }
        // Upstream compares with the modules before this one only, so one module's duplicate
        // unfollowed dependencies each become a module.
        sources.insert(source);
        sources.extend(unfollowed.iter().map(|m| m.source.clone()));
        out.modules.push(node);
        out.modules.extend(unfollowed);
    }
    Ok(out)
}

/// A reused file's code layer read back from its state ([`kept_code`]), when the layer is linked
/// again.
type KeptCode = Option<Result<Option<FileCode>, ()>>;

/// Each visited file's `experimentalStats`, when they are asked for.
type Statistics = Vec<Option<Result<rb_model::ExperimentalStats, PipelineError>>>;

/// The statistics of each visited file: the earlier module's, else computed, as the pipeline's
/// reuse takes them; none when they are not asked for.
fn statistics(
    settings: &Settings,
    visited: &[(String, Visited)],
    earlier: &[Module],
) -> Statistics {
    if !settings.experimental_stats {
        return visited.iter().map(|_| None).collect();
    }
    visited
        .par_iter()
        .map(|(source, visit)| {
            let kept = match visit {
                Visited::Previous(at) => earlier.get(*at).and_then(|m| m.experimental_stats),
                Visited::Read(_) => None,
            };
            Some(kept.map_or_else(|| pipeline::stats(source, settings), Ok))
        })
        .collect()
}

/// The extraction for `initial` (the walk's initial sources) with each file of `previous` that
/// `reusable` accepts taken from there, and every other file read (see the module doc).
///
/// # Errors
/// As [`crate::extract_with`]: the first file the walk reaches that cannot be read, or no module.
pub fn replay(
    initial: &[String],
    settings: &Settings,
    config: &ResolveConfig,
    reusable: impl Fn(&Module) -> bool,
    mut previous: Extraction,
) -> Result<Extraction, ExtractError> {
    pipeline::settle_followable(initial, settings, config);
    let modules = std::mem::take(&mut previous.modules);
    let states = std::mem::take(&mut previous.files);
    let earlier = sort_out(&modules, &states, settings, reusable);
    let visited = walk(initial, settings, config, &modules, &earlier)?;
    let same_files = visited.len() == earlier.files.len()
        && visited
            .iter()
            .zip(&earlier.files)
            .all(|((source, _), earlier_source)| source == earlier_source);
    // The earlier layer stands when linking would give it again: the same files in the same
    // order, and every file read again with the code layer its state kept.
    let same_layer = settings.code_layer
        && previous.code.is_some()
        && same_files
        && visited.iter().all(|(source, visit)| match visit {
            Visited::Previous(_) => true,
            Visited::Read(read) => {
                let kept = states.get(source);
                kept.is_some() && matches!(kept_code(kept), Ok(ref code) if *code == read.code)
            }
        });
    // Each reused file's code layer, read back from its state only when the layer is relinked.
    let relink = settings.code_layer && !same_layer;
    let kept: Vec<KeptCode> = visited
        .par_iter()
        .map(|(source, visit)| match visit {
            Visited::Previous(_) if relink => Some(kept_code(states.get(source))),
            Visited::Previous(_) | Visited::Read(_) => None,
        })
        .collect();
    let statistics = statistics(settings, &visited, &modules);
    drop(earlier);
    let mut states = file_states(settings, states, &visited, same_files);
    let assembled = assemble(
        (settings, config),
        visited,
        modules,
        (kept, statistics),
        &mut states,
        relink,
    )?;
    if assembled.files == 0 {
        return Err(ExtractError::NoModulesFound);
    }
    let code = if same_layer {
        previous.code.take()
    } else {
        // The earlier layer is replaced; the pool frees it while the caller goes on.
        if let Some(earlier) = previous.code.take() {
            rayon::spawn(move || drop(earlier));
        }
        settings
            .code_layer
            .then(|| codelayer::link(assembled.codes))
    };
    let count = assembled.modules.len() as u64;
    let sidecar = crate::sidecar::receipt(&assembled.modules, None);
    Ok(Extraction {
        modules: assembled.modules,
        code,
        inspected: Receipt::counts(assembled.files, 0, count),
        warnings: Vec::new(),
        files: states,
        sidecar,
        walk: None,
    })
}
