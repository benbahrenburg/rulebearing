//! `rulebearing diff`: what changed between two cruise results, or between a revision and the
//! working tree, for a review comment.
//!
//! - Source: [design § The subcommands a guard reaches for](../../../../docs/artifacts/design.md#the-subcommands-a-guard-reaches-for)
//!   ("`rulebearing diff <old.json> <new.json>` prints added and removed edges and new
//!   violations, for a review comment; `--base main` does the same against the base branch's
//!   cruise")
//! - Contract: [Wave 3 plan § 1.5](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//!   (the command line and the JSON shape)
//! - Plan: [Wave 3, Step 4](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)
//! - Decisions: [ADR-0015](../../../../docs/adr/0015-stable-violation-id.md) (violations are
//!   matched by stable id), [ADR-0008](../../../../docs/adr/0008-exit-code-contract.md) and
//!   [ADR-0030](../../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md) (the exit
//!   code)
//! - Requirement: [FR-CLI-01](../../../../docs/prd.md#fr-cli-01)
//!
//! `diff OLD.json NEW.json` reads two saved results, Rulebearing's or dependency-cruiser's, and
//! compares them ([`rb_report::diff`] says how). It reads nothing else, so the configuration flags
//! and `--no-cache` are refused without `--base` rather than ignored.
//!
//! `diff --base REF [FILES]` compares the cruise of `REF` with the cruise of the working tree. Both
//! sides run the full cruise (extraction, evaluation, ratchets) under one configuration, the one
//! the configuration flags find in the working tree, so the diff shows what the code changed and
//! not what an edit to the rules did. Liveness is off on both sides: `diff` reports, and a rule
//! that matches nothing is `cruise`'s finding. The base graph comes from the cache when an entry
//! for that commit exists; otherwise `REF` is checked out with `git worktree add --detach` into a
//! folder under the system temporary directory (git hooks off: `core.hooksPath` is a fresh,
//! private, empty folder), extracted there from the same subfolder the command runs
//! in, and the worktree is removed afterwards, on an error too. A path the base does not have, or a
//! base with no module, is an empty base graph: everything on the working-tree side is added. The
//! user's working tree is never touched. The extracted base graph is then written to the cache,
//! keyed on the repository root, the commit, the configuration, the build and the paths, so a
//! second `diff --base` against the same commit does not check it out again. `--no-cache` neither
//! reads nor writes that entry.
//!
//! A checkout holds what git tracks and nothing else: no installed packages and no build output.
//! An edge into `node_modules` therefore resolves differently on the base side than in a working
//! tree where the packages are installed, and a .NET solution has no assemblies to read until it
//! is built, which exits 2 with the extractor's reason. For those repositories, cruise each
//! revision where it is installed and built and compare the two results with `diff OLD NEW`.
//!
//! `diff` is a report, so it exits 0 whatever it found, as the non-gating reporters do
//! ([ADR-0030](../../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md)).
//! `--exit-code` makes it gate on what the change introduced: the exit code is the number of new
//! error-severity violations, capped at 255, with the same 2-and-3 ambiguity [ADR-0008](../../../../docs/adr/0008-exit-code-contract.md)
//! documents; `--exit-code-mode strict` shifts a non-zero count to `10 + n`, as on `cruise` and
//! `fmt`. An unreadable input, a file that is not a cruise result, an unknown revision, a
//! folder outside a repository and a base that cannot be cruised exit 2 with the reason; an invalid
//! configuration, an unknown output type or a wrong command line exit 3.
//! With `--exit-code`, a side read in source mode refuses the gate, exit 2, unless
//! `--allow-approximate-gate` ([`crate::exit::gate`];
//! [Wave 3, Step 15](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)).

use std::path::{Path, PathBuf};

use clap::Args;
use rb_config::Config;
use rb_model::GraphDocument;
use rb_report::diff::{self, Diff, Side};

use crate::cache::{self, CacheKey, key};
use crate::cli::{ConfigArgs, CruiseArgs};
use crate::context::Context;
use crate::exit::{APPROXIMATE_REASON, ExitCodeMode, RunExit, gate, is_approximate};
use crate::pipeline::{self, RunError, RunOptions};
use crate::progress::Progress;
use crate::{Outcome, configure, ratchets, write_output};

/// The exit codes of `diff`, printed under its help.
pub const DIFF_EXIT_CODES: &str = "Exit codes:
  0       the diff was written (with --exit-code: no new error-severity violation)
  1-255   with --exit-code: the number of new error-severity violations, capped at 255
          (with --exit-code-mode strict: 10 + the number, so 2 and 3 are never a count)
  2       an input cannot be read or is not a cruise result, the revision is unknown, or a side
          cannot be cruised
  3       the configuration, the output type or the command line is invalid";

/// `diff`.
#[derive(Debug, Clone, Default, Args)]
#[command(after_help = DIFF_EXIT_CODES)]
pub struct DiffArgs {
    /// Without --base: the old and the new cruise result. With --base: the files, directories
    /// and globs to cruise on both sides (default: the working directory)
    #[arg(value_name = "OLD-JSON NEW-JSON | FILES-OR-DIRECTORIES")]
    pub inputs: Vec<String>,
    /// Compare the cruise of this revision (a branch, tag or commit) with the working tree
    #[arg(long, value_name = "REF")]
    pub base: Option<String>,
    /// Configuration, with --base: one configuration, from the working tree, for both sides
    #[command(flatten)]
    pub config: ConfigArgs,
    /// Output type: json, markdown or agent
    #[arg(short = 'T', long, value_name = "TYPE", default_value = "json")]
    pub output_type: String,
    /// File to write output to; - for stdout
    #[arg(short = 'f', long, value_name = "FILE", default_value = "-")]
    pub output_to: String,
    /// Exit with the number of new error-severity violations
    #[arg(short = 'e', long)]
    pub exit_code: bool,
    /// With --exit-code: default (the count) or strict (10 + the count, so 2 and 3 are never a
    /// count)
    #[arg(
        long,
        value_enum,
        value_name = "MODE",
        default_value_t = ExitCodeMode::Default,
        requires = "exit_code"
    )]
    pub exit_code_mode: ExitCodeMode,
    /// Let a run read in source mode (--mode source) decide the exit code, for a local script;
    /// without it such a run exits 2, since its edges are approximate. Never in CI (ADR-0011)
    #[arg(long, requires = "exit_code")]
    pub allow_approximate_gate: bool,

    /// With --base: check the base out and extract it even when the cache holds its graph, and
    /// do not write the entry
    #[arg(long)]
    pub no_cache: bool,
}

fn failed(code: RunExit, message: &str) -> Outcome {
    Outcome::failed(code, format!("rulebearing diff: {message}\n"))
}

/// Whether any configuration flag was given.
fn config_given(args: &ConfigArgs) -> bool {
    args.config.is_some()
        || args.validate.is_some()
        || args.no_config
        || args.config_format.is_some()
        || args.config_via_node
        || args.strict_compat
        || args.require_comment_token
}

/// Runs `diff`.
pub fn run(ctx: &mut Context<'_>, args: &DiffArgs) -> Outcome {
    if !diff::DIFF_OUTPUT_TYPES.contains(&args.output_type.as_str()) {
        return failed(
            RunExit::InvalidConfig,
            &diff::DiffError::OutputType(args.output_type.clone()).to_string(),
        );
    }
    let result = match &args.base {
        None => saved(ctx, args),
        Some(reference) => against_base(ctx, args, reference),
    };
    let (diff, approximate) = match result {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    let text = match diff::render(&args.output_type, &diff) {
        Ok(text) => text,
        Err(e) => return failed(RunExit::InvalidConfig, &e.to_string()),
    };
    let mut stdout = String::new();
    if let Err(message) = write_output(ctx, &args.output_to, &text, &mut stdout) {
        return failed(RunExit::Untrustworthy, &message);
    }
    let mut code = if args.exit_code {
        RunExit::Violations(diff.new_errors())
    } else {
        RunExit::Violations(0)
    };
    let mut stderr = String::new();
    if let Some(refused) = gate(
        code,
        args.exit_code,
        approximate,
        args.allow_approximate_gate,
    ) {
        code = refused;
        stderr = format!("warning: {APPROXIMATE_REASON}\n");
    }
    Outcome {
        stdout,
        stderr,
        code: code.code_in(args.exit_code_mode),
    }
}

/// Reads a saved result.
fn read(ctx: &Context<'_>, file: &str) -> Result<GraphDocument, Outcome> {
    let path = ctx.resolve(file);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        failed(
            RunExit::Untrustworthy,
            &format!("cannot read {}: {e}", path.display()),
        )
    })?;
    rb_ingest::dependency_cruiser::read(&text).map_err(|e| {
        failed(
            RunExit::Untrustworthy,
            &format!(
                "{} is not a cruise result: {e}; write one with `rulebearing cruise -T json -f FILE`",
                path.display()
            ),
        )
    })
}

/// `diff OLD NEW`.
fn saved(ctx: &Context<'_>, args: &DiffArgs) -> Result<(Diff, bool), Outcome> {
    if config_given(&args.config) || args.no_cache {
        return Err(failed(
            RunExit::InvalidConfig,
            "the configuration flags and --no-cache apply to --base only; two saved results are compared as they are",
        ));
    }
    let [old, new] = args.inputs.as_slice() else {
        return Err(failed(
            RunExit::InvalidConfig,
            &format!(
                "give two cruise results, OLD.json NEW.json, or --base REF (got {} argument(s))",
                args.inputs.len()
            ),
        ));
    };
    let (old, new) = (read(ctx, old)?, read(ctx, new)?);
    let approximate = is_approximate(&old) || is_approximate(&new);
    Ok((diff::compute(&old, &new), approximate))
}

/// A folder this process created under the system temporary directory, removed with everything
/// in it when dropped: the empty `core.hooksPath` of every git call, and the parent of the base
/// checkout.
///
/// A fixed or guessable path in the shared temporary directory would let whoever creates it first
/// decide what is in it: a `post-checkout` that `git worktree add` then runs as the user, or a
/// folder (or a link) the checkout lands in. This folder is new (`create_dir` refuses an entry
/// that exists, of any kind, and a fresh name is tried), and private to the user on Unix (mode
/// 0700), so nobody else can put anything into it.
#[derive(Debug)]
pub(crate) struct PrivateDir {
    /// The folder.
    pub(crate) path: PathBuf,
}

/// A builder for a folder only its owner can enter: mode 0700 where the platform has modes.
fn private_dir_builder() -> std::fs::DirBuilder {
    #[cfg(unix)]
    {
        let mut builder = std::fs::DirBuilder::new();
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder
    }
    #[cfg(not(unix))]
    {
        std::fs::DirBuilder::new()
    }
}

impl PrivateDir {
    /// Creates `<temp>/<prefix><pid>-<nanos>-<n>`, trying up to sixteen names.
    ///
    /// # Errors
    /// The I/O error when no fresh folder can be created.
    pub(crate) fn create(prefix: &str) -> std::io::Result<Self> {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let names = std::iter::repeat_with(|| {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            std::env::temp_dir().join(format!("{prefix}{}-{nanos}-{n}", std::process::id()))
        })
        .take(16);
        Self::create_first(names)
    }

    /// Creates the first of `candidates` that does not exist yet; an existing entry is skipped,
    /// never used.
    ///
    /// # Errors
    /// The I/O error of the last attempt when every candidate exists, or the first other error.
    pub(crate) fn create_first(
        candidates: impl IntoIterator<Item = PathBuf>,
    ) -> std::io::Result<Self> {
        let mut last = std::io::Error::from(std::io::ErrorKind::AlreadyExists);
        for path in candidates {
            match private_dir_builder().create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }
}

impl Drop for PrivateDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// `git` in `dir`, with hooks off (`core.hooksPath` is a fresh, private, empty
/// [`PrivateDir`]); its trimmed stdout, or its stderr as the error.
pub(crate) fn git(dir: &Path, arguments: &[&str]) -> Result<String, String> {
    let no_hooks = PrivateDir::create("rulebearing-no-hooks-")
        .map_err(|e| format!("cannot create a private empty folder for git's hooks: {e}"))?;
    let output = crate::git::command()
        .arg("-c")
        .arg(format!("core.hooksPath={}", no_hooks.path.display()))
        .args(arguments)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    drop(no_hooks);
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// The commit `reference` names in the repository at `dir`.
///
/// # Errors
/// A message naming the revision when it is not one (a leading `-` is refused, so a revision is
/// never read as a git option).
pub fn resolve_revision(dir: &Path, reference: &str) -> Result<String, String> {
    let unknown = || {
        format!(
            "unknown revision `{reference}`: name a branch, tag or commit this repository has (fetch it first if it is remote)"
        )
    };
    if reference.is_empty() || reference.starts_with('-') {
        return Err(unknown());
    }
    git(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{reference}^{{commit}}"),
        ],
    )
    .ok()
    .filter(|sha| !sha.is_empty())
    .ok_or_else(unknown)
}

/// A detached checkout of one commit, in `base/` inside a `PrivateDir` of its own under the
/// system temporary directory; the worktree and the folder are removed when dropped.
#[derive(Debug)]
pub struct Checkout {
    /// The repository the worktree belongs to.
    repository: PathBuf,
    /// The worktree's folder.
    pub path: PathBuf,
    /// The private folder that holds it, removed after the worktree, as the field drops.
    folder: PrivateDir,
}

impl Checkout {
    /// Checks `sha` out of the repository at `repository` with `git worktree add --detach`.
    ///
    /// # Errors
    /// Git's message when the worktree cannot be added.
    pub fn add(repository: &Path, sha: &str) -> Result<Self, String> {
        let folder = PrivateDir::create("rulebearing-diff-").map_err(|e| {
            format!("cannot create a private folder for the checkout of {sha}: {e}")
        })?;
        let checkout = Self {
            repository: repository.to_path_buf(),
            path: folder.path.join("base"),
            folder,
        };
        let target = checkout.path.to_string_lossy().into_owned();
        git(
            repository,
            &["worktree", "add", "--quiet", "--detach", &target, sha],
        )
        .map_err(|e| format!("cannot check {sha} out into {target}: {e}"))?;
        Ok(checkout)
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        let target = self.path.to_string_lossy().into_owned();
        let _ = git(
            &self.repository,
            &["worktree", "remove", "--force", &target],
        );
        // What git left, and the private folder around it (dropping the field removes it again,
        // harmlessly).
        let _ = std::fs::remove_dir_all(&self.folder.path);
        let _ = git(&self.repository, &["worktree", "prune"]);
    }
}

/// The cache key of the base graph: the repository root, the commit, the configuration and the
/// build, as the working tree's key has, with inputs that say it is a clean checkout of that
/// commit extracted from `prefix` over `paths` (a checkout has no other inputs, so these name its
/// graph, and they can never equal a working tree's fingerprint, which is a hash).
pub fn base_key(
    root: &Path,
    config: &Config,
    sha: &str,
    prefix: &str,
    paths: &[String],
) -> CacheKey {
    CacheKey {
        root: root.to_string_lossy().replace('\\', "/"),
        head: sha.to_owned(),
        config_hash: key::config_hash(config, root),
        config_files: key::config_files(config, root),
        version: key::VERSION.to_owned(),
        inputs: format!(
            "checkout {sha} from `{prefix}` of {}",
            serde_json::to_string(paths).unwrap_or_default()
        ),
    }
}

/// Writes `document` as the base graph's cache entry: the key, then the graph under a temporary
/// name renamed into place, as the query commands' entries are written.
fn store(directory: &Path, key: &CacheKey, document: &GraphDocument) -> Result<(), String> {
    std::fs::create_dir_all(directory)
        .map_err(|e| format!("cannot create {}: {e}", directory.display()))?;
    let mut key_text = serde_json::to_string_pretty(&key.to_json()).map_err(|e| e.to_string())?;
    key_text.push('\n');
    let key_file = directory.join(cache::KEY_FILE);
    std::fs::write(&key_file, key_text)
        .map_err(|e| format!("cannot write {}: {e}", key_file.display()))?;
    let text = serde_json::to_string(document).map_err(|e| e.to_string())?;
    let graph = directory.join(cache::GRAPH_FILE);
    let temporary = directory.join(format!("{}.{}.tmp", cache::GRAPH_FILE, std::process::id()));
    std::fs::write(&temporary, text)
        .map_err(|e| format!("cannot write {}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, &graph)
        .map_err(|e| format!("cannot write {}: {e}", graph.display()))
}

/// The cached base graph, when the entry exists and reads back as a graph document.
fn cached(directory: &Path) -> Option<GraphDocument> {
    let text = std::fs::read_to_string(directory.join(cache::GRAPH_FILE)).ok()?;
    rb_ingest::dependency_cruiser::read(&text).ok()
}

/// The exit code and message for a side that cannot be cruised.
fn side_failed(side: &str, error: &RunError) -> Outcome {
    let code = error.exit();
    failed(code, &format!("{side} cannot be cruised: {error}"))
}

/// Evaluates an extracted graph as `cruise -T json` would, ratchets included, liveness off.
fn evaluate(
    ctx: &Context<'_>,
    config: &Config,
    options: &RunOptions,
    mut document: GraphDocument,
) -> Result<GraphDocument, RunError> {
    pipeline::reset(&mut document);
    let run =
        pipeline::evaluate_document(ctx, config, document, options, &mut Progress::new(None))?;
    let ratchets = ratchets::evaluate(ctx, config, run.evaluated(), false);
    let mut document = run.document;
    if !ratchets.results.is_empty() {
        document.summary.ratchets = Some(ratchets.results);
    }
    Ok(document)
}

/// The paths of `paths` the base checkout at `cwd` has: a glob (`*`, `?`, `[`, `{`) is kept as
/// it is, a literal path only when it exists there.
pub fn present_paths(cwd: &Path, paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|p| p.contains(['*', '?', '[', '{']) || cwd.join(p.as_str()).exists())
        .cloned()
        .collect()
}

/// The base graph, extracted: from the cache, else from a checkout (then cached). A base without
/// the given paths or without any module is an empty graph, not an error.
fn base_graph(
    ctx: &Context<'_>,
    config: &Config,
    args: &DiffArgs,
    root: &Path,
    sha: &str,
) -> Result<GraphDocument, Outcome> {
    let prefix = ctx.repository_prefix();
    let key = base_key(root, config, sha, &prefix, &args.inputs);
    let directory = key.directory(&ctx.cwd);
    if !args.no_cache
        && let Some(document) = cached(&directory)
    {
        return Ok(document);
    }
    let checkout = Checkout::add(root, sha).map_err(|e| failed(RunExit::Untrustworthy, &e))?;
    let mut empty: &[u8] = &[];
    // Canonical, as a process's working directory is: the extractors make paths relative to it,
    // and the temporary directory is often reached through a symbolic link (`/var` on macOS).
    let folder = checkout.path.join(&prefix);
    let base_ctx = Context {
        cwd: folder.canonicalize().unwrap_or(folder),
        stdin: &mut empty,
        today: ctx.today,
        timestamp: ctx.timestamp.clone(),
        color_terminal: false,
        warm: None,
    };
    let base = args.base.as_deref().unwrap_or(sha);
    // A path the change adds is not in the base yet, and a base with nothing to extract (a
    // first commit, a new package) is empty: both mean everything on the head side is added.
    let present = present_paths(&base_ctx.cwd, &args.inputs);
    let document = if !args.inputs.is_empty() && present.is_empty() {
        GraphDocument::default()
    } else {
        match pipeline::extract(&base_ctx, config, &present) {
            Ok(document) => document,
            Err(rb_model::ExtractError::NoModulesFound) => GraphDocument::default(),
            Err(e) => {
                return Err(side_failed(
                    &format!("the base `{base}` ({sha})"),
                    &RunError::Extract(e),
                ));
            }
        }
    };
    drop(checkout);
    if !args.no_cache {
        // A cache that cannot be written costs the next run a checkout, not this one its answer.
        if store(&directory, &key, &document).is_ok() {
            cache::prune(&ctx.cwd.join(key::CACHE_DIR), cache::KEEP);
        }
    }
    Ok(document)
}

/// `diff --base REF`.
fn against_base(
    ctx: &mut Context<'_>,
    args: &DiffArgs,
    reference: &str,
) -> Result<(Diff, bool), Outcome> {
    let root = git(&ctx.cwd, &["rev-parse", "--show-toplevel"])
        .ok()
        .filter(|r| !r.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            failed(
                RunExit::Untrustworthy,
                &format!(
                    "{} is not inside a git repository, so --base has nothing to check out; compare two saved results with `diff OLD.json NEW.json`",
                    ctx.cwd.display()
                ),
            )
        })?;
    let root = key::worktree_root(&root);
    let sha = resolve_revision(&root, reference).map_err(|e| failed(RunExit::Untrustworthy, &e))?;
    let loaded = configure::load(ctx, &args.config)
        .map_err(|e| side_failed("the configuration", &RunError::Config(e)))?;
    let has_config = loaded.is_some();
    let mut config = loaded.unwrap_or_default();
    // The defaults `cruise` lays over a configuration when no flag is given.
    let flags = CruiseArgs {
        config: args.config.clone(),
        ..CruiseArgs::default()
    };
    configure::apply_flags(&mut config, &flags, ctx)
        .map_err(|e| side_failed("the configuration", &RunError::Config(e)))?;
    config.options.metrics = Some(configure::wants_metrics(&config, false, "json"));
    let options = RunOptions {
        liveness: false,
        options_used: configure::options_used(has_config.then_some(&config), ctx, "json", "-"),
        paths: args.inputs.clone(),
        affected: None,
    };
    let head = pipeline::extract(ctx, &config, &args.inputs)
        .map_err(|e| side_failed("the working tree", &RunError::Extract(e)))?;
    let head =
        evaluate(ctx, &config, &options, head).map_err(|e| side_failed("the working tree", &e))?;
    let base = base_graph(ctx, &config, args, &root, &sha)?;
    let base = evaluate(ctx, &config, &options, base)
        .map_err(|e| side_failed(&format!("the base `{reference}` ({sha})"), &e))?;
    let head_sha = key::head(&root);
    let approximate = is_approximate(&base) || is_approximate(&head);
    let diff = diff::compute(&base, &head).with_sides(
        Side {
            revision: Some(reference.to_owned()),
            sha: Some(sha),
        },
        Side {
            revision: None,
            sha: (!head_sha.is_empty()).then_some(head_sha),
        },
    );
    Ok((diff, approximate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hooks_folder_is_fresh_empty_private_and_removed() -> std::io::Result<()> {
        let (a, b) = (
            PrivateDir::create("rb-test-hooks-")?,
            PrivateDir::create("rb-test-hooks-")?,
        );
        assert_ne!(a.path, b.path, "each call has its own folder");
        assert!(a.path.is_dir());
        assert_eq!(std::fs::read_dir(&a.path)?.count(), 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&a.path)?.permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
        let path = a.path.clone();
        drop(a);
        assert!(!path.exists());
        // git runs with it: a plain query answers.
        assert!(git(&std::env::temp_dir(), &["--version"]).is_ok_and(|v| v.starts_with("git")));
        Ok(())
    }

    #[test]
    fn an_entry_that_already_exists_at_a_name_is_never_used() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("rb-diff-guessed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        // Someone else got there first: a folder holding a hook, a plain file, and on Unix a link
        // to a folder of theirs.
        let planted = dir.join("planted");
        std::fs::create_dir_all(&planted)?;
        std::fs::write(planted.join("post-checkout"), "#!/bin/sh\n")?;
        std::fs::write(dir.join("file"), "x")?;
        let mut candidates = vec![planted.clone(), dir.join("file")];
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&planted, dir.join("link"))?;
            candidates.push(dir.join("link"));
        }
        candidates.push(dir.join("fresh"));
        let made = PrivateDir::create_first(candidates.clone())?;
        assert_eq!(made.path, dir.join("fresh"));
        assert_eq!(std::fs::read_dir(&made.path)?.count(), 0);
        drop(made);
        assert!(planted.join("post-checkout").is_file(), "left as it was");
        assert!(!dir.join("fresh").exists());
        // With every name taken, nothing is created and the error says so.
        candidates.pop();
        let none = PrivateDir::create_first(candidates);
        assert!(none.is_err_and(|e| e.kind() == std::io::ErrorKind::AlreadyExists));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn a_checkout_lives_in_a_private_folder_that_goes_with_it() -> Result<(), String> {
        let repo = std::env::temp_dir().join(format!("rb-diff-checkout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).map_err(|e| e.to_string())?;
        std::fs::write(repo.join("a.ts"), "export const a = 1;\n").map_err(|e| e.to_string())?;
        for args in [
            &["init", "--quiet"][..],
            &["add", "."],
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "one",
            ],
        ] {
            git(&repo, args)?;
        }
        let sha = resolve_revision(&repo, "HEAD")?;
        let checkout = Checkout::add(&repo, &sha)?;
        let parent = checkout
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        assert!(checkout.path.join("a.ts").is_file());
        assert!(
            parent
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("rulebearing-diff-"))
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&parent)
                .map_err(|e| e.to_string())?
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        drop(checkout);
        assert!(!parent.exists());
        let listed = git(&repo, &["worktree", "list", "--porcelain"])?;
        assert_eq!(listed.matches("worktree ").count(), 1, "{listed}");
        let _ = std::fs::remove_dir_all(&repo);
        Ok(())
    }

    #[test]
    fn only_the_paths_the_base_has_are_extracted() {
        let dir = std::env::temp_dir().join(format!("rb-diff-present-{}", std::process::id()));
        let _ = std::fs::create_dir_all(dir.join("src"));
        let paths: Vec<String> = ["src", "gone", "lib/**/*.ts", "a?.ts", "[ab].ts", "{x,y}"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(
            present_paths(&dir, &paths),
            ["src", "lib/**/*.ts", "a?.ts", "[ab].ts", "{x,y}"]
        );
        assert!(present_paths(&dir, &[]).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn any_configuration_flag_counts_as_given() {
        assert!(!config_given(&ConfigArgs::default()));
        let each = [
            ConfigArgs {
                config: Some(String::new()),
                ..ConfigArgs::default()
            },
            ConfigArgs {
                validate: Some("x".into()),
                ..ConfigArgs::default()
            },
            ConfigArgs {
                no_config: true,
                ..ConfigArgs::default()
            },
            ConfigArgs {
                config_format: Some("native".into()),
                ..ConfigArgs::default()
            },
            ConfigArgs {
                config_via_node: true,
                ..ConfigArgs::default()
            },
            ConfigArgs {
                strict_compat: true,
                ..ConfigArgs::default()
            },
            ConfigArgs {
                require_comment_token: true,
                ..ConfigArgs::default()
            },
        ];
        for args in each {
            assert!(config_given(&args), "{args:?}");
        }
    }

    #[test]
    fn a_revision_that_looks_like_an_option_is_refused_before_git_sees_it() {
        let dir = std::env::temp_dir();
        for reference in ["", "-h", "--output=/tmp/x"] {
            let e = resolve_revision(&dir, reference);
            assert!(
                e.as_ref()
                    .is_err_and(|m| m.contains("unknown revision") && m.contains("fetch it")),
                "{reference}: {e:?}"
            );
        }
    }

    #[test]
    fn the_base_key_names_the_commit_the_folder_and_the_paths() {
        let config = Config::default();
        let root = Path::new("/repo");
        let a = base_key(root, &config, "abc", "", &[]);
        assert_eq!(a.head, "abc");
        assert_eq!(a.root, "/repo");
        assert_eq!(a.version, key::VERSION);
        assert_eq!(a.inputs, "checkout abc from `` of []");
        let others = [
            base_key(root, &config, "abf", "", &[]),
            base_key(root, &config, "abc", "web/", &[]),
            base_key(root, &config, "abc", "", &["src".into()]),
            base_key(Path::new("/other"), &config, "abc", "", &[]),
        ];
        for other in &others {
            assert_ne!(a.name(), other.name(), "{other:?}");
        }
        assert_eq!(a.name(), base_key(root, &config, "abc", "", &[]).name());
    }

    #[test]
    fn a_side_that_cannot_be_cruised_is_named_with_its_exit_code() {
        let extract = side_failed(
            "the base `main` (abc)",
            &RunError::Extract(rb_model::ExtractError::NoModulesFound),
        );
        assert_eq!(extract.code, 2);
        assert!(
            extract
                .stderr
                .starts_with("rulebearing diff: the base `main` (abc) cannot be cruised: "),
            "{}",
            extract.stderr
        );
        let config = side_failed(
            "the configuration",
            &RunError::Config(rb_config::ConfigError::Invalid("bad".into())),
        );
        assert_eq!(config.code, 3);
    }

    #[test]
    fn a_missing_cache_entry_or_a_foreign_one_is_not_a_graph() {
        let dir = std::env::temp_dir().join(format!("rb-diff-cached-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(cached(&dir).is_none());
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(cache::GRAPH_FILE), "[1]");
        assert!(cached(&dir).is_none());
        let key = base_key(Path::new("/repo"), &Config::default(), "abc", "", &[]);
        let document = GraphDocument {
            modules: vec![rb_model::Module::new("src/a.ts")],
            ..GraphDocument::default()
        };
        assert!(store(&dir, &key, &document).is_ok());
        let sources =
            |d: GraphDocument| -> Vec<String> { d.modules.into_iter().map(|m| m.source).collect() };
        assert_eq!(cached(&dir).map(sources), Some(vec!["src/a.ts".to_owned()]));
        let written = std::fs::read_to_string(dir.join(cache::KEY_FILE)).unwrap_or_default();
        assert!(written.contains("\"head\": \"abc\""), "{written}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
