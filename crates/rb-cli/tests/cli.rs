//! End-to-end checks on the command-line surface.
//!
//! - Contract: [ADR-0008](../../../docs/adr/0008-exit-code-contract.md) (exit codes),
//!   [ADR-0024](../../../docs/adr/0024-test-quality-gates.md) (why this is a snapshot)
//! - Requirements: [FR-CORE-06](../../../docs/prd.md#fr-core-06), [FR-CLI-08](../../../docs/prd.md#fr-cli-08)
//!
//! The help text is a promise: `rulebearing` claims flag parity with dependency-cruiser, so a
//! flag that disappears or is renamed must show up in review rather than in a user's pipeline.
//! The snapshot lives in `tests/help.txt`; regenerate it deliberately with
//! `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-cli`.

use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("help.txt")
}

#[test]
fn help_matches_the_snapshot() -> Result<(), Box<dyn Error>> {
    let output = Command::new(BIN).arg("--help").output()?;
    assert!(output.status.success(), "`--help` should exit 0");
    let actual = String::from_utf8(output.stdout)?;

    if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(snapshot_path(), &actual)?;
        return Ok(());
    }

    let expected = std::fs::read_to_string(snapshot_path())?;
    assert_eq!(
        actual.replace("\r\n", "\n"),
        expected.replace("\r\n", "\n"),
        "the help text changed; if that was intended, regenerate with RB_UPDATE_SNAPSHOTS=1"
    );
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
    // A subcommand that exists but is not implemented: the run cannot be trusted, so 2 rather
    // than 0. A pipeline must never read a stub as a passing gate.
    let known = Command::new(BIN).arg("cruise").output()?;
    assert_eq!(
        known.status.code(),
        Some(2),
        "a known subcommand should exit 2 until implemented"
    );
    assert!(String::from_utf8(known.stderr)?.contains("not implemented"));

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
