//! `-T plugin:<path>`, end to end: a plugin renders a real cruise, its `exitCode` is the count,
//! the receipt names it, `fmt` renders a saved result through it, and every failure (missing,
//! invalid, a sandbox refusal) exits 3 naming the plugin and the fix.
//!
//! - Plan: [Wave 3, Step 7](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar),
//!   the [`plugin:<path>` contract](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#15-interfaces-and-contracts-this-wave-freezes)
//! - Decisions: [ADR-0006](../../../docs/adr/0006-embedded-quickjs-config-evaluator.md),
//!   [ADR-0030](../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md),
//!   [ADR-0008](../../../docs/adr/0008-exit-code-contract.md)
//! - Requirements: [FR-OUT-01](../../../docs/prd.md#fr-out-01), [NFR-SEC-01](../../../docs/prd.md#nfr-sec-01)

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

const CONFIG: &str = r#"forbidden:
  - name: web-not-to-db
    severity: error
    comment: "The web layer reaches the store through a service (adr:0010)"
    fix: Call the store through src/services
    from: { path: "^src/web/" }
    to: { path: "^src/db/" }
"#;

/// A plugin that reports each module with its dependency count, the receipt, and gates on the
/// error count as `err` would.
const SUMMARY: &str = r"const path = require('path');
const { title } = require('./title.json');
module.exports = (result) => ({
  output: [title, ...result.modules.map((m) => path.basename(m.source) + ' ' + m.dependencies.length),
    'plugins=' + JSON.stringify(result.summary.plugins ?? null)].join('\n') + '\n',
  exitCode: result.summary.error,
});
";

fn write(dir: &Path, file: &str, text: &str) -> Result {
    let path = dir.join(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}

fn tree(name: &str) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rb-cli-plugin-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".git"))?;
    write(&dir, "rulebearing.yaml", CONFIG)?;
    write(&dir, "src/db/store.ts", "export const store = 1;\n")?;
    write(
        &dir,
        "src/web/page.ts",
        "import { store } from \"../db/store\";\nexport const page = store;\n",
    )?;
    write(&dir, "reporters/summary.cjs", SUMMARY)?;
    write(
        &dir,
        "reporters/title.json",
        r##"{ "title": "# modules" }"##,
    )?;
    write(
        &dir,
        "node_modules/stats-plugin/package.json",
        r#"{ "name": "stats-plugin", "type": "module", "exports": { ".": { "import": "./main.js" } } }"#,
    )?;
    write(
        &dir,
        "node_modules/stats-plugin/main.js",
        "export default (r) => ({ output: `modules=${r.summary.totalCruised}`, exitCode: 0 });\n",
    )?;
    write(
        &dir,
        "reporters/invalid.cjs",
        "module.exports = () => ({ output: 'x' });\n",
    )?;
    write(
        &dir,
        "reporters/reads-fs.cjs",
        "module.exports = () => ({ output: require('fs').readFileSync('/etc/passwd', 'utf8'), exitCode: 0 });\n",
    )?;
    Ok(dir.canonicalize()?)
}

fn run(dir: &Path, args: &[&str]) -> Result<Output> {
    Ok(Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .output()?)
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn cruise_renders_through_a_plugin_and_exits_with_its_count() -> Result {
    let dir = tree("cruise")?;
    let args = ["cruise", "src", "-T", "plugin:./reporters/summary.cjs"];
    let first = run(&dir, &args)?;
    assert_eq!(first.status.code(), Some(1), "{}", stderr(&first));
    assert_eq!(
        stdout(&first),
        "# modules\nstore.ts 0\npage.ts 1\nplugins=[\"reporters/summary.cjs\"]\n"
    );
    // Deterministic: two runs, byte for byte.
    let second = run(&dir, &args)?;
    assert_eq!(first.stdout, second.stdout);
    // The plugin's count shifts under strict mode like any reporter's.
    let strict = run(
        &dir,
        &[
            "cruise",
            "src",
            "-T",
            "plugin:./reporters/summary.cjs",
            "--exit-code-mode",
            "strict",
        ],
    )?;
    assert_eq!(strict.status.code(), Some(11));
    // --strict-schema hands the plugin dependency-cruiser's shape: no receipt.
    let stripped = run(
        &dir,
        &[
            "cruise",
            "src",
            "-T",
            "plugin:./reporters/summary.cjs",
            "--strict-schema",
        ],
    )?;
    assert!(
        stdout(&stripped).ends_with("plugins=null\n"),
        "{}",
        stdout(&stripped)
    );
    // A package under node_modules, through its exports, an ES module.
    let package = run(&dir, &["cruise", "src", "-T", "plugin:stats-plugin"])?;
    assert_eq!(package.status.code(), Some(0), "{}", stderr(&package));
    assert_eq!(stdout(&package), "modules=2");
    // A file:// URL names the same module.
    let url = format!(
        "plugin:file://{}/reporters/summary.cjs",
        dir.to_string_lossy()
    );
    let by_url = run(&dir, &["cruise", "src", "-T", &url])?;
    assert_eq!(by_url.stdout, first.stdout);
    // --output-to writes the plugin's output to the file.
    let to_file = run(
        &dir,
        &[
            "cruise",
            "src",
            "-T",
            "plugin:stats-plugin",
            "-f",
            "out/stats.txt",
        ],
    )?;
    assert_eq!(to_file.status.code(), Some(0));
    assert_eq!(
        std::fs::read_to_string(dir.join("out/stats.txt"))?,
        "modules=2"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// The cache: the evaluated layer serves a plugin (the verdict does not depend on the reporter),
/// the rendered layer never serves or stores one (the output depends on the plugin's code and
/// what it requires, which the key does not cover).
#[test]
fn a_plugins_output_is_never_cached_and_its_verdict_is() -> Result {
    let dir = tree("cache")?;
    let plugin = |version: &str| {
        format!(
            "module.exports = (r) => ({{ output: '{version} ' + JSON.stringify(r.summary.cache) + ' ' + r.summary.error + '\\n', exitCode: r.summary.error }});\n"
        )
    };
    write(&dir, "reporters/cached.cjs", &plugin("v1"))?;
    let args = [
        "cruise",
        "src",
        "--cache",
        "-T",
        "plugin:./reporters/cached.cjs",
    ];
    let cache = dir.join(".graph/cache");
    let first = run(&dir, &args)?;
    assert_eq!(first.status.code(), Some(1), "{}", stderr(&first));
    assert!(
        stdout(&first).starts_with("v1 {\"hit\":false"),
        "{}",
        stdout(&first)
    );
    let stored = |prefix: &str| -> std::io::Result<bool> {
        Ok(std::fs::read_dir(&cache)?
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with(prefix)))
    };
    assert!(stored("evaluated")?, "the verdict is kept for the next run");
    assert!(!stored("rendered")?, "a plugin's output is not kept");
    // The same run again: the verdict comes from the cache, the plugin runs again.
    let second = run(&dir, &args)?;
    assert_eq!(second.status.code(), Some(1));
    assert!(
        stdout(&second).starts_with("v1 {\"hit\":true"),
        "{}",
        stdout(&second)
    );
    assert!(!stored("rendered")?);
    // A change to the plugin alone (no source, no configuration) shows at once.
    write(&dir, "reporters/cached.cjs", &plugin("v2"))?;
    let third = run(&dir, &args)?;
    assert!(
        stdout(&third).starts_with("v2 {\"hit\":true"),
        "{}",
        stdout(&third)
    );
    assert!(stdout(&third).ends_with(" 1\n"));
    // A built-in reporter over the same cache still uses the rendered layer: its second run
    // renders from the stored verdict and keeps the output.
    for _ in 0..2 {
        let err = run(&dir, &["cruise", "src", "--cache", "-T", "err"])?;
        assert_eq!(err.status.code(), Some(1));
    }
    assert!(stored("rendered")?, "a built-in reporter's output is kept");
    let again = run(&dir, &args)?;
    assert!(stdout(&again).starts_with("v2 "), "{}", stdout(&again));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn fmt_renders_a_saved_result_through_a_plugin() -> Result {
    let dir = tree("fmt")?;
    let saved = run(&dir, &["cruise", "src", "-T", "json", "-f", "cruise.json"])?;
    assert_eq!(saved.status.code(), Some(0), "{}", stderr(&saved));
    let plain = run(
        &dir,
        &["fmt", "-T", "plugin:./reporters/summary.cjs", "cruise.json"],
    )?;
    assert_eq!(
        plain.status.code(),
        Some(0),
        "fmt exits 0 without --exit-code"
    );
    assert_eq!(
        stdout(&plain),
        "# modules\nstore.ts 0\npage.ts 1\nplugins=[\"reporters/summary.cjs\"]\n"
    );
    let gated = run(
        &dir,
        &[
            "fmt",
            "--exit-code",
            "-T",
            "plugin:./reporters/summary.cjs",
            "cruise.json",
        ],
    )?;
    assert_eq!(gated.status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_plugin_that_cannot_run_exits_three_naming_it_and_the_fix() -> Result {
    let dir = tree("errors")?;
    for (plugin, needles) in [
        (
            "plugin:./reporters/missing.cjs",
            &[
                "Could not find reporter plugin './reporters/missing.cjs' (or it isn't valid)",
                "Name a JavaScript module",
            ][..],
        ),
        (
            "plugin:./reporters/invalid.cjs",
            &[
                "./reporters/invalid.cjs is not a valid plugin",
                "no own `exitCode`",
                "returns { output: string, exitCode: number }",
            ][..],
        ),
        (
            "plugin:./reporters/reads-fs.cjs",
            &[
                "the reporter sandbox refused it",
                "`fs` is not available in the reporter sandbox",
                "use a built-in reporter",
            ][..],
        ),
        (
            "plugin:../../../../../../etc/passwd",
            &[
                "the reporter sandbox refused it",
                "is outside the repository",
            ][..],
        ),
    ] {
        for command in [
            vec!["cruise", "src", "-T", plugin],
            vec!["fmt", "-T", plugin, "cruise.json"],
        ] {
            if command[0] == "fmt" {
                let saved = run(&dir, &["cruise", "src", "-T", "json", "-f", "cruise.json"])?;
                assert_eq!(saved.status.code(), Some(0));
            }
            let output = run(&dir, &command)?;
            assert_eq!(
                output.status.code(),
                Some(3),
                "{command:?}: {}",
                stderr(&output)
            );
            assert!(stdout(&output).is_empty(), "{command:?}");
            for needle in needles {
                assert!(
                    stderr(&output).contains(needle),
                    "{command:?}: {}",
                    stderr(&output)
                );
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
