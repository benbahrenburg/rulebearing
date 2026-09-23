//! The first run, end to end: `init` on a fresh repository and `adopt` on one already on
//! dependency-cruiser with three violations. Both must leave a gate that exits 0.
//!
//! - Plan: [Wave 1, Step 16](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-16-init-1f),
//!   [Step 17](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-17-adopt-1f)
//!   (a fixture repository with three violations; the PR body snapshot)
//! - Source: [design § The developer relations hat](../../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo)
//! - Requirement: [FR-CLI-03](../../../docs/prd.md#fr-cli-03)
//!
//! The pull request body is a committed snapshot, `tests/adopt-pr-body.md`; regenerate it
//! deliberately with `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-cli --test first_run`.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

fn tree(name: &str, files: &[(&str, &str)]) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-first-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (file, text) in files {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    Ok(dir)
}

/// A command that cannot reach the repository the tests run in. Under a git hook (`pre-push`)
/// git exports `GIT_DIR`, `GIT_INDEX_FILE` and others, and a `git` started with them acts on the
/// host repository instead of the temporary one.
fn isolated(program: &str) -> Command {
    let mut command = Command::new(program);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(isolated(BIN)
        .args(args)
        .current_dir(dir)
        .env("SOURCE_DATE_EPOCH", "1790000000")
        .output()?)
}

fn git(dir: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(isolated("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Tester")
        .env("GIT_AUTHOR_EMAIL", "tester@example.com")
        .env("GIT_COMMITTER_NAME", "Tester")
        .env("GIT_COMMITTER_EMAIL", "tester@example.com")
        .output()?)
}

const MONOREPO: &[(&str, &str)] = &[
    ("tsconfig.json", "{ \"compilerOptions\": {} }\n"),
    (
        "package.json",
        "{ \"name\": \"root\", \"private\": true }\n",
    ),
    (
        "apps/web/src/features/cart/index.ts",
        "export const cart = 1;\n",
    ),
    (
        "apps/web/src/features/search/index.ts",
        "import { cart } from \"../cart\";\nexport const search = cart;\n",
    ),
    (
        "apps/web/src/main.ts",
        "import { search } from \"./features/search\";\nimport { button } from \"../../../packages/ui/src/button\";\nexport default [search, button];\n",
    ),
    (
        "apps/admin/src/main.ts",
        "import { search } from \"../../web/src/features/search\";\nexport default search;\n",
    ),
    ("packages/ui/src/button.ts", "export const button = 1;\n"),
    ("vitest.config.ts", "export default {};\n"),
];

#[test]
fn init_writes_a_configuration_that_passes() -> Result<(), Box<dyn Error>> {
    let dir = tree("init", MONOREPO)?;
    let dry = run(&dir, &["init", "--dry-run", "--owner", "@me"])?;
    assert_eq!(
        dry.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let proposal = String::from_utf8(dry.stdout)?;
    assert!(
        !dir.join("rulebearing.yaml").exists(),
        "--dry-run writes nothing"
    );
    for rule in [
        "apps-are-independent",
        "packages-not-to-apps",
        "features-are-independent-in-apps-web",
    ] {
        assert!(
            proposal.contains(&format!("- name: {rule}")),
            "{rule} missing:\n{proposal}"
        );
    }
    assert!(
        proposal.contains("\"owner\":\"@me\"") && proposal.contains("\"expires\":\"2026-12-20\"")
    );

    let written = run(&dir, &["init", "--owner", "@me"])?;
    assert_eq!(
        written.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("rulebearing.yaml"))?,
        proposal
    );
    let gate = run(&dir, &["cruise", "apps", "packages"])?;
    assert_eq!(gate.status.code(), Some(0), "the first run is green");

    let again = run(&dir, &["init"])?;
    assert_eq!(
        again.status.code(),
        Some(3),
        "an existing configuration is kept"
    );
    assert!(String::from_utf8_lossy(&again.stderr).contains("rulebearing adopt"));
    assert_eq!(
        run(&dir, &["init", "--force", "--owner", "@me"])?
            .status
            .code(),
        Some(0)
    );
    // A file named by --output is not overwritten either.
    std::fs::write(dir.join("custom.yaml"), "mine\n")?;
    std::fs::remove_file(dir.join("rulebearing.yaml"))?;
    let custom = run(&dir, &["init", "--output", "custom.yaml"])?;
    assert_eq!(custom.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&custom.stderr).contains("custom.yaml already exists"));
    assert_eq!(std::fs::read_to_string(dir.join("custom.yaml"))?, "mine\n");

    let empty = tree("init-empty", &[("README.md", "nothing here\n")])?;
    assert_eq!(run(&empty, &["init"])?.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&empty);
    Ok(())
}

const DEPENDENCY_CRUISER: &str = r#"module.exports = {
  forbidden: [
    { name: "a-not-to-b", severity: "error", comment: "a stays apart from b", from: { path: "^src/a/" }, to: { path: "^src/b/" } },
    { name: "no-circular", severity: "error", from: {}, to: { circular: true } },
  ],
};
"#;

/// Three violations: two edges from a to b, and the a-b cycle.
const BROWNFIELD: &[(&str, &str)] = &[
    ("package.json", "{ \"name\": \"brownfield\" }\n"),
    (".dependency-cruiser.cjs", DEPENDENCY_CRUISER),
    (
        "src/a/a.js",
        "import { b } from \"../b/b\";\nexport const a = b;\n",
    ),
    (
        "src/a/a2.js",
        "import { b } from \"../b/b\";\nexport const a2 = b;\n",
    ),
    (
        "src/b/b.js",
        "import { a } from \"../a/a\";\nexport const b = 1;\n",
    ),
];

fn snapshot() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/adopt-pr-body.md")
}

#[test]
fn adopt_baselines_and_goes_green() -> Result<(), Box<dyn Error>> {
    let dir = tree("adopt", BROWNFIELD)?;
    let before = run(&dir, &["cruise", "src"])?;
    assert_eq!(
        before.status.code(),
        Some(3),
        "three errors before adopting"
    );

    let adopted = run(
        &dir,
        &["adopt", "--no-pr", "--owner", "@team", "--ci", "azure"],
    )?;
    assert_eq!(
        adopted.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&adopted.stderr)
    );
    assert!(String::from_utf8(adopted.stdout)?.starts_with("baselined 3 findings"));
    let config = std::fs::read_to_string(dir.join("rulebearing.yaml"))?;
    assert!(config.contains("extends: \"./.dependency-cruiser.cjs\""));
    assert_eq!(config.matches("\"owner\":\"@team\"").count(), 3);
    assert!(
        config.contains("\"cycle\""),
        "a cycle entry carries its cycle"
    );
    assert!(dir.join("azure-pipelines.rulebearing.yml").is_file());
    assert!(dir.join(".githooks/pre-commit").is_file());
    let page = std::fs::read_to_string(dir.join("docs/architecture/rulebearing.md"))?;
    assert!(page.contains("| `a-not-to-b` | Files matching `src/a/` may not import files matching `src/b/`. | a stays apart from b |  | 2 |"), "{page}");

    let gate = run(&dir, &["cruise", "--config", "rulebearing.yaml", "src"])?;
    assert_eq!(gate.status.code(), Some(0), "green on day one");
    let result = run(
        &dir,
        &[
            "cruise",
            "--config",
            "rulebearing.yaml",
            "-T",
            "json",
            "src",
        ],
    )?;
    let value: Value = serde_json::from_slice(&result.stdout)?;
    assert_eq!(value["summary"]["ignore"], 3);

    // rulebearing.yaml now exists and is found first; it is native, so adopt refuses.
    let native = run(&dir, &["adopt", "--no-pr"])?;
    assert_eq!(
        native.status.code(),
        Some(3),
        "rulebearing.yaml is found first and is native"
    );
    let none = tree("adopt-none", &[("package.json", "{}\n")])?;
    let missing = run(&none, &["adopt", "--no-pr"])?;
    assert_eq!(missing.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("rulebearing init"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&none);
    Ok(())
}

#[test]
fn adopt_commits_on_a_branch_without_a_remote() -> Result<(), Box<dyn Error>> {
    let dir = tree("adopt-git", BROWNFIELD)?;
    git(&dir, &["init", "-q"])?;
    git(&dir, &["config", "user.name", "Tester"])?;
    git(&dir, &["config", "user.email", "tester@example.com"])?;
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-qm", "start"])?;
    // Work the user had staged stays staged, out of the adopt commit.
    std::fs::write(dir.join("notes.txt"), "mine\n")?;
    git(&dir, &["add", "notes.txt"])?;
    let adopted = run(&dir, &["adopt"])?;
    let stdout = String::from_utf8(adopted.stdout)?;
    assert_eq!(
        adopted.status.code(),
        Some(0),
        "{stdout}{}",
        String::from_utf8_lossy(&adopted.stderr)
    );
    assert!(
        stdout.contains(
            "committed on rulebearing/adopt; push it and open a pull request (no remote found)"
        ),
        "{stdout}"
    );
    let branch = git(&dir, &["branch", "--show-current"])?;
    assert_eq!(
        String::from_utf8(branch.stdout)?.trim(),
        "rulebearing/adopt"
    );
    let files = git(&dir, &["show", "--name-only", "--format=", "HEAD"])?;
    assert_eq!(
        String::from_utf8(files.stdout)?.lines().collect::<Vec<_>>(),
        [
            ".githooks/pre-commit",
            ".github/workflows/rulebearing.yml",
            "docs/architecture/rulebearing.md",
            "rulebearing.yaml"
        ]
    );
    let staged = git(&dir, &["diff", "--cached", "--name-only"])?;
    assert_eq!(String::from_utf8(staged.stdout)?.trim(), "notes.txt");
    let adopt_head = git(&dir, &["rev-parse", "HEAD"])?.stdout;
    git(&dir, &["checkout", "-q", "-"])?;
    let again = run(&dir, &["adopt"])?;
    assert!(
        String::from_utf8(again.stdout)?.contains("does it exist already?"),
        "an earlier adopt branch is never reset: {}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert_eq!(
        git(&dir, &["rev-parse", "rulebearing/adopt"])?.stdout,
        adopt_head
    );
    let config = std::fs::read_to_string(dir.join("rulebearing.yaml"))?;
    assert!(
        config.contains("\"owner\":\"Tester\""),
        "the owner defaults to the git user"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn the_pull_request_body_matches_the_snapshot() -> Result<(), Box<dyn Error>> {
    let entries: Vec<Value> = serde_json::from_str(
        r#"[
          { "id": "RB-1", "rule": { "name": "a-not-to-b", "severity": "error" } },
          { "id": "RB-2", "rule": { "name": "a-not-to-b", "severity": "error" } },
          { "id": "RB-3", "rule": { "name": "no-circular", "severity": "error" } }
        ]"#,
    )?;
    let files = [
        "rulebearing.yaml",
        ".githooks/pre-commit",
        ".github/workflows/rulebearing.yml",
        "docs/architecture/rulebearing.md",
    ]
    .map(str::to_owned);
    let body = rb_cli::cmd::adopt::pr_body("./.dependency-cruiser.cjs", &entries, &files, "0.1.0");
    if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(snapshot(), &body)?;
        return Ok(());
    }
    let expected = std::fs::read_to_string(snapshot())?;
    assert_eq!(
        body,
        expected.replace("\r\n", "\n"),
        "the PR body changed; if that was intended, regenerate with RB_UPDATE_SNAPSHOTS=1"
    );
    let clean = rb_cli::cmd::adopt::pr_body("./x.json", &[], &files, "0.1.0");
    assert!(clean.contains("no findings, so there is no baseline"));
    Ok(())
}
