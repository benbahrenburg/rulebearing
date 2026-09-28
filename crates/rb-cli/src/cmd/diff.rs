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
//! folder under the system temporary directory (git hooks off), extracted there from the same
//! subfolder the command runs in, and the worktree is removed afterwards, on an error too. The
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
//! documents. An unreadable input, a file that is not a cruise result, an unknown revision, a
//! folder outside a repository and a base that cannot be cruised exit 2 with the reason; an invalid
//! configuration, an unknown output type or a wrong command line exit 3.

use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Args;
use rb_config::Config;
use rb_model::GraphDocument;
use rb_report::diff::{self, Diff, Side};

use crate::cache::{self, CacheKey, key};
use crate::cli::{ConfigArgs, CruiseArgs};
use crate::context::Context;
use crate::exit::RunExit;
use crate::pipeline::{self, RunError, RunOptions};
use crate::progress::Progress;
use crate::{Outcome, configure, ratchets, write_output};

/// The exit codes of `diff`, printed under its help.
pub const DIFF_EXIT_CODES: &str = "Exit codes:
  0       the diff was written (with --exit-code: no new error-severity violation)
  1-255   with --exit-code: the number of new error-severity violations, capped at 255
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
    let diff = match result {
        Ok(diff) => diff,
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
    let code = if args.exit_code {
        RunExit::Violations(diff.new_errors())
    } else {
        RunExit::Violations(0)
    };
    Outcome {
        stdout,
        stderr: String::new(),
        code: code.code(),
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
fn saved(ctx: &Context<'_>, args: &DiffArgs) -> Result<Diff, Outcome> {
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
    Ok(diff::compute(&read(ctx, old)?, &read(ctx, new)?))
}

/// `git` in `dir`, with hooks off; its trimmed stdout, or its stderr as the error.
fn git(dir: &Path, arguments: &[&str]) -> Result<String, String> {
    let no_hooks = std::env::temp_dir().join("rulebearing-no-git-hooks");
    let output = Command::new("git")
        .arg("-c")
        .arg(format!("core.hooksPath={}", no_hooks.display()))
        .args(arguments)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
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

/// A detached checkout of one commit in a folder under the system temporary directory, removed
/// when dropped.
#[derive(Debug)]
pub struct Checkout {
    /// The repository the worktree belongs to.
    repository: PathBuf,
    /// The worktree's folder.
    pub path: PathBuf,
}

impl Checkout {
    /// Checks `sha` out of the repository at `repository` with `git worktree add --detach`.
    ///
    /// # Errors
    /// Git's message when the worktree cannot be added.
    pub fn add(repository: &Path, sha: &str) -> Result<Self, String> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let path =
            std::env::temp_dir().join(format!("rulebearing-diff-{}-{nanos}", std::process::id()));
        let checkout = Self {
            repository: repository.to_path_buf(),
            path,
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
        if self.path.exists() {
            let _ = std::fs::remove_dir_all(&self.path);
        }
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
    let code = match error {
        RunError::Config(_) | RunError::Engine(rb_rules::EngineError::Element(_)) => {
            RunExit::InvalidConfig
        }
        RunError::Extract(_) | RunError::Engine(_) | RunError::Report(_) => RunExit::Untrustworthy,
    };
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
    let ratchets = ratchets::evaluate(ctx, config, &run.evaluation.document, false);
    let mut document = run.document;
    if !ratchets.results.is_empty() {
        document.summary.ratchets = Some(ratchets.results);
    }
    Ok(document)
}

/// The base graph, extracted: from the cache, else from a checkout (then cached).
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
    };
    let base = args.base.as_deref().unwrap_or(sha);
    let document = pipeline::extract(&base_ctx, config, &args.inputs)
        .map_err(|e| side_failed(&format!("the base `{base}` ({sha})"), &RunError::Extract(e)))?;
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
fn against_base(ctx: &mut Context<'_>, args: &DiffArgs, reference: &str) -> Result<Diff, Outcome> {
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
    Ok(diff::compute(&base, &head).with_sides(
        Side {
            revision: Some(reference.to_owned()),
            sha: Some(sha),
        },
        Side {
            revision: None,
            sha: (!head_sha.is_empty()).then_some(head_sha),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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
