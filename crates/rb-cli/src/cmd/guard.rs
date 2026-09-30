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
//! files, the built assemblies in compiled mode), and the folders holding the sources. A saved
//! source is extracted again alone, through the extractor's incremental entry, and the answer
//! rewritten; a changed structural file, or a source added to or removed from a folder, reads
//! everything again. While nothing changes the file is rewritten every [`HEARTBEAT_MS`], so its
//! age says the daemon is alive and has seen every change up to then. .NET is read in source mode
//! unless `--mode compiled` is given, since a daemon that waited for a build would not be one.
//! It polls rather than subscribing to file-system events: `notify` is CC0-1.0, outside the
//! licence allow-list, and the plan names polling as the fallback (§ 1.8). It writes nothing
//! outside `.graph/guard/`, logs to stderr only, removes the findings file when it stops, and
//! stops when standard input closes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
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
    /// The configuration files' hash ([`key::config_hash`]).
    pub config_hash: String,
    /// The command line answered for ([`answer_key`]).
    pub key: String,
    /// How .NET was read: `source` or `compiled`.
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

/// The answer a running guard has for `args`, when it is fresh, for this command line, this
/// configuration and this build.
pub fn served(ctx: &mut Context<'_>, args: &CruiseArgs) -> Option<String> {
    if args.config.config.as_deref() == Some("-") {
        return None;
    }
    let text = std::fs::read_to_string(ctx.resolve(FINDINGS)).ok()?;
    let findings: Findings = serde_json::from_str(&text).ok()?;
    let now = now_ms();
    let fresh = findings.written_at <= now.saturating_add(HEARTBEAT_MS)
        && now.saturating_sub(findings.written_at) < FRESH_MS;
    if !fresh || findings.tool != tool() || findings.key != answer_key(args) {
        return None;
    }
    let loaded = configure::load(ctx, &args.config).ok()?;
    (findings.config_hash == config_hash(ctx, loaded.as_ref())).then_some(findings.answer)
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
    /// `.cs` files are sources.
    cs: bool,
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

impl Watched {
    fn of(ctx: &Context<'_>, config: &Config, parts: &Parts) -> Self {
        let mut watched = Self {
            cs: pipeline::dotnet_source_mode(config),
            ..Self::default()
        };
        let base = ctx.cwd.join(
            config
                .languages
                .typescript
                .base_dir
                .as_deref()
                .unwrap_or(""),
        );
        for (part, extraction, folder) in [
            (Part::TypeScript, &parts.typescript, &base),
            (Part::Dotnet, &parts.dotnet, &ctx.cwd),
            (Part::Python, &parts.python, &ctx.cwd),
        ] {
            for name in extraction.iter().flat_map(|e| e.files.keys()) {
                let path = folder.join(name);
                let stamped = stamp(&path);
                watched.sources.insert(path, (part, name.clone(), stamped));
            }
        }
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
            structural
                .extend(rb_extract_dotnet::project_files(&ctx.cwd, &options).unwrap_or_default());
            if !watched.cs {
                structural.extend(
                    rb_extract_dotnet::assembly_inputs(&ctx.cwd, &options).unwrap_or_default(),
                );
            }
        }
        for path in structural {
            let path = ctx.resolve(path);
            let stamped = stamp(&path);
            watched.structural.insert(path, stamped);
        }
        let folders: BTreeSet<PathBuf> = watched
            .sources
            .keys()
            .filter_map(|p| p.parent().map(Path::to_path_buf))
            .collect();
        for folder in folders {
            let names = listing(&folder, watched.cs);
            watched
                .folders
                .insert(folder.clone(), (stamp(&folder), names));
        }
        watched
    }

    /// Checks every watched file once, recording the new stamps of the sources it reports.
    fn check(&mut self, cwd: &Path) -> Change {
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

/// The plans that read the changed sources again and reuse everything else.
fn plans(parts: &Parts, changed: &[(Part, String, PathBuf)]) -> Plans {
    let plan = |part: Part, extraction: &Option<Extraction>| {
        let names: Vec<PathBuf> = changed
            .iter()
            .filter(|(p, _, _)| *p == part)
            .map(|(_, name, _)| PathBuf::from(name))
            .collect();
        if names.is_empty() {
            return Plan::Reuse(extraction.clone());
        }
        let unchanged = extraction
            .iter()
            .flat_map(|e| e.files.keys())
            .map(PathBuf::from)
            .filter(|n| !names.contains(n))
            .collect();
        Plan::Incremental(ExtractRequest {
            changed: names,
            unchanged,
            previous: extraction.clone().unwrap_or_default(),
        })
    };
    Plans {
        typescript: plan(Part::TypeScript, &parts.typescript),
        dotnet: plan(Part::Dotnet, &parts.dotnet),
        python: plan(Part::Python, &parts.python),
        keep_file_states: true,
    }
}

/// What the guard holds between checks.
struct State {
    loaded: Option<Config>,
    effective: Config,
    parts: Parts,
    watched: Watched,
}

/// Loads the configuration and extracts in full.
fn build(ctx: &mut Context<'_>, hook: &CruiseArgs) -> Result<State, (RunExit, String)> {
    let loaded =
        configure::load(ctx, &hook.config).map_err(|e| (RunExit::InvalidConfig, e.to_string()))?;
    let mut effective = loaded.clone().unwrap_or_default();
    configure::apply_flags(&mut effective, hook, ctx)
        .map_err(|e| (RunExit::InvalidConfig, e.to_string()))?;
    let plans = Plans {
        keep_file_states: true,
        ..Plans::default()
    };
    let parts = pipeline::extract_parts(ctx, &effective, &hook.paths, &plans)
        .map_err(|e| (RunExit::Untrustworthy, e.to_string()))?;
    let watched = Watched::of(ctx, &effective, &parts);
    Ok(State {
        loaded,
        effective,
        parts,
        watched,
    })
}

/// The hook's answer for the state's extraction, or why there is none.
fn answer(ctx: &mut Context<'_>, hook: &CruiseArgs, state: &State) -> Result<String, String> {
    let (document, warnings) =
        pipeline::merge(&state.effective, &state.parts).map_err(|e| e.to_string())?;
    let outcome = cruise::hook_answer(ctx, hook, Given { document, warnings });
    if outcome.stdout.is_empty()
        && let Some(line) = outcome
            .stderr
            .lines()
            .find(|l| l.starts_with("rulebearing cruise:"))
    {
        return Err(line.to_owned());
    }
    Ok(outcome.stdout)
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
    rechecked: Vec<String>,
    newest: Option<u64>,
    timings: Timings,
}

/// Extracts as `plans` say (in full with `None`), then answers; the state keeps the new parts.
fn check(
    ctx: &mut Context<'_>,
    hook: &CruiseArgs,
    state: &mut State,
    plans: Option<Plans>,
    rechecked: Vec<String>,
    newest: Option<u64>,
) -> Check {
    let started = Instant::now();
    let extracted = match plans {
        None => build(ctx, hook)
            .map(|built| *state = built)
            .map_err(|(_, message)| message),
        Some(plans) => pipeline::extract_parts(ctx, &state.effective, &hook.paths, &plans)
            .map(|parts| state.parts = parts)
            .map_err(|e| e.to_string()),
    };
    let extract = millis(started.elapsed());
    let answered = extracted.and_then(|()| answer(ctx, hook, state));
    Check {
        answered,
        rechecked,
        newest,
        timings: Timings {
            extract,
            answer: millis(started.elapsed()).saturating_sub(extract),
        },
    }
}

/// The findings for a check of `state`.
fn findings(ctx: &Context<'_>, hook: &CruiseArgs, state: &State, check: Check) -> Findings {
    let written_at = now_ms();
    let (answer, error) = match check.answered {
        Ok(answer) => (answer, None),
        Err(error) => (String::new(), Some(error)),
    };
    Findings {
        tool: tool(),
        written_at,
        config_hash: config_hash(ctx, state.loaded.as_ref()),
        key: answer_key(hook),
        mode: if pipeline::dotnet_source_mode(&state.effective) {
            "source".to_owned()
        } else {
            "compiled".to_owned()
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
    let started = Instant::now();
    let mut state = match build(ctx, &hook) {
        Ok(state) => state,
        Err((code, message)) => {
            let _ = writeln!(log, "rulebearing guard: {message}");
            return code.code();
        }
    };
    let extract = millis(started.elapsed());
    let answered = answer(ctx, &hook, &state);
    let timings = Timings {
        extract,
        answer: millis(started.elapsed()).saturating_sub(extract),
    };
    let first = Check {
        answered,
        rechecked: Vec::new(),
        newest: None,
        timings,
    };
    let mut current = findings(ctx, &hook, &state, first);
    if let Err(message) = write(ctx, &current) {
        let _ = writeln!(log, "rulebearing guard: {message}");
        return RunExit::Untrustworthy.code();
    }
    let _ = writeln!(log, "{}", describe(&current));
    if !args.watch {
        return 0;
    }
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
        let done = match state.watched.check(&ctx.cwd) {
            Change::None => {
                if beat.elapsed() >= Duration::from_millis(HEARTBEAT_MS) {
                    current.written_at = now_ms();
                    let _ = write(ctx, &current);
                    beat = Instant::now();
                }
                continue;
            }
            Change::Structural(reason) => {
                let _ = writeln!(log, "guard: {reason}; reading everything again");
                check(ctx, &hook, &mut state, None, Vec::new(), None)
            }
            Change::Sources(changed, newest) => {
                let plans = plans(&state.parts, &changed);
                let rechecked = changed
                    .iter()
                    .map(|(_, _, path)| key::slashed(path.strip_prefix(&ctx.cwd).unwrap_or(path)))
                    .collect();
                check(ctx, &hook, &mut state, Some(plans), rechecked, Some(newest))
            }
        };
        current = findings(ctx, &hook, &state, done);
        if let Err(message) = write(ctx, &current) {
            let _ = writeln!(log, "rulebearing guard: {message}");
        }
        let _ = writeln!(log, "{}", describe(&current));
        beat = Instant::now();
    }
    let _ = std::fs::remove_file(ctx.resolve(FINDINGS));
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

    #[test]
    fn a_description_says_what_the_hook_will_do() {
        let base = Findings {
            tool: tool(),
            written_at: 1,
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
