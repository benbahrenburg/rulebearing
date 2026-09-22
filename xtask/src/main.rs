//! The one lint entry point, and the documentation link check on its own.
//!
//! - Decision: [ADR-0023](../../docs/adr/0023-documentation-link-and-lint-gates.md)
//! - Working agreement: [CLAUDE.md](../../CLAUDE.md)
//! - Requirements: [NFR-DOC-01](../../docs/prd.md#nfr-doc-01), [NFR-QUAL-02](../../docs/prd.md#nfr-qual-02)
//!
//! ```text
//! cargo xtask check-links      documentation links only (also runs on every compile)
//! cargo xtask lint             every linter for every language in the repository
//! cargo xtask lint --fix       the same, applying what each linter can fix
//! cargo xtask ci               lint, then the workspace test suite
//! ```
//!
//! A language whose tree does not exist yet reports "not applicable" rather than passing, so
//! the first TypeScript, C# or Python file to land brings its linter with it. A linter that is
//! not installed reports "skipped", or fails the run under `--strict`, which is what continuous
//! integration passes.

use std::path::Path;
use std::process::{Command, ExitCode};

use xtask::{any_file, doclinks, repo_root};

/// What one lint step did.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    /// The linter ran and was happy.
    Passed,
    /// The linter ran and reported findings, or could not run.
    Failed(String),
    /// The language has no files in this repository yet.
    NotApplicable(&'static str),
    /// The linter is not installed and `--strict` was not given.
    Skipped(String),
}

/// One lint step: a language, a name and its outcome.
#[derive(Debug, Clone)]
struct Step {
    language: &'static str,
    name: &'static str,
    outcome: Outcome,
}

impl Step {
    fn symbol(&self) -> &'static str {
        match self.outcome {
            Outcome::Passed => "pass",
            Outcome::Failed(_) => "FAIL",
            Outcome::NotApplicable(_) => "n/a ",
            Outcome::Skipped(_) => "skip",
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flags: Vec<&str> = args.iter().map(String::as_str).collect();
    let fix = flags.contains(&"--fix");
    let strict = flags.contains(&"--strict");
    let root = repo_root();

    match flags.first().copied() {
        Some("check-links") => check_links(&root),
        Some("lint") => lint(&root, fix, strict),
        Some("ci") => {
            let lint_code = lint(&root, false, strict);
            if lint_code != ExitCode::SUCCESS {
                return lint_code;
            }
            run_step(
                &root,
                "rust",
                "cargo test",
                "cargo",
                &["test", "--workspace", "--all-features"],
            )
        }
        Some("--help" | "-h") | None => {
            println!("{}", usage());
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("xtask: unknown command `{other}`\n\n{}", usage());
            ExitCode::FAILURE
        }
    }
}

fn usage() -> String {
    [
        "xtask: repository automation for Rulebearing",
        "",
        "Usage: cargo xtask <command> [--fix] [--strict]",
        "",
        "Commands:",
        "  check-links   every relative link in docs and doc comments resolves",
        "  lint          check-links plus every configured linter, all languages",
        "  ci            lint, then cargo test --workspace",
        "",
        "Flags:",
        "  --fix         apply what each linter can fix (lint only)",
        "  --strict      a linter that is not installed fails instead of being skipped",
        "",
        "See CLAUDE.md and docs/adr/0023-documentation-link-and-lint-gates.md.",
    ]
    .join("\n")
}

/// Runs the documentation link check and prints the report.
fn check_links(root: &Path) -> ExitCode {
    match doclinks::check(root) {
        Ok(report) if report.is_clean() => {
            println!(
                "doc links: {} links in {} files resolve",
                report.checked, report.files
            );
            ExitCode::SUCCESS
        }
        Ok(report) => {
            for broken in &report.broken {
                eprintln!("{broken}");
            }
            eprintln!(
                "doc links: {} of {} links do not resolve (see docs/adr/0001-record-architecture-decisions.md)",
                report.broken.len(),
                report.checked
            );
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("doc links: could not run: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs every linter that applies to this repository.
fn lint(root: &Path, fix: bool, strict: bool) -> ExitCode {
    let mut steps = vec![docs_step(root)];
    steps.extend(rust_steps(root, fix, strict));
    steps.extend(typescript_steps(root, fix, strict));
    steps.extend(python_steps(root, fix, strict));
    steps.extend(csharp_steps(root, fix, strict));
    verdict(report(&steps))
}

/// Documentation links. Always applicable: the docs tree is what the code is written against.
fn docs_step(root: &Path) -> Step {
    let outcome = match doclinks::check(root) {
        Ok(report) if report.is_clean() => Outcome::Passed,
        Ok(report) => Outcome::Failed(
            report
                .broken
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        Err(error) => Outcome::Failed(error.to_string()),
    };
    Step {
        language: "docs",
        name: "doc links",
        outcome,
    }
}

/// Rust: `rustfmt` and `clippy` with the workspace lints. Always applicable.
fn rust_steps(root: &Path, fix: bool, strict: bool) -> Vec<Step> {
    const FMT_CHECK: &[&str] = &["fmt", "--all", "--", "--check"];
    const FMT_FIX: &[&str] = &["fmt", "--all"];
    const CLIPPY_CHECK: &[&str] = &[
        "clippy",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--",
        "-D",
        "warnings",
    ];
    const CLIPPY_FIX: &[&str] = &[
        "clippy",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--fix",
        "--allow-dirty",
        "--allow-staged",
    ];
    vec![
        cmd_step(
            root,
            "rust",
            "rustfmt",
            "cargo",
            if fix { FMT_FIX } else { FMT_CHECK },
            strict,
        ),
        cmd_step(
            root,
            "rust",
            "clippy",
            "cargo",
            if fix { CLIPPY_FIX } else { CLIPPY_CHECK },
            strict,
        ),
    ]
}

/// TypeScript and JavaScript: the npm wrapper, the vitest reporter, the eslint plugin.
fn typescript_steps(root: &Path, fix: bool, strict: bool) -> Vec<Step> {
    const DIRS: &[&str] = &["wrappers", "adapters", "frontends"];
    const EXTS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "mjs", "cjs"];
    const ESLINT_CHECK: &[&str] = &["--no-install", "eslint", "."];
    const ESLINT_FIX: &[&str] = &["--no-install", "eslint", ".", "--fix"];
    const PRETTIER_CHECK: &[&str] = &["--no-install", "prettier", "--check", "."];
    const PRETTIER_FIX: &[&str] = &["--no-install", "prettier", "--write", "."];
    const WHY: &str = "no TypeScript or JavaScript files yet";

    if !any_file(root, DIRS, EXTS).unwrap_or(false) {
        return vec![
            not_applicable("typescript", "eslint", WHY),
            not_applicable("typescript", "prettier", WHY),
        ];
    }
    vec![
        cmd_step(
            root,
            "typescript",
            "eslint",
            "npx",
            if fix { ESLINT_FIX } else { ESLINT_CHECK },
            strict,
        ),
        cmd_step(
            root,
            "typescript",
            "prettier",
            "npx",
            if fix { PRETTIER_FIX } else { PRETTIER_CHECK },
            strict,
        ),
    ]
}

/// Python: the pip wrapper and the pytest plugin.
fn python_steps(root: &Path, fix: bool, strict: bool) -> Vec<Step> {
    const DIRS: &[&str] = &["wrappers", "adapters"];
    const EXTS: &[&str] = &["py", "pyi"];
    const CHECK: &[&str] = &["check", "."];
    const CHECK_FIX: &[&str] = &["check", ".", "--fix"];
    const FORMAT_CHECK: &[&str] = &["format", "--check", "."];
    const FORMAT_FIX: &[&str] = &["format", "."];
    const WHY: &str = "no Python files yet";

    if !any_file(root, DIRS, EXTS).unwrap_or(false) {
        return vec![
            not_applicable("python", "ruff check", WHY),
            not_applicable("python", "ruff format", WHY),
            not_applicable("python", "mypy", WHY),
        ];
    }
    vec![
        cmd_step(
            root,
            "python",
            "ruff check",
            "ruff",
            if fix { CHECK_FIX } else { CHECK },
            strict,
        ),
        cmd_step(
            root,
            "python",
            "ruff format",
            "ruff",
            if fix { FORMAT_FIX } else { FORMAT_CHECK },
            strict,
        ),
        cmd_step(root, "python", "mypy", "mypy", &["."], strict),
    ]
}

/// C#: the test adapter, the Roslyn analyzer, the fallback extractor, conformance fixtures.
fn csharp_steps(root: &Path, fix: bool, strict: bool) -> Vec<Step> {
    const DIRS: &[&str] = &["adapters", "frontends", "conformance"];
    const EXTS: &[&str] = &["cs", "csproj", "sln", "slnx"];
    const CHECK: &[&str] = &["format", "--verify-no-changes", "--severity", "warn"];
    const FIX: &[&str] = &["format", "--severity", "warn"];

    if !any_file(root, DIRS, EXTS).unwrap_or(false) {
        return vec![not_applicable(
            "csharp",
            "dotnet format",
            "no C# projects yet",
        )];
    }
    vec![cmd_step(
        root,
        "csharp",
        "dotnet format",
        "dotnet",
        if fix { FIX } else { CHECK },
        strict,
    )]
}

fn not_applicable(language: &'static str, name: &'static str, why: &'static str) -> Step {
    Step {
        language,
        name,
        outcome: Outcome::NotApplicable(why),
    }
}

/// Runs one external linter, reporting a missing tool separately from a failing one.
fn cmd_step(
    root: &Path,
    language: &'static str,
    name: &'static str,
    program: &str,
    args: &[&str],
    strict: bool,
) -> Step {
    println!("  running {language}: {name}");
    let outcome = match Command::new(program).args(args).current_dir(root).status() {
        Ok(status) if status.success() => Outcome::Passed,
        Ok(status) => Outcome::Failed(format!("`{program}` exited with {status}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let message = format!("`{program}` is not installed");
            if strict {
                Outcome::Failed(message)
            } else {
                Outcome::Skipped(message)
            }
        }
        Err(error) => Outcome::Failed(format!("`{program}` could not start: {error}")),
    };
    Step {
        language,
        name,
        outcome,
    }
}

/// Runs one command as a single named step, for `ci`.
fn run_step(
    root: &Path,
    language: &'static str,
    name: &'static str,
    program: &str,
    args: &[&str],
) -> ExitCode {
    verdict(report(&[cmd_step(
        root, language, name, program, args, true,
    )]))
}

/// Prints the summary table and says whether every step passed.
fn report(steps: &[Step]) -> bool {
    println!();
    for step in steps {
        let detail = match &step.outcome {
            Outcome::Passed => String::new(),
            Outcome::Failed(why) => format!("  {}", why.lines().next().unwrap_or(why)),
            Outcome::NotApplicable(why) => format!("  {why}"),
            Outcome::Skipped(why) => format!("  {why}"),
        };
        println!(
            "{} {:<11} {:<14}{}",
            step.symbol(),
            step.language,
            step.name,
            detail
        );
    }
    let failures: Vec<&Step> = steps
        .iter()
        .filter(|s| matches!(s.outcome, Outcome::Failed(_)))
        .collect();
    if failures.is_empty() {
        println!("\nlint: clean");
        return true;
    }
    println!();
    for step in &failures {
        if let Outcome::Failed(why) = &step.outcome {
            eprintln!("{} / {} failed:\n{why}", step.language, step.name);
        }
    }
    false
}

/// Maps a verdict to a process exit code.
fn verdict(clean: bool) -> ExitCode {
    if clean {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn empty_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("rb-xtask-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let docs = root.join("docs");
        fs::create_dir_all(&docs).ok();
        fs::write(docs.join("a.md"), "# Head\n\n[self](a.md#head)\n").ok();
        root
    }

    #[test]
    fn usage_lists_every_command_and_flag() {
        let text = usage();
        for token in ["check-links", "lint", "ci", "--fix", "--strict"] {
            assert!(text.contains(token), "usage does not mention {token}");
        }
    }

    #[test]
    fn symbols_distinguish_the_four_outcomes() {
        let step = |outcome| Step {
            language: "x",
            name: "y",
            outcome,
        };
        assert_eq!(step(Outcome::Passed).symbol(), "pass");
        assert_eq!(step(Outcome::Failed("why".into())).symbol(), "FAIL");
        assert_eq!(step(Outcome::NotApplicable("why")).symbol(), "n/a ");
        assert_eq!(step(Outcome::Skipped("why".into())).symbol(), "skip");
    }

    #[test]
    fn only_a_failure_makes_the_run_dirty() {
        let step = |outcome| Step {
            language: "x",
            name: "y",
            outcome,
        };
        assert!(report(&[
            step(Outcome::Passed),
            step(Outcome::NotApplicable("none yet"))
        ]));
        assert!(report(&[step(Outcome::Skipped("not installed".into()))]));
        assert!(!report(&[
            step(Outcome::Passed),
            step(Outcome::Failed("broke".into()))
        ]));
        assert!(report(&[]));
    }

    #[test]
    fn a_missing_linter_is_skipped_but_fails_under_strict() {
        let root = empty_root("missing-tool");
        let lenient = cmd_step(
            &root,
            "x",
            "ghost",
            "rb-no-such-linter",
            &["--version"],
            false,
        );
        assert!(matches!(lenient.outcome, Outcome::Skipped(_)));
        let strict = cmd_step(
            &root,
            "x",
            "ghost",
            "rb-no-such-linter",
            &["--version"],
            true,
        );
        assert!(matches!(strict.outcome, Outcome::Failed(_)));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_language_with_no_files_is_not_applicable_and_runs_nothing() {
        let root = empty_root("no-languages");
        for step in typescript_steps(&root, false, true)
            .into_iter()
            .chain(python_steps(&root, false, true))
            .chain(csharp_steps(&root, false, true))
        {
            assert!(
                matches!(step.outcome, Outcome::NotApplicable(_)),
                "{}/{} should be not applicable in an empty tree",
                step.language,
                step.name
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_language_steps_appear_once_the_files_do() {
        let root = empty_root("with-languages");
        fs::create_dir_all(root.join("wrappers/npm/src")).ok();
        fs::write(
            root.join("wrappers/npm/src/index.ts"),
            "export const x = 1;\n",
        )
        .ok();
        fs::create_dir_all(root.join("adapters/python")).ok();
        fs::write(root.join("adapters/python/probe.py"), "x = 1\n").ok();
        fs::create_dir_all(root.join("adapters/dotnet")).ok();
        fs::write(root.join("adapters/dotnet/P.csproj"), "<Project />\n").ok();

        // `--strict` with linters that are certainly absent from a scratch directory would run
        // the real tools, so this only asserts the step is no longer "not applicable".
        let steps: Vec<Step> = typescript_steps(&root, false, false)
            .into_iter()
            .chain(python_steps(&root, false, false))
            .chain(csharp_steps(&root, false, false))
            .collect();
        assert_eq!(steps.len(), 6);
        for step in &steps {
            assert!(
                !matches!(step.outcome, Outcome::NotApplicable(_)),
                "{}/{} should apply once its files exist",
                step.language,
                step.name
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_docs_step_passes_when_links_resolve_and_fails_when_they_do_not() {
        let root = empty_root("docs-step");
        assert!(matches!(docs_step(&root).outcome, Outcome::Passed));
        fs::write(
            root.join("docs").join("a.md"),
            "# Head\n\n[gone](missing.md)\n",
        )
        .ok();
        let message = match docs_step(&root).outcome {
            Outcome::Failed(why) => why,
            other => format!("{other:?}"),
        };
        assert!(
            message.contains("missing.md"),
            "expected a failure naming the missing file, got: {message}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_step_that_runs_reports_pass_or_fail() {
        let root = empty_root("real-command");
        let ok = cmd_step(&root, "shell", "true", "true", &[], true);
        assert_eq!(ok.outcome, Outcome::Passed);
        let bad = cmd_step(&root, "shell", "false", "false", &[], true);
        assert!(matches!(bad.outcome, Outcome::Failed(_)));
        let _ = fs::remove_dir_all(&root);
    }
}
