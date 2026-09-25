//! `config expand` output, byte-compared against a committed fixture: a native file that
//! composes presets for one language, substitutes a define and uses both shorthands.
//!
//! - Plan: [Wave 2, Step 9](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#29-step-9-presets---init-presets-vue-svelte-markdown-webpackconfig-collapse-highlight-experimentalstats-2d)
//!   ("`config expand` output fixture")
//! - Source: [design § Shorthands](../../../docs/artifacts/design.md#shorthands) ("`rulebearing
//!   config expand` prints the expansion, so nothing is hidden")
//! - Requirement: [FR-CFG-05](../../../docs/prd.md#fr-cfg-05)
//!
//! The input is `tests/config-expand/rulebearing.yaml`, the expected output
//! `tests/config-expand/expanded.yaml` (YAML) and `expanded.json` (`--json`). Regenerate them
//! deliberately with `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-cli --test config_expand`.

use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

#[test]
fn config_expand_matches_the_fixture() -> Result<(), Box<dyn Error>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/config-expand");
    for (flags, file) in [
        (&[][..], "expanded.yaml"),
        (&["--json"][..], "expanded.json"),
    ] {
        let output = Command::new(BIN)
            .args(["config", "expand", "rulebearing.yaml"])
            .args(flags)
            .current_dir(&dir)
            .output()?;
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual = String::from_utf8(output.stdout)?;
        let again = Command::new(BIN)
            .args(["config", "expand", "rulebearing.yaml"])
            .args(flags)
            .current_dir(&dir)
            .output()?;
        assert_eq!(actual.as_bytes(), again.stdout, "two runs print alike");
        let path = dir.join(file);
        if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
            std::fs::write(&path, &actual)?;
            continue;
        }
        let expected = std::fs::read_to_string(&path)?;
        assert_eq!(
            actual.replace("\r\n", "\n"),
            expected.replace("\r\n", "\n"),
            "{file} changed; if that was intended, regenerate with RB_UPDATE_SNAPSHOTS=1"
        );
    }
    Ok(())
}
