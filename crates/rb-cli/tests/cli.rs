//! End-to-end checks on the command-line surface.
//!
//! - Contract: [ADR-0008](../../../docs/adr/0008-exit-code-contract.md) (exit codes),
//!   [ADR-0024](../../../docs/adr/0024-test-quality-gates.md) (why this is a snapshot)
//! - Requirements: [FR-CORE-06](../../../docs/prd.md#fr-core-06), [FR-CLI-08](../../../docs/prd.md#fr-cli-08)
//!
//! The help text is a promise: `rulebearing` claims flag parity with dependency-cruiser, so a
//! flag that disappears or is renamed must show up in review rather than in a user's pipeline.
//! The snapshots live in `tests/help.txt` (the top level), `tests/help-cruise.txt` and
//! `tests/help-init.txt`; regenerate them deliberately with `RB_UPDATE_SNAPSHOTS=1 cargo test -p
//! rb-cli`.

use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

fn snapshot_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(file)
}

/// The help texts snapshotted: the top level, and `cruise` and `init`, whose flags are
/// dependency-cruiser's `depcruise` flags and `--init`'s presets.
const HELP: &[(&[&str], &str)] = &[
    (&["--help"], "help.txt"),
    (&["cruise", "--help"], "help-cruise.txt"),
    (&["init", "--help"], "help-init.txt"),
];

#[test]
fn help_matches_the_snapshot() -> Result<(), Box<dyn Error>> {
    for (args, file) in HELP {
        let output = Command::new(BIN).args(*args).output()?;
        assert!(
            output.status.success(),
            "`{}` should exit 0",
            args.join(" ")
        );
        let actual = String::from_utf8(output.stdout)?;

        if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
            std::fs::write(snapshot_path(file), &actual)?;
            continue;
        }

        let expected = std::fs::read_to_string(snapshot_path(file))?;
        assert_eq!(
            actual.replace("\r\n", "\n"),
            expected.replace("\r\n", "\n"),
            "the help text of `{}` changed; if that was intended, regenerate with RB_UPDATE_SNAPSHOTS=1",
            args.join(" ")
        );
    }
    Ok(())
}

#[test]
fn version_names_the_tool_and_the_crate_version() -> Result<(), Box<dyn Error>> {
    let output = Command::new(BIN).arg("--version").output()?;
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout)?;
    assert_eq!(
        text.trim(),
        format!("rulebearing {}", env!("CARGO_PKG_VERSION"))
    );
    Ok(())
}

#[test]
fn exit_codes_follow_the_contract() -> Result<(), Box<dyn Error>> {
    // An empty cruise cannot be trusted: 2, never a silent 0 (ADR-0008).
    let empty = std::env::temp_dir().join(format!("rb-cli-empty-{}", std::process::id()));
    std::fs::create_dir_all(&empty)?;
    let cruise = Command::new(BIN)
        .args(["cruise", "--no-config", "."])
        .current_dir(&empty)
        .output()?;
    assert_eq!(cruise.status.code(), Some(2), "an empty cruise exits 2");
    assert!(String::from_utf8(cruise.stderr)?.contains("no modules found"));
    let _ = std::fs::remove_dir_all(&empty);

    // A subcommand a later wave delivers says so and exits 2.
    let later = Command::new(BIN).arg("serve").output()?;
    assert_eq!(later.status.code(), Some(2));
    assert!(String::from_utf8(later.stderr)?.contains("wave 3"));

    // An unknown subcommand is a configuration error.
    let unknown = Command::new(BIN).arg("frobnicate").output()?;
    assert_eq!(
        unknown.status.code(),
        Some(3),
        "an unknown subcommand should exit 3"
    );

    // No arguments prints the usage and succeeds.
    let bare = Command::new(BIN).output()?;
    assert_eq!(bare.status.code(), Some(0));
    Ok(())
}
