//! The fixtures the command tests share: the wave 1 tree, the `TestAssembly` graph, and a way to
//! run the binary in a folder.
//!
//! - Plan: [Wave 2, Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

/// The wave 1 fixture's rules: one fence, one ratchet, a decision token.
pub const CONFIG: &str = r#"# The fixture's rules (a hand-written comment the edits keep).
forbidden:
  - name: domain-not-to-web
    severity: error
    comment: "The domain stays independent of the web layer. adr:0010"
    fix: Move the shared type into src/domain
    from: { path: "^src/domain/" }
    to: { path: "^src/web/" }
rules:
  ratchets:
    - name: domain-web-edges
      comment: "plan:wave-1"
      from: { path: "^src/domain/" }
      to: { path: "^src/web/" }
      budget: budgets/domain-web.json
"#;

/// A fresh folder under the system's temporary directory.
pub fn scratch(name: &str) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rb-cli-cmd-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn write(dir: &Path, file: &str, text: &str) -> Result {
    let path = dir.join(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}

pub fn read(dir: &Path, file: &str) -> Result<String> {
    Ok(std::fs::read_to_string(dir.join(file))?)
}

/// The wave 1 fixture: `src/domain/model.ts` imports `src/web/view.ts` (the one violation),
/// `src/main.ts` imports the domain, and the decision record `adr:0010` exists.
pub fn tree(name: &str) -> Result<PathBuf> {
    let dir = scratch(name)?;
    write(
        &dir,
        "src/domain/model.ts",
        "import { w } from \"../web/view\";\nexport const d = w;\n",
    )?;
    write(&dir, "src/web/view.ts", "export const w = 1;\n")?;
    write(
        &dir,
        "src/main.ts",
        "import { d } from \"./domain/model\";\nconsole.log(d);\n",
    )?;
    write(&dir, "rulebearing.yaml", CONFIG)?;
    write(&dir, "budgets/domain-web.json", "{ \"ceiling\": 1 }\n")?;
    write(
        &dir,
        "docs/adr/0010-domain-independent.md",
        "# ADR-0010: The domain is independent\n",
    )?;
    Ok(dir)
}

/// The `TestAssembly` graph conformance gate 2 reads: 48 modules and a code layer.
pub fn test_assembly() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/archunitnet/graphs/TestAssembly.json")
        .to_string_lossy()
        .into_owned()
}

pub fn run(dir: &Path, args: &[&str]) -> Result<Output> {
    Ok(Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .env("SOURCE_DATE_EPOCH", "1790208000")
        .output()?)
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub fn code(output: &Output) -> Option<i32> {
    output.status.code()
}

pub fn json(output: &Output) -> Result<serde_json::Value> {
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub fn clean(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}
