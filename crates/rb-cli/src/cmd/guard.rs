//! `guard`: the Stop hook's answer, computed ahead of the hook, and with `--watch` kept current
//! as files are saved, so the hook itself only reads a file.
//!
//! - Plan: [Wave 3, Step 16](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof),
//!   [§ 1.4](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#the-inner-loop-on-a-large-net-solution),
//!   the decision rule in [§ 1.6](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#16-decisions-applied-and-decisions-this-wave-must-make)
//!   ("The `guard --watch` findings file and its freshness")
//! - Source: [design § The agentic engineering hat](../../../../docs/artifacts/design.md#the-agentic-engineering-hat-turn-two)
//!   ("a daemon that re-checks a saved file within 100 ms and writes findings to a file the Stop
//!   hook reads")
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md) (a thin loop over
//!   the CLI's own commands), [ADR-0011](../../../../docs/adr/0011-read-dotnet-assemblies-not-source.md)
//! - Requirement: [FR-CLI-05](../../../../docs/prd.md#fr-cli-05), [NFR-PERF-03](../../../../docs/prd.md#nfr-perf-03)
//!
//! `guard` extracts once, keeping each file's state, evaluates as `cruise --from-hook` does and
//! writes [`FINDINGS`]: the hook's answer, when it was written (`writtenAt`, milliseconds since the
//! epoch), the hash of the configuration files (`configHash`, as the cache computes it), and a
//! key of the command line it answers for. `cruise --from-hook` serves that answer instead of
//! cruising when the file is younger than [`FRESH_MS`], names the same configuration hash and
//! key, and was written by the same build ([`served`]).
//!
//! With `--watch` it then polls: every source file an extractor read, the files whose change
//! moves resolution (the configuration, tsconfig and package manifests, solution and project
//! files, the built assemblies in compiled mode), the folders holding the sources, and every
//! folder the TypeScript walk listed to find its initial files. A saved source is extracted again
//! alone, through the extractor's incremental entry, and the answer rewritten; a changed
//! structural file, a source added to or removed from a folder, or a file the walk gathers or a
//! subfolder added to or removed from a folder it listed, reads everything again. So while
//! nothing reads everything again, the walk's folders are unchanged and the incremental run starts
//! from the earlier walk's initial files instead of listing every folder again
//! ([`rb_model::ExtractRequest::walk_unchanged`]). An incremental run that fails has taken the
//! earlier extraction with it, so the next change reads everything again. While nothing changes
//! the file is rewritten every [`HEARTBEAT_MS`], so its age says the daemon is alive and has seen every change up to then. .NET is read in source mode
//! unless `--mode compiled` is given, since a daemon that waited for a build would not be one.
//! It polls rather than subscribing to file-system events: `notify` is CC0-1.0, outside the
//! licence allow-list, and the plan names polling as the fallback (§ 1.8). It writes nothing
//! outside `.graph/guard/`, logs to stderr only, removes the findings file when it stops, and
//! stops when standard input closes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Args;
use rb_config::Config;
use rb_model::{ExtractRequest, Extraction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cache::{changes, key};
use crate::cli::{ColorChoice, ConfigArgs, CruiseArgs, Liveness, ModeArg, SidecarArg};
use crate::cmd::cruise::{self, Given};
use crate::context::Context;
use crate::exit::{ExitCodeMode, RunExit};
use crate::pipeline::{self, Parts, Plan, Plans};
use crate::{Outcome, configure};

/// The findings file, relative to the working directory.
pub const FINDINGS: &str = ".graph/guard/findings.json";

/// The graph `guard --watch` rewrites when a save changed it: the merged document it answers
/// for, unevaluated, which a server's warm graph takes when it is newer than the saved one
/// ([`crate::serve::graph`]).
pub const GUARD_GRAPH: &str = ".graph/guard/graph.json";
/// The file a hook writes to ask a running guard to confirm it has seen every change until then.
pub const REQUEST: &str = ".graph/guard/request";

/// How long a hook waits for the guard's confirmation before it cruises itself.
pub const WAIT_MS: u64 = 5_000;

/// How old the findings may be for the hook to serve them.
pub const FRESH_MS: u64 = 5_000;

/// How often an unchanged answer is rewritten, so its age stays under [`FRESH_MS`].
pub const HEARTBEAT_MS: u64 = 1_000;

/// `rulebearing guard`.
#[derive(Debug, Clone, Default, Args)]
pub struct GuardArgs {
    /// Files, directories and globs to cruise, as `cruise` takes them
    #[arg(value_name = "FILES-OR-DIRECTORIES")]
    pub paths: Vec<String>,
    /// Configuration
    #[command(flatten)]
    pub config: ConfigArgs,
    /// Keep the findings current: check the watched files every --interval milliseconds, check
    /// again what was saved, and stop when standard input closes
    #[arg(long)]
    pub watch: bool,
    /// With --watch: milliseconds to wait between two checks of the watched files
    #[arg(long, value_name = "MS", default_value_t = 25, requires = "watch")]
    pub interval: u64,
    /// How .NET is read: source (the default here, no build needed) or compiled
    #[arg(long, value_enum, value_name = "MODE")]
    pub mode: Option<ModeArg>,
    /// As `cruise --sidecar`
    #[arg(long, value_enum, value_name = "RUNTIME")]
    pub sidecar: Option<SidecarArg>,
    /// As `cruise --affected`
    #[arg(short = 'A', long, value_name = "REVISION", num_args = 0..=1,
          default_missing_value = rb_config::model::DEFAULT_AFFECTED_REVISION)]
    pub affected: Option<String>,
    /// As `cruise --affected-depth`
    #[arg(long, value_name = "NUMBER")]
    pub affected_depth: Option<u32>,
    /// As `cruise --liveness`
    #[arg(long, value_enum, value_name = "MODE", conflicts_with = "no_liveness")]
    pub liveness: Option<Liveness>,
    /// As `cruise --no-liveness`
    #[arg(long)]
    pub no_liveness: bool,
    /// As `cruise --max-findings`
    #[arg(long, value_name = "N")]
    pub max_findings: Option<usize>,
}

impl GuardArgs {
    /// The `cruise --from-hook` whose answer the guard keeps: these flags, with .NET in source
    /// mode unless `--mode compiled`.
    pub fn hook(&self) -> CruiseArgs {
        CruiseArgs {
            paths: self.paths.clone(),
            config: self.config.clone(),
            mode: Some(self.mode.unwrap_or(ModeArg::Source)),
            sidecar: self.sidecar,
            affected: self.affected.clone(),
            affected_depth: self.affected_depth,
            liveness: self.liveness,
            no_liveness: self.no_liveness,
            max_findings: self.max_findings,
            from_hook: true,
            ..CruiseArgs::default()
        }
    }
}

/// The findings file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Findings {
    /// `rulebearing <version>`: another build's answer is never served.
    pub tool: String,
    /// When the answer was last confirmed current, milliseconds since the epoch.
    pub written_at: u64,
    /// The start of the last scan the answer reflects, milliseconds since the epoch: every change
    /// made before it is in the answer. A hook serves the answer only once this passes the moment
    /// it asked ([`REQUEST`]).
    pub seen_up_to: u64,
    /// The configuration files' hash ([`key::config_hash`]).
    pub config_hash: String,
    /// The command line answered for ([`answer_key`]).
    pub key: String,
    /// How the guard reads .NET, `source` or `compiled` (its `--mode`), whether or not the tree
    /// holds any .NET.
    pub mode: String,
    /// What `cruise --from-hook` prints: the block decision, or nothing.
    pub answer: String,
    /// The files checked again for this answer, relative to the working directory; empty for
    /// a full read.
    pub rechecked: Vec<String>,
    /// From the newest of those files' modification times to the answer written, milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    /// Why the answer could not be computed, when it could not; the answer is then empty, as the
    /// hook's is for a run it cannot trust.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// How long the check took, by stage, milliseconds: extracting the changed files and
    /// evaluating and reporting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timings: Option<Timings>,
}

/// A check's stages, milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Timings {
    /// Reading the changed files again (or everything).
    pub extract: u64,
    /// Evaluating the rules and rendering the hook's answer.
    pub answer: u64,
}

fn millis(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

fn tool() -> String {
    format!("rulebearing {}", env!("CARGO_PKG_VERSION"))
}

/// Nanoseconds since the epoch, as [`stamp`] records modification times.
fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The command line a hook answer depends on: every flag of `cruise` except those that only
/// shape how it is printed or reached (the reporter and destination, progress, colour, the
/// cache, the mode, the exit code's form), hashed.
pub fn answer_key(args: &CruiseArgs) -> String {
    let normalised = CruiseArgs {
        from_hook: true,
        output_type: None,
        output_to: None,
        progress: None,
        no_progress: false,
        color: ColorChoice::default(),
        cache: None,
        cache_strategy: None,
        no_cache: false,
        mode: None,
        exit_code_mode: ExitCodeMode::Default,
        allow_approximate_gate: false,
        ..args.clone()
    };
    let digest = Sha256::digest(format!("{normalised:?}").as_bytes());
    digest
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// The configuration hash the findings are keyed on, from the files the load read.
fn config_hash(ctx: &Context<'_>, loaded: Option<&Config>) -> String {
    let root = key::worktree_root(&ctx.cwd);
    match loaded {
        Some(config) => key::config_hash(config, &root),
        None => key::config_hash(&Config::default(), &root),
    }
}

/// Whether `findings` answer for `args`: the same build, the same command line and the current
/// configuration.
fn answers_for(ctx: &mut Context<'_>, args: &CruiseArgs, findings: &Findings) -> bool {
    if findings.tool != tool() || findings.key != answer_key(args) {
        return false;
    }
    configure::load(ctx, &args.config)
        .is_ok_and(|loaded| findings.config_hash == config_hash(ctx, loaded.as_ref()))
}

fn read_findings(ctx: &Context<'_>) -> Option<Findings> {
    serde_json::from_str(&std::fs::read_to_string(ctx.resolve(FINDINGS)).ok()?).ok()
}

/// The answer a running `guard --watch` has for `args`, once it confirms it has seen every
/// change until now: the findings must be younger than [`FRESH_MS`] and answer for this command
/// line, configuration and build; the hook then writes [`REQUEST`] and waits, up to [`WAIT_MS`],
/// for findings whose `seenUpTo` passes it. A file saved a moment before the hook is therefore in
/// the answer, never missed. `None` when no guard confirms in time: the hook cruises itself.
pub fn served(ctx: &mut Context<'_>, args: &CruiseArgs) -> Option<String> {
    if args.config.config.as_deref() == Some("-") {
        return None;
    }
    let findings = read_findings(ctx)?;
    let now = now_ms();
    let fresh = findings.written_at <= now.saturating_add(HEARTBEAT_MS)
        && now.saturating_sub(findings.written_at) < FRESH_MS;
    if !fresh || !answers_for(ctx, args, &findings) {
        return None;
    }
    if findings.seen_up_to >= now {
        return Some(findings.answer);
    }
    let request = ctx.resolve(REQUEST);
    std::fs::write(&request, now.to_string()).ok()?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(WAIT_MS) {
        std::thread::sleep(Duration::from_millis(2));
        match read_findings(ctx) {
            Some(confirmed) if confirmed.seen_up_to >= now => {
                return answers_for(ctx, args, &confirmed).then_some(confirmed.answer);
            }
            Some(_) => {}
            // The guard stopped and took its findings with it.
            None => return None,
        }
    }
    None
}

/// The moment a hook asked, when it asked after `since`.
fn requested_after(ctx: &Context<'_>, since: u64) -> bool {
    std::fs::read_to_string(ctx.resolve(REQUEST))
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .is_some_and(|asked| asked > since)
}

/// A file's size and modification time in nanoseconds; `None` while it does not exist.
type Stamp = Option<(u64, u64)>;

/// A file's size and modification time in nanoseconds, a folder's included.
fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
    Some((meta.len(), modified))
}

/// Which part a source belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    TypeScript,
    Dotnet,
    Python,
}

/// What the guard watches.
#[derive(Debug, Default)]
struct Watched {
    /// Each source: its part, its name in the part's file states, its stamp.
    sources: BTreeMap<PathBuf, (Part, String, Stamp)>,
    /// The files whose change reads everything again, with their stamps (none while absent).
    structural: BTreeMap<PathBuf, Stamp>,
    /// Each folder holding a source: its stamp and the relevant names it held.
    folders: BTreeMap<PathBuf, (Stamp, BTreeSet<String>)>,
    /// Each folder the TypeScript walk listed: its stamp and the entries that can change what
    /// the walk gathers ([`walk_listing`]).
    walked: BTreeMap<PathBuf, (Stamp, BTreeSet<String>)>,
    /// The TypeScript options the walk ran with.
    typescript: rb_model::TypeScriptOptions,
    /// `.cs` files are sources.
    cs: bool,
    /// Why the next check reads everything: a structural file or a folder changed while the
    /// extraction these stamps follow was running, so the extraction may not have seen it.
    unsettled: Option<String>,
}

/// What a check of the watched files found.
#[derive(Debug, PartialEq, Eq)]
enum Change {
    None,
    /// Sources whose content changed, with the newest modification time among them (ns).
    Sources(Vec<(Part, String, PathBuf)>, u64),
    /// Something reuse cannot follow.
    Structural(String),
}

/// Whether a file's appearing or disappearing in a watched folder can change the graph.
fn relevant(name: &str, cs: bool) -> bool {
    if changes::is_manifest(name) {
        return true;
    }
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        (changes::RELEVANT_EXTENSIONS.contains(&extension) && !matches!(extension, "dll" | "pdb"))
            || (cs && extension.eq_ignore_ascii_case("cs"))
    })
}

fn listing(folder: &Path, cs: bool) -> BTreeSet<String> {
    std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| relevant(name, cs))
                .collect()
        })
        .unwrap_or_default()
}

/// The entries of a folder the TypeScript walk listed that can change what it gathers: each
/// subfolder, marked with a trailing `/`, and each file the walk gathers by its name
/// ([`rb_extract_ts::gathers`]). Any other entry (an editor's swap file, say) changes nothing.
fn walk_listing(folder: &Path, options: &rb_model::TypeScriptOptions) -> BTreeSet<String> {
    let gathers = |name: &str| {
        #[cfg(feature = "extract-ts")]
        {
            rb_extract_ts::gathers(options, name)
        }
        #[cfg(not(feature = "extract-ts"))]
        {
            let _ = (options, name);
            false
        }
    };
    std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    // As the walk tells them apart: through a symbolic link.
                    if std::fs::metadata(e.path()).is_ok_and(|m| m.is_dir()) {
                        Some(format!("{name}/"))
                    } else {
                        gathers(&name).then_some(name)
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// How far behind the clock a modification time may be stamped: a kernel stamps with a coarse
/// clock a few milliseconds behind.
const STAMP_SLACK_NS: u64 = 10_000_000;

/// The slack for a time in whole seconds: a file system that keeps one or two second times
/// (FAT, some network mounts) rounds a save down by up to that much.
const COARSE_SLACK_NS: u64 = 2_000_000_000;

/// Whether `stamped` may have been modified at or after `since` (nanoseconds since the epoch),
/// allowing for how coarsely the time was recorded. A file wrongly taken as modified is only
/// checked once more.
fn after(stamped: Stamp, since: u64) -> bool {
    stamped.is_some_and(|(_, modified)| {
        let slack = if modified % 1_000_000_000 == 0 {
            COARSE_SLACK_NS
        } else {
            STAMP_SLACK_NS
        };
        modified.saturating_add(slack) >= since
    })
}

impl Watched {
    /// What to watch after an extraction that started at `since` (nanoseconds since the
    /// epoch). A file or folder modified since may have changed under the extraction, so it is
    /// not taken as seen: a source is checked again on the first scan, and a structural file or
    /// a folder reads everything again.
    fn of(ctx: &Context<'_>, config: &Config, parts: &Parts, since: u64) -> Self {
        let mut watched = Self {
            cs: pipeline::dotnet_source_mode(config),
            typescript: config.languages.typescript.clone(),
            ..Self::default()
        };
        watched.add_sources(ctx, config, parts, since);
        let root = key::worktree_root(&ctx.cwd);
        let mut structural: Vec<PathBuf> = config.files.clone();
        structural.extend(key::manifest_paths(&root, &ctx.cwd, config));
        #[cfg(feature = "extract-ts")]
        if parts.typescript.is_some() {
            structural.extend(rb_extract_ts::configuration_files(
                &config.languages.typescript,
                &ctx.cwd,
            ));
        }
        #[cfg(feature = "extract-dotnet")]
        if parts.dotnet.is_some() {
            let options = config.languages.dotnet.clone().unwrap_or_default();
            structural.extend(pipeline::dotnet_project_files(
                &ctx.cwd,
                &options,
                parts.dotnet.as_ref(),
            ));
            if !watched.cs {
                structural.extend(
                    rb_extract_dotnet::assembly_inputs(&ctx.cwd, &options).unwrap_or_default(),
                );
            }
        }
        for path in structural {
            let path = ctx.resolve(path);
            let stamped = stamp(&path);
            watched.unsettle(&path, stamped, since, ctx);
            watched.structural.insert(path, stamped);
        }
        // The guard writes its findings under `.graph/` a few milliseconds before each scan; a
        // folder of its own is no input, and watching it would read everything again each time.
        let own = Path::new(FINDINGS)
            .components()
            .next()
            .map(|first| ctx.resolve(first.as_os_str()));
        let walked = parts.typescript.iter().filter_map(|t| t.walk.as_ref());
        for folder in walked.flat_map(|w| &w.folders) {
            let folder = ctx.resolve(folder);
            if own.as_ref().is_some_and(|own| folder.starts_with(own)) {
                continue;
            }
            let names = walk_listing(&folder, &watched.typescript);
            let stamped = stamp(&folder);
            watched.unsettle(&folder, stamped, since, ctx);
            watched.walked.insert(folder.clone(), (stamped, names));
        }
        watched
    }

    /// Records that `path` changed during the extraction, when it did.
    fn unsettle(&mut self, path: &Path, stamped: Stamp, since: u64, ctx: &Context<'_>) {
        if self.unsettled.is_none() && after(stamped, since) {
            let shown = key::slashed(path.strip_prefix(&ctx.cwd).unwrap_or(path));
            self.unsettled = Some(format!("{shown} changed while it was being read"));
        }
    }

    /// Watches every source of `parts` not watched yet, and the folders holding them: all of
    /// them after a full read, and after a check the files it reached for the first time. A
    /// source modified at or after `since` is checked again on the next scan.
    fn add_sources(&mut self, ctx: &Context<'_>, config: &Config, parts: &Parts, since: u64) {
        let base = ctx.cwd.join(
            config
                .languages
                .typescript
                .base_dir
                .as_deref()
                .unwrap_or(""),
        );
        let mut folders = BTreeSet::new();
        for (part, extraction, folder) in [
            (Part::TypeScript, &parts.typescript, &base),
            (Part::Dotnet, &parts.dotnet, &ctx.cwd),
            (Part::Python, &parts.python, &ctx.cwd),
        ] {
            for name in extraction.iter().flat_map(|e| e.files.keys()) {
                let path = folder.join(name);
                if self.sources.contains_key(&path) {
                    continue;
                }
                let stamped = stamp(&path);
                // Not seen as it is now: the first scan finds it different and checks it again.
                let recorded = if after(stamped, since) { None } else { stamped };
                if let Some(parent) = path.parent() {
                    folders.insert(parent.to_path_buf());
                }
                self.sources.insert(path, (part, name.clone(), recorded));
            }
        }
        for folder in folders {
            if self.folders.contains_key(&folder) {
                continue;
            }
            let names = listing(&folder, self.cs);
            let stamped = stamp(&folder);
            self.unsettle(&folder, stamped, since, ctx);
            self.folders.insert(folder, (stamped, names));
        }
    }

    /// Checks every watched file once, recording the new stamps of the sources it reports.
    fn check(&mut self, cwd: &Path) -> Change {
        if let Some(reason) = self.unsettled.take() {
            return Change::Structural(reason);
        }
        let shown = |path: &Path| key::slashed(path.strip_prefix(cwd).unwrap_or(path));
        for (path, recorded) in &self.structural {
            if stamp(path) != *recorded {
                return Change::Structural(format!("{} changed", shown(path)));
            }
        }
        for (folder, (recorded, names)) in &mut self.folders {
            let now = stamp(folder);
            if now != *recorded {
                if listing(folder, self.cs) != *names {
                    return Change::Structural(format!(
                        "a file was added to or removed from {}",
                        shown(folder)
                    ));
                }
                *recorded = now;
            }
        }
        for (folder, (recorded, names)) in &mut self.walked {
            let now = stamp(folder);
            if now != *recorded {
                if walk_listing(folder, &self.typescript) != *names {
                    return Change::Structural(format!(
                        "a file or folder was added to or removed from {}",
                        shown(folder)
                    ));
                }
                *recorded = now;
            }
        }
        let mut changed = Vec::new();
        let mut newest = 0;
        for (path, (part, name, recorded)) in &mut self.sources {
            let now = stamp(path);
            if now == *recorded {
                continue;
            }
            let Some((_, modified)) = now else {
                return Change::Structural(format!("{} was deleted", shown(path)));
            };
            *recorded = now;
            newest = newest.max(modified);
            changed.push((*part, name.clone(), path.clone()));
        }
        if changed.is_empty() {
            Change::None
        } else {
            Change::Sources(changed, newest)
        }
    }
}

/// The plans that read the changed sources again and reuse everything else, taking the parts
/// out of `parts` so they move into the next extraction rather than being copied. The TypeScript
/// request promises the walk unchanged: a change to any folder it listed is structural
/// ([`Watched::check`]), so no check that reaches here saw one.
fn plans(parts: &mut Parts, changed: &[(Part, String, PathBuf)]) -> Plans {
    let plan = |part: Part, extraction: &mut Option<Extraction>| {
        let names: Vec<PathBuf> = changed
            .iter()
            .filter(|(p, _, _)| *p == part)
            .map(|(_, name, _)| PathBuf::from(name))
            .collect();
        if names.is_empty() {
            return Plan::Reuse(extraction.take());
        }
        let previous = extraction.take().unwrap_or_default();
        let unchanged = previous
            .files
            .keys()
            .map(PathBuf::from)
            .filter(|n| !names.contains(n))
            .collect();
        Plan::Incremental(ExtractRequest {
            changed: names,
            unchanged,
            previous,
            walk_unchanged: part == Part::TypeScript,
        })
    };
    Plans {
        typescript: plan(Part::TypeScript, &mut parts.typescript),
        dotnet: plan(Part::Dotnet, &mut parts.dotnet),
        python: plan(Part::Python, &mut parts.python),
        keep_file_states: true,
        keep_walk: true,
    }
}

/// What the guard holds between checks.
struct State {
    effective: Config,
    parts: Parts,
    watched: Watched,
    /// The configuration files' hash ([`config_hash`]), as they were when this state was built.
    /// They are watched as structural files, so a change to one builds a new state.
    config_hash: String,
    /// An incremental extraction failed after taking the parts: the next change reads
    /// everything again.
    stale: bool,
    /// The last document answered for, with its answer ([`answer`]).
    answered: Option<Answered>,
}

/// A merged document, the extractors' warnings and the answer given for them.
struct Answered {
    document: Arc<rb_model::GraphDocument>,
    warnings: Vec<rb_model::Warning>,
    answer: Result<String, String>,
}

/// Loads the configuration and extracts in full.
fn build(ctx: &mut Context<'_>, hook: &CruiseArgs) -> Result<State, (RunExit, String)> {
    // The findings' folder exists before the walk, so its appearing in a folder the walk lists
    // is not taken for a change.
    if let Some(folder) = ctx.resolve(FINDINGS).parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let loaded =
        configure::load(ctx, &hook.config).map_err(|e| (RunExit::InvalidConfig, e.to_string()))?;
    let mut effective = loaded.clone().unwrap_or_default();
    configure::apply_flags(&mut effective, hook, ctx)
        .map_err(|e| (RunExit::InvalidConfig, e.to_string()))?;
    let plans = Plans {
        keep_file_states: true,
        keep_walk: true,
        ..Plans::default()
    };
    let since = now_ns();
    let parts = pipeline::extract_parts(ctx, &effective, &hook.paths, plans)
        .map_err(|e| (RunExit::Untrustworthy, e.to_string()))?;
    let watched = Watched::of(ctx, &effective, &parts, since);
    Ok(State {
        config_hash: config_hash(ctx, loaded.as_ref()),
        effective,
        parts,
        watched,
        stale: false,
        answered: None,
    })
}

/// The hook's answer for the state's extraction, or why there is none.
///
/// A save that leaves the graph as it was (an edit inside a function, a comment) merges into the
/// document already answered for, and the answer is a function of that document while the
/// configuration stands, so it is given again without evaluating. That holds only where nothing
/// else feeds the answer: not with `--affected`, whose closure follows what git calls changed,
/// and not with ratchets, whose budgets are files read on every run.
///
/// With `publish`, a document answered anew is also returned, for [`GUARD_GRAPH`]; one answered
/// as before is not, since the file already holds it.
fn answer(
    ctx: &mut Context<'_>,
    hook: &CruiseArgs,
    state: &mut State,
    publish: bool,
) -> (Result<String, String>, Option<Arc<rb_model::GraphDocument>>) {
    let (document, warnings) = match pipeline::merge(&state.effective, &state.parts) {
        Ok(merged) => merged,
        Err(e) => return (Err(e.to_string()), None),
    };
    let repeatable = hook.affected.is_none() && state.effective.rules.ratchets.is_empty();
    if repeatable
        && let Some(last) = &state.answered
        && last.warnings == warnings
        && *last.document == document
    {
        return (last.answer.clone(), None);
    }
    let copy = (repeatable || publish).then(|| Arc::new(crate::value::copy(&document)));
    let published = copy.clone().filter(|_| publish);
    let kept = copy
        .filter(|_| repeatable)
        .map(|document| (document, warnings.clone()));
    let outcome = cruise::hook_answer(ctx, hook, Given { document, warnings });
    let answer = match outcome
        .stderr
        .lines()
        .find(|l| l.starts_with("rulebearing cruise:"))
    {
        Some(line) if outcome.stdout.is_empty() => Err(line.to_owned()),
        _ => Ok(outcome.stdout),
    };
    state.answered = kept.map(|(document, warnings)| Answered {
        document,
        warnings,
        answer: answer.clone(),
    });
    (answer, published)
}

/// Writes each document it is sent to [`GUARD_GRAPH`] through a temporary file, on a thread of its
/// own so the check that produced it does not wait; when several are waiting, only the newest is
/// written. Dropping the writer stops the thread once it has written the last.
struct GraphWriter {
    send: std::sync::mpsc::Sender<Arc<rb_model::GraphDocument>>,
    thread: std::thread::JoinHandle<()>,
}

impl GraphWriter {
    fn start(path: PathBuf) -> Self {
        let (send, receive) = std::sync::mpsc::channel::<Arc<rb_model::GraphDocument>>();
        let thread = std::thread::spawn(move || {
            while let Ok(mut document) = receive.recv() {
                while let Ok(newer) = receive.try_recv() {
                    document = newer;
                }
                let _ = write_graph(&path, &document);
            }
        });
        Self { send, thread }
    }

    fn publish(&self, document: Option<Arc<rb_model::GraphDocument>>) {
        if let Some(document) = document {
            let _ = self.send.send(document);
        }
    }

    fn stop(self) {
        drop(self.send);
        let _ = self.thread.join();
    }
}

/// Writes `document` to `path` through a temporary file in the same folder.
fn write_graph(path: &Path, document: &rb_model::GraphDocument) -> Result<(), String> {
    let folder = path.parent().unwrap_or(path);
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let text = serde_json::to_string(document).map_err(|e| e.to_string())?;
    let temporary = folder.join(format!("graph.{}.tmp", std::process::id()));
    std::fs::write(&temporary, text).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())
}

/// Writes the findings file through a temporary file in the same folder, so a reader never
/// sees half of one.
fn write(ctx: &Context<'_>, findings: &Findings) -> Result<(), String> {
    let path = ctx.resolve(FINDINGS);
    let folder = path.parent().unwrap_or(&ctx.cwd).to_path_buf();
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
    let mut text = serde_json::to_string_pretty(findings).map_err(|e| e.to_string())?;
    text.push('\n');
    let temporary = folder.join(format!("findings.{}.tmp", std::process::id()));
    std::fs::write(&temporary, text)
        .map_err(|e| format!("cannot write {}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, &path).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// One check's result: the answer or why there is none, the files read again (none for a full
/// read) with the newest modification time among them, and the stages' times.
struct Check {
    answered: Result<String, String>,
    /// The document answered anew, for [`GUARD_GRAPH`].
    published: Option<Arc<rb_model::GraphDocument>>,
    rechecked: Vec<String>,
    newest: Option<u64>,
    timings: Timings,
    /// When the scan that led to this check started.
    scanned: u64,
}

/// Extracts as `plans` say (in full with `None`), then answers; the state keeps the new parts.
/// `scanned` is when the scan that found the change started: the answer reflects every change
/// made before it.
fn check(
    ctx: &mut Context<'_>,
    hook: &CruiseArgs,
    state: &mut State,
    plans: Option<Plans>,
    (rechecked, newest): (Vec<String>, Option<u64>),
    scanned: u64,
    publish: bool,
) -> Check {
    let started = Instant::now();
    let since = now_ns();
    let extracted = match plans {
        None => match build(ctx, hook) {
            Ok(built) => {
                *state = built;
                Ok(())
            }
            Err((_, message)) => {
                // The state is the earlier one, which did not see what asked for this read: the
                // next scan asks again.
                state
                    .watched
                    .unsettled
                    .get_or_insert_with(|| "the last full read failed".into());
                Err(message)
            }
        },
        Some(plans) => match pipeline::extract_parts(ctx, &state.effective, &hook.paths, plans) {
            Ok(parts) => {
                // A file the change made reachable is watched from now on.
                state
                    .watched
                    .add_sources(ctx, &state.effective, &parts, since);
                state.parts = parts;
                Ok(())
            }
            Err(error) => {
                state.stale = true;
                Err(error.to_string())
            }
        },
    };
    let extract = millis(started.elapsed());
    let (answered, published) = match extracted {
        Ok(()) => answer(ctx, hook, state, publish),
        Err(message) => (Err(message), None),
    };
    Check {
        answered,
        published,
        rechecked,
        newest,
        timings: Timings {
            extract,
            answer: millis(started.elapsed()).saturating_sub(extract),
        },
        scanned,
    }
}

/// Checks again what `change` names: everything for a structural change, else the changed
/// sources alone. Never called with [`Change::None`].
fn recheck(
    ctx: &mut Context<'_>,
    hook: &CruiseArgs,
    state: &mut State,
    change: Change,
    scanned: u64,
    log: &mut dyn Write,
) -> Check {
    match change {
        Change::None | Change::Structural(_) => {
            if let Change::Structural(reason) = &change {
                let _ = writeln!(log, "guard: {reason}; reading everything again");
            }
            check(ctx, hook, state, None, (Vec::new(), None), scanned, true)
        }
        Change::Sources(changed, newest) => {
            let plans = plans(&mut state.parts, &changed);
            let rechecked = changed
                .iter()
                .map(|(_, _, path)| key::slashed(path.strip_prefix(&ctx.cwd).unwrap_or(path)))
                .collect();
            check(
                ctx,
                hook,
                state,
                Some(plans),
                (rechecked, Some(newest)),
                scanned,
                true,
            )
        }
    }
}

/// The findings for a check of `state`.
fn findings(hook: &CruiseArgs, state: &State, check: Check) -> Findings {
    let written_at = now_ms();
    let (answer, error) = match check.answered {
        Ok(answer) => (answer, None),
        Err(error) => (String::new(), Some(error)),
    };
    Findings {
        tool: tool(),
        written_at,
        seen_up_to: check.scanned,
        config_hash: state.config_hash.clone(),
        key: answer_key(hook),
        // The flag wins over the key, so this is how .NET is read wherever it is read at all.
        mode: if hook.mode == Some(ModeArg::Compiled) {
            "compiled".to_owned()
        } else {
            "source".to_owned()
        },
        answer,
        rechecked: check.rechecked,
        latency_ms: check
            .newest
            .map(|ns| written_at.saturating_sub(ns / 1_000_000)),
        error,
        timings: Some(check.timings),
    }
}

fn describe(findings: &Findings) -> String {
    let verdict = match (&findings.error, findings.answer.is_empty()) {
        (Some(error), _) => format!("no answer: {error}"),
        (None, true) => "clean: the hook lets the turn end".to_owned(),
        (None, false) => "errors: the hook will keep the agent going".to_owned(),
    };
    let split = findings.timings.map_or_else(String::new, |t| {
        format!(" (extract {} ms, answer {} ms)", t.extract, t.answer)
    });
    match (findings.rechecked.as_slice(), findings.latency_ms) {
        ([], _) => format!("guard: read everything{split}; {verdict}"),
        (files, Some(ms)) => format!(
            "guard: checked {} again in {ms} ms{split}; {verdict}",
            files.join(", ")
        ),
        (files, None) => format!(
            "guard: checked {} again{split}; {verdict}",
            files.join(", ")
        ),
    }
}

/// Runs `guard`: one answer, or with `--watch` answers kept current until `stop` says so.
/// Messages go to `log`; the exit code is 0 unless the first extraction fails.
pub fn run(
    ctx: &mut Context<'_>,
    args: &GuardArgs,
    stop: &dyn Fn() -> bool,
    log: &mut dyn Write,
) -> u8 {
    if args.config.config.as_deref() == Some("-") {
        let _ = writeln!(
            log,
            "rulebearing guard: --config - reads standard input, which guard keeps to know when to stop; name the file"
        );
        return RunExit::InvalidConfig.code();
    }
    let hook = args.hook();
    let scanned = now_ms();
    let started = Instant::now();
    let mut state = match build(ctx, &hook) {
        Ok(state) => state,
        Err((code, message)) => {
            let _ = writeln!(log, "rulebearing guard: {message}");
            return code.code();
        }
    };
    let extract = millis(started.elapsed());
    let (answered, published) = answer(ctx, &hook, &mut state, args.watch);
    let timings = Timings {
        extract,
        answer: millis(started.elapsed()).saturating_sub(extract),
    };
    let first = Check {
        answered,
        published,
        rechecked: Vec::new(),
        newest: None,
        timings,
        scanned,
    };
    let writer = args
        .watch
        .then(|| GraphWriter::start(ctx.resolve(GUARD_GRAPH)));
    if let Some(writer) = &writer {
        writer.publish(first.published.clone());
    }
    let mut current = findings(&hook, &state, first);
    if let Err(message) = write(ctx, &current) {
        let _ = writeln!(log, "rulebearing guard: {message}");
        return RunExit::Untrustworthy.code();
    }
    let _ = writeln!(log, "{}", describe(&current));
    let Some(writer) = writer else {
        return 0;
    };
    let _ = writeln!(
        log,
        "guard: watching {} source(s) and {} other file(s); {FINDINGS} stays current until standard input closes",
        state.watched.sources.len(),
        state.watched.structural.len()
    );
    let interval = Duration::from_millis(args.interval);
    let mut beat = Instant::now();
    while !stop() {
        std::thread::sleep(interval);
        let scanned = now_ms();
        let change = match state.watched.check(&ctx.cwd) {
            Change::Sources(..) if state.stale => {
                Change::Structural("the last check failed".into())
            }
            change => change,
        };
        let done = match change {
            Change::None => {
                // Nothing changed up to this scan: a hook that asked before it can have the
                // answer now, and the heartbeat keeps it fresh.
                let asked = requested_after(ctx, current.seen_up_to);
                if asked || beat.elapsed() >= Duration::from_millis(HEARTBEAT_MS) {
                    current.written_at = now_ms();
                    current.seen_up_to = scanned;
                    let _ = write(ctx, &current);
                    beat = Instant::now();
                }
                continue;
            }
            change => recheck(ctx, &hook, &mut state, change, scanned, log),
        };
        writer.publish(done.published.clone());
        current = findings(&hook, &state, done);
        if let Err(message) = write(ctx, &current) {
            let _ = writeln!(log, "rulebearing guard: {message}");
        }
        let _ = writeln!(log, "{}", describe(&current));
        beat = Instant::now();
    }
    writer.stop();
    let _ = std::fs::remove_file(ctx.resolve(FINDINGS));
    let _ = std::fs::remove_file(ctx.resolve(REQUEST));
    let _ = std::fs::remove_file(ctx.resolve(GUARD_GRAPH));
    let _ = writeln!(log, "guard: standard input closed; stopped");
    0
}

/// `guard` from the library: one answer. `--watch` needs the process's own standard input to
/// know when to stop, which only the binary holds ([`crate::run_daemon`]).
pub fn once(ctx: &mut Context<'_>, args: &GuardArgs) -> Outcome {
    if args.watch {
        return Outcome::failed(
            RunExit::InvalidConfig,
            "rulebearing guard --watch: run it from the rulebearing binary, which watches its own standard input to know when to stop\n",
        );
    }
    let mut log = Vec::new();
    let code = run(ctx, args, &|| true, &mut log);
    Outcome {
        stdout: String::new(),
        stderr: String::from_utf8_lossy(&log).into_owned(),
        code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context<'a>(dir: &Path, stdin: &'a mut dyn std::io::Read) -> Context<'a> {
        Context {
            cwd: dir.to_path_buf(),
            stdin,
            today: chrono::NaiveDate::default(),
            timestamp: String::new(),
            color_terminal: false,
            warm: None,
        }
    }

    /// TypeScript parts whose extraction read `files`.
    fn read(files: &[&str]) -> Parts {
        Parts {
            typescript: Some(Extraction {
                files: files
                    .iter()
                    .map(|f| ((*f).to_owned(), rb_model::FileState::default()))
                    .collect(),
                ..Extraction::default()
            }),
            ..Parts::default()
        }
    }

    fn folder(name: &str) -> std::io::Result<PathBuf> {
        let dir = std::env::temp_dir().join(format!("rb-guard-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src"))?;
        dir.canonicalize()
    }

    fn pause() {
        std::thread::sleep(Duration::from_millis(20));
    }

    #[test]
    fn a_stamp_counts_as_after_within_the_slack_of_how_it_was_recorded() {
        let since = 100 * 1_000_000_000 + 500_000_000;
        for (modified, expected) in [
            (since, true),
            (since + 1, true),
            // A coarse kernel clock: up to 10 ms behind.
            (since - STAMP_SLACK_NS, true),
            (since - STAMP_SLACK_NS - 1, false),
            // Whole seconds: a file system that rounds down by up to two.
            (100 * 1_000_000_000, true),
            (99 * 1_000_000_000, true),
            (98 * 1_000_000_000, false),
        ] {
            assert_eq!(after(Some((1, modified)), since), expected, "{modified}");
        }
        assert!(!after(None, since));
    }

    #[test]
    fn the_guard_s_own_folder_is_not_watched() -> std::io::Result<()> {
        let dir = folder("own")?;
        std::fs::create_dir_all(dir.join(".graph/guard"))?;
        let mut stdin = std::io::empty();
        let ctx = context(&dir, &mut stdin);
        let mut parts = read(&["src/a.ts"]);
        if let Some(typescript) = parts.typescript.as_mut() {
            typescript.walk = Some(rb_model::Walk {
                folders: vec![
                    ".".into(),
                    "src".into(),
                    ".graph".into(),
                    ".graph/guard".into(),
                ],
                ..rb_model::Walk::default()
            });
        }
        pause();
        // Written now, as the guard writes its findings just before a scan.
        std::fs::write(dir.join(".graph/guard/findings.json"), "{}")?;
        let watched = Watched::of(&ctx, &Config::default(), &parts, now_ns());
        let walked: Vec<&PathBuf> = watched.walked.keys().collect();
        assert_eq!(walked, [&dir.join("."), &dir.join("src")]);
        assert!(watched.unsettled.is_none());
        std::fs::remove_dir_all(&dir)
    }

    #[test]
    fn an_unchanged_graph_is_answered_as_before_and_a_changed_one_anew() -> std::io::Result<()> {
        let dir = folder("repeat")?;
        std::fs::create_dir_all(dir.join("lib"))?;
        std::fs::write(dir.join("lib/b.ts"), "export const b = 1;\n")?;
        std::fs::write(
            dir.join("src/a.ts"),
            "import { b } from \"../lib/b\";\nexport const a = b;\n",
        )?;
        std::fs::write(
            dir.join("rulebearing.yaml"),
            "rules:\n  dependencies:\n    forbidden:\n      - name: r\n        comment: \"plan:rulebearing-wave-3\"\n        severity: error\n        from: { path: \"^src/\" }\n        to: { path: \"^lib/\" }\n",
        )?;
        let mut stdin = std::io::empty();
        let mut ctx = context(&dir, &mut stdin);
        let hook = GuardArgs::default().hook();
        let mut state = build(&mut ctx, &hook).map_err(|(_, e)| std::io::Error::other(e))?;
        let (first, published) = answer(&mut ctx, &hook, &mut state, true);
        let published = published.ok_or_else(|| std::io::Error::other("nothing published"))?;
        assert!(
            published.modules.iter().any(|m| m.source == "src/a.ts"),
            "a document answered anew is published"
        );
        assert!(
            first
                .as_ref()
                .is_ok_and(|a| a.contains("\"decision\":\"block\""))
        );
        // The kept answer is what the same graph gets: mark it to see that it is given again.
        if let Some(kept) = state.answered.as_mut() {
            assert_eq!(kept.answer, first);
            kept.answer = Ok("as before".into());
        }
        let (again, republished) = answer(&mut ctx, &hook, &mut state, true);
        assert_eq!(again, Ok("as before".into()));
        assert!(
            republished.is_none(),
            "the file already holds an unchanged graph"
        );
        // A graph that differs by one edge is evaluated.
        if let Some(typescript) = state.parts.typescript.as_mut() {
            for module in &mut typescript.modules {
                module.dependencies.clear();
            }
        }
        let (changed, unpublished) = answer(&mut ctx, &hook, &mut state, false);
        assert!(
            unpublished.is_none(),
            "nothing is published without --watch"
        );
        assert_ne!(changed, Ok("as before".into()));
        assert_ne!(changed, first);
        // With --affected the answer follows what git calls changed, so none is kept.
        let affected = CruiseArgs {
            affected: Some("HEAD".into()),
            ..hook.clone()
        };
        state.answered = None;
        let (_, published) = answer(&mut ctx, &affected, &mut state, true);
        assert!(state.answered.is_none());
        assert!(
            published.is_some(),
            "a graph that is not kept is still published"
        );
        std::fs::remove_dir_all(&dir)
    }

    #[test]
    fn a_full_read_that_fails_is_asked_for_again() -> std::io::Result<()> {
        let dir = folder("retry")?;
        std::fs::write(dir.join("src/a.ts"), "export const a = 1;\n")?;
        std::fs::write(
            dir.join("rulebearing.yaml"),
            "rules:\n  dependencies:\n    forbidden:\n      - name: r\n        comment: \"plan:rulebearing-wave-3\"\n        severity: error\n        allowEmpty: true\n        from: { path: \"^src/\" }\n        to: { path: \"^lib/\" }\n",
        )?;
        let mut stdin = std::io::empty();
        let mut ctx = context(&dir, &mut stdin);
        let hook = GuardArgs::default().hook();
        let mut state = build(&mut ctx, &hook).map_err(|(_, e)| std::io::Error::other(e))?;
        // The scan that asked for the read has taken its reason; then the read fails.
        state.watched.unsettled = None;
        std::fs::write(dir.join("rulebearing.yaml"), "rules: [not, a, rule, set\n")?;
        let failed = check(
            &mut ctx,
            &hook,
            &mut state,
            None,
            (Vec::new(), None),
            now_ms(),
            true,
        );
        assert!(failed.answered.is_err());
        assert!(
            failed.published.is_none(),
            "a failed read publishes nothing"
        );
        assert_eq!(
            state.watched.check(&dir),
            Change::Structural("the last full read failed".into())
        );
        std::fs::remove_dir_all(&dir)
    }

    #[test]
    fn what_changed_while_it_was_read_is_not_taken_as_seen() -> std::io::Result<()> {
        let dir = folder("unsettled")?;
        std::fs::write(dir.join("src/a.ts"), "export const a = 1;\n")?;
        pause();
        let since = now_ns();
        pause();
        // Written while the extraction runs: the extraction may have read it before or after.
        std::fs::write(dir.join("src/b.ts"), "export const b = 1;\n")?;
        let mut stdin = std::io::empty();
        let ctx = context(&dir, &mut stdin);
        let mut watched = Watched::of(
            &ctx,
            &Config::default(),
            &read(&["src/a.ts", "src/b.ts"]),
            since,
        );
        assert!(watched.sources[&dir.join("src/a.ts")].2.is_some());
        assert_eq!(watched.sources[&dir.join("src/b.ts")].2, None);
        // The folder gained b.ts during the read, so the first scan reads everything.
        assert_eq!(
            watched.check(&dir),
            Change::Structural("src changed while it was being read".into())
        );
        // A source changed during the read is checked again; once seen, it is settled.
        assert!(matches!(
            watched.check(&dir),
            Change::Sources(changed, _) if changed.len() == 1 && changed[0].1 == "src/b.ts"
        ));
        assert_eq!(watched.check(&dir), Change::None);
        // Nothing modified since: everything is seen as it is.
        pause();
        let settled = Watched::of(
            &ctx,
            &Config::default(),
            &read(&["src/a.ts", "src/b.ts"]),
            now_ns(),
        );
        assert!(settled.unsettled.is_none());
        assert!(settled.sources.values().all(|(_, _, s)| s.is_some()));
        std::fs::remove_dir_all(&dir)
    }

    #[test]
    fn a_file_a_check_reaches_first_is_watched_from_then_on() -> std::io::Result<()> {
        let dir = folder("reached")?;
        std::fs::create_dir_all(dir.join("lib"))?;
        std::fs::write(dir.join("src/a.ts"), "export const a = 1;\n")?;
        std::fs::write(dir.join("lib/c.ts"), "export const c = 1;\n")?;
        pause();
        let mut stdin = std::io::empty();
        let ctx = context(&dir, &mut stdin);
        let config = Config::default();
        let mut watched = Watched::of(&ctx, &config, &read(&["src/a.ts"]), now_ns());
        assert!(!watched.sources.contains_key(&dir.join("lib/c.ts")));
        // An edit to a.ts made lib/c.ts reachable.
        watched.add_sources(&ctx, &config, &read(&["src/a.ts", "lib/c.ts"]), now_ns());
        assert!(watched.sources.contains_key(&dir.join("lib/c.ts")));
        assert!(watched.folders.contains_key(&dir.join("lib")));
        assert_eq!(watched.check(&dir), Change::None);
        pause();
        std::fs::write(dir.join("lib/c.ts"), "export const c = 2;\n")?;
        assert!(matches!(
            watched.check(&dir),
            Change::Sources(changed, _) if changed[0].1 == "lib/c.ts"
        ));
        std::fs::remove_dir_all(&dir)
    }

    #[test]
    fn the_key_ignores_how_the_answer_is_printed_and_reached() {
        let hook = GuardArgs::default().hook();
        let printed = CruiseArgs {
            output_type: Some("agent".into()),
            output_to: Some("-".into()),
            no_progress: true,
            cache: Some(String::new()),
            mode: Some(ModeArg::Compiled),
            allow_approximate_gate: true,
            ..hook.clone()
        };
        assert_eq!(answer_key(&hook), answer_key(&printed));
        assert_eq!(answer_key(&hook).len(), 64);
        let narrower = CruiseArgs {
            affected: Some("HEAD".into()),
            ..hook.clone()
        };
        assert_ne!(answer_key(&hook), answer_key(&narrower));
        let elsewhere = CruiseArgs {
            paths: vec!["src".into()],
            ..hook
        };
        assert_ne!(answer_key(&elsewhere), answer_key(&narrower));
    }

    #[test]
    fn the_guard_answers_as_the_hook_in_source_mode_unless_told() {
        let hook = GuardArgs::default().hook();
        assert!(hook.from_hook);
        assert_eq!(hook.mode, Some(ModeArg::Source));
        let compiled = GuardArgs {
            mode: Some(ModeArg::Compiled),
            ..GuardArgs::default()
        }
        .hook();
        assert_eq!(compiled.mode, Some(ModeArg::Compiled));
    }

    #[test]
    fn only_sources_and_manifests_are_relevant_in_a_folder() {
        for (name, cs, expected) in [
            ("a.ts", false, true),
            ("a.py", false, true),
            ("package.json", false, true),
            ("App.csproj", false, true),
            ("a.cs", false, false),
            ("a.cs", true, true),
            ("a.dll", true, false),
            (".a.ts.swp", false, false),
            ("4913", false, false),
        ] {
            assert_eq!(relevant(name, cs), expected, "{name} cs={cs}");
        }
    }

    #[cfg(feature = "extract-ts")]
    #[test]
    fn a_walked_folder_lists_its_subfolders_and_the_files_the_walk_gathers() -> std::io::Result<()>
    {
        let dir = std::env::temp_dir().join(format!("rb-guard-listing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub"))?;
        for name in [
            "a.ts",
            "b.d.ts",
            ".a.ts.swp",
            "4913",
            "logo.svg",
            "notes.md",
        ] {
            std::fs::write(dir.join(name), "")?;
        }
        let plain = rb_model::TypeScriptOptions::default();
        let names: Vec<String> = walk_listing(&dir, &plain).into_iter().collect();
        assert_eq!(names, ["a.ts", "b.d.ts", "sub/"]);
        let markdown: rb_model::TypeScriptOptions =
            serde_json::from_str(r#"{"extraExtensionsToScan": [".md"]}"#).unwrap_or_default();
        assert!(walk_listing(&dir, &markdown).contains("notes.md"));
        assert!(walk_listing(&dir.join("absent"), &plain).is_empty());
        std::fs::remove_dir_all(&dir)
    }

    #[test]
    fn a_description_says_what_the_hook_will_do() {
        let base = Findings {
            tool: tool(),
            written_at: 1,
            seen_up_to: 1,
            config_hash: String::new(),
            key: String::new(),
            mode: "source".into(),
            answer: String::new(),
            rechecked: Vec::new(),
            latency_ms: None,
            error: None,
            timings: None,
        };
        assert_eq!(
            describe(&base),
            "guard: read everything; clean: the hook lets the turn end"
        );
        let blocked = Findings {
            answer: "{}".into(),
            rechecked: vec!["src/a.ts".into()],
            latency_ms: Some(12),
            ..base.clone()
        };
        assert_eq!(
            describe(&blocked),
            "guard: checked src/a.ts again in 12 ms; errors: the hook will keep the agent going"
        );
        let failed = Findings {
            error: Some("rulebearing cruise: no modules".into()),
            ..base
        };
        assert!(describe(&failed).ends_with("no answer: rulebearing cruise: no modules"));
    }
}
