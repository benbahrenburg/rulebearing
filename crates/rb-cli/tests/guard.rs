//! `guard` and `guard --watch` through the binary: the findings file, the Stop hook serving it
//! only when it is fresh and answers for the same command line and configuration, a saved file
//! checked again within 100 ms, a structural change read in full, a `.cs` file in source mode,
//! and a clean stop when standard input closes, with nothing written outside `.graph/`.
//!
//! - Plan: [Wave 3, Step 16](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)
//!   ("an integration test that saves a file and asserts the findings file within 100 ms ...;
//!   clean exit on stdin close; no writes outside `.graph/`"), [§ 1.6](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#16-decisions-applied-and-decisions-this-wave-must-make)
//!   (younger than 5 s and the same configuration hash)
//! - Requirement: [FR-CLI-05](../../../docs/prd.md#fr-cli-05), [NFR-PERF-03](../../../docs/prd.md#nfr-perf-03)

use std::collections::BTreeMap;
use std::error::Error;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

const FINDINGS: &str = ".graph/guard/findings.json";

const CONFIG: &str = "rules:
  dependencies:
    forbidden:
      - name: ui-not-to-db
        comment: \"plan:rulebearing-wave-3\"
        fix: \"Go through the service.\"
        severity: error
        from: { path: \"^src/ui/\" }
        to: { path: \"^src/db/\" }
";

/// A monorepo-shaped tree: `modules` services under `src/services/`, one UI file that breaks the
/// rule, and the database module.
fn tree(name: &str, modules: usize) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rb-cli-guard-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/services"))?;
    std::fs::create_dir_all(dir.join("src/ui"))?;
    std::fs::create_dir_all(dir.join("src/db"))?;
    std::fs::write(dir.join("rulebearing.yaml"), CONFIG)?;
    std::fs::write(
        dir.join("package.json"),
        "{ \"name\": \"guard-fixture\" }\n",
    )?;
    std::fs::write(dir.join("src/db/store.ts"), "export const store = 1;\n")?;
    for i in 0..modules {
        let previous = if i == 0 {
            "import { store } from \"../db/store\";\n".to_owned()
        } else {
            format!("import {{ s{} }} from \"./s{}\";\n", i - 1, i - 1)
        };
        std::fs::write(
            dir.join(format!("src/services/s{i}.ts")),
            format!("{previous}export const s{i} = {i};\n"),
        )?;
    }
    std::fs::write(
        dir.join("src/ui/page.ts"),
        "import { store } from \"../db/store\";\nimport { s0 } from \"../services/s0\";\nexport const page = store + s0;\n",
    )?;
    Ok(dir.canonicalize()?)
}

fn command(dir: &Path) -> Command {
    let mut command = Command::new(BIN);
    command
        .current_dir(dir)
        .env("SOURCE_DATE_EPOCH", "1790000000")
        .env_remove("VIRTUAL_ENV");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    command
}

fn run(dir: &Path, args: &[&str]) -> Result<Output> {
    Ok(command(dir).args(args).stdin(Stdio::null()).output()?)
}

/// The Stop hook, as Claude Code runs it.
fn hook(dir: &Path, extra: &[&str]) -> Result<Output> {
    let mut child = command(dir)
        .args(["cruise", "--output-type", "agent", "--from-hook"])
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(b"{\"stop_hook_active\": false}")?;
    }
    Ok(child.wait_with_output()?)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn findings(dir: &Path) -> Result<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(
        dir.join(FINDINGS),
    )?)?)
}

/// Waits for the findings to satisfy `test`, up to `limit`.
fn until(dir: &Path, limit: Duration, test: impl Fn(&Value) -> bool) -> Result<Value> {
    let start = Instant::now();
    loop {
        if let Ok(found) = findings(dir)
            && test(&found)
        {
            return Ok(found);
        }
        if start.elapsed() > limit {
            return Err(format!(
                "findings never satisfied the test within {limit:?}: {:?}",
                findings(dir).ok()
            )
            .into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Every file under `dir` outside `.graph/`, with its content.
fn snapshot(dir: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in std::fs::read_dir(&folder)? {
            let entry = entry?;
            let path = entry.path();
            if path.strip_prefix(dir)?.starts_with(".graph") {
                continue;
            }
            if entry.file_type()?.is_dir() {
                pending.push(path);
            } else {
                files.insert(path.strip_prefix(dir)?.to_path_buf(), std::fs::read(&path)?);
            }
        }
    }
    Ok(files)
}

#[test]
fn the_hook_serves_a_fresh_answer_for_the_same_command_and_configuration() -> Result {
    let dir = tree("once", 3)?;
    // The hook's own answer, with no guard running.
    let cold = hook(&dir, &[])?;
    assert_eq!(cold.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&cold.stdout).contains("\"decision\":\"block\""));
    let once = run(&dir, &["guard"])?;
    assert_eq!(
        once.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&once.stderr)
    );
    let found = findings(&dir)?;
    assert_eq!(
        found["answer"].as_str().map(str::as_bytes),
        Some(cold.stdout.as_slice())
    );
    assert_eq!(found["mode"], "source");
    assert_eq!(found["rechecked"], serde_json::json!([]));
    // Findings no running guard confirms are not served: the hook asks, waits, and cruises.
    let waited = Instant::now();
    assert_eq!(hook(&dir, &[])?.stdout, cold.stdout);
    assert!(
        waited.elapsed() >= Duration::from_secs(4),
        "the hook waited for a confirmation"
    );
    // Served: the answer is the file's, byte for byte, so mark it to see that it was. A
    // `seenUpTo` ahead of the clock stands for a guard that has confirmed.
    let mut marked = found.clone();
    marked["answer"] = Value::from("served\n");
    marked["writtenAt"] = Value::from(now_ms());
    marked["seenUpTo"] = Value::from(now_ms() + 600_000);
    std::fs::write(dir.join(FINDINGS), serde_json::to_string(&marked)?)?;
    assert_eq!(hook(&dir, &[])?.stdout, b"served\n");
    // Another command line is not answered by these findings.
    assert_ne!(
        hook(&dir, &["--affected-depth", "1", "--max-findings", "1"])?.stdout,
        b"served\n"
    );
    // Nor stale findings.
    let mut stale = marked.clone();
    stale["writtenAt"] = Value::from(now_ms().saturating_sub(6_000));
    std::fs::write(dir.join(FINDINGS), serde_json::to_string(&stale)?)?;
    assert_eq!(hook(&dir, &[])?.stdout, cold.stdout);
    // Nor findings for another configuration.
    let mut fresh = marked.clone();
    fresh["writtenAt"] = Value::from(now_ms());
    std::fs::write(dir.join(FINDINGS), serde_json::to_string(&fresh)?)?;
    std::fs::write(
        dir.join("rulebearing.yaml"),
        CONFIG.replace("Go through", "Always go through"),
    )?;
    assert_ne!(hook(&dir, &[])?.stdout, b"served\n");
    // Nor another build's.
    let mut other = fresh;
    other["tool"] = Value::from("rulebearing 0.0.0");
    std::fs::write(dir.join(FINDINGS), serde_json::to_string(&other)?)?;
    assert_ne!(hook(&dir, &[])?.stdout, b"served\n");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn watch(dir: &Path) -> Result<Daemon> {
    let child = command(dir)
        .args(["guard", "--watch", "--interval", "5"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    Ok(Daemon(child))
}

#[test]
fn a_saved_file_is_checked_again_within_100_ms_and_stdin_closing_stops_it() -> Result {
    let dir = tree("watch", 200)?;
    let before = snapshot(&dir)?;
    let mut daemon = watch(&dir)?;
    let first = until(&dir, Duration::from_secs(60), |f| {
        f["answer"]
            .as_str()
            .is_some_and(|a| a.contains("ui-not-to-db"))
    })?;
    // The heartbeat keeps an unchanged answer fresh.
    let beat = until(&dir, Duration::from_secs(10), |f| {
        f["writtenAt"].as_u64() > first["writtenAt"].as_u64()
    })?;
    assert_eq!(beat["answer"], first["answer"]);
    // A save that removes the violating import: checked again alone, within 100 ms.
    let page = "import { s0 } from \"../services/s0\";\nexport const page = s0;\n";
    std::fs::write(dir.join("src/ui/page.ts"), page)?;
    let saved = Instant::now();
    let after = until(&dir, Duration::from_secs(10), |f| {
        f["rechecked"] == serde_json::json!(["src/ui/page.ts"])
    })?;
    let observed = saved.elapsed();
    assert_eq!(after["answer"], "", "the fix clears the finding: {after}");
    let latency = after["latencyMs"].as_u64().unwrap_or(u64::MAX);
    assert!(
        latency < 100,
        "from save to findings written: {latency} ms (observed {observed:?})"
    );
    // The hook serves it, once the guard confirms it has seen every change.
    let asked = Instant::now();
    assert_eq!(hook(&dir, &[])?.stdout, b"");
    assert!(
        asked.elapsed() < Duration::from_secs(4),
        "served, not cruised after the wait"
    );
    // A save the hook runs right after is in the answer, never a stale one: the violation is
    // back the moment the import is.
    std::fs::write(
        dir.join("src/ui/page.ts"),
        "import { store } from \"../db/store\";\nexport const page = store;\n",
    )?;
    let answer = hook(&dir, &[])?;
    assert!(
        String::from_utf8_lossy(&answer.stdout).contains("ui-not-to-db"),
        "a stale answer was served"
    );
    std::fs::write(dir.join("src/ui/page.ts"), page)?;
    until(&dir, Duration::from_secs(10), |f| f["answer"] == "")?;
    // A new file is structural: everything is read again, and its violation found.
    std::fs::write(
        dir.join("src/ui/other.ts"),
        "import { store } from \"../db/store\";\nexport const other = store;\n",
    )?;
    until(&dir, Duration::from_secs(60), |f| {
        f["rechecked"] == serde_json::json!([])
            && f["answer"]
                .as_str()
                .is_some_and(|a| a.contains("src/ui/other.ts"))
    })?;
    // Standard input closing stops it cleanly, and it takes its findings with it.
    drop(daemon.0.stdin.take());
    let start = Instant::now();
    let status = loop {
        if let Some(status) = daemon.0.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(10) {
            return Err("guard did not stop when standard input closed".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success());
    let mut log = String::new();
    if let Some(mut stderr) = daemon.0.stderr.take() {
        std::io::Read::read_to_string(&mut stderr, &mut log)?;
    }
    assert!(
        log.contains("guard: checked src/ui/page.ts again in"),
        "{log}"
    );
    assert!(log.contains("stopped"), "{log}");
    assert!(!dir.join(FINDINGS).exists());
    // Nothing was written outside `.graph/` but the two edits the test made.
    let mut expected = before;
    expected.insert(PathBuf::from("src/ui/page.ts"), page.as_bytes().to_vec());
    expected.insert(
        PathBuf::from("src/ui/other.ts"),
        b"import { store } from \"../db/store\";\nexport const other = store;\n".to_vec(),
    );
    assert_eq!(snapshot(&dir)?, expected);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

fn copy(from: &Path, to: &Path) -> Result {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[test]
fn a_saved_cs_file_is_read_again_in_source_mode() -> Result {
    let dir = std::env::temp_dir().join(format!("rb-cli-guard-dotnet-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../rb-extract-dotnet/tests/fixtures/source"),
        &dir,
    )?;
    std::fs::write(
        dir.join("rulebearing.yaml"),
        "rules:\n  dependencies:\n    forbidden:\n      - name: orders-not-to-customers\n        comment: \"plan:rulebearing-wave-3\"\n        severity: error\n        from: { path: \"^Domain/Orders/\" }\n        to: { path: \"^Domain/Customers/\" }\n",
    )?;
    let dir = dir.canonicalize()?;
    let daemon = watch(&dir)?;
    let first = until(&dir, Duration::from_secs(60), |f| {
        f["answer"]
            .as_str()
            .is_some_and(|a| a.contains("orders-not-to-customers"))
    })?;
    assert_eq!(first["mode"], "source");
    assert!(
        first["answer"]
            .as_str()
            .is_some_and(|a| a.contains("approximate: "))
    );
    std::fs::write(
        dir.join("Domain/Orders/Order.cs"),
        "namespace Shop.Domain.Orders;\n\npublic partial class Order\n{\n    public Order(int customer)\n    {\n    }\n}\n",
    )?;
    let after = until(&dir, Duration::from_secs(10), |f| {
        f["rechecked"] == serde_json::json!(["Domain/Orders/Order.cs"])
    })?;
    assert_eq!(after["answer"], "", "{after}");
    drop(daemon);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn watch_needs_the_binary_and_a_named_configuration() -> Result {
    let dir = tree("refusals", 1)?;
    let piped = run(&dir, &["guard", "--watch", "--config", "-"])?;
    assert_eq!(
        piped.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&piped.stderr)
    );
    assert!(String::from_utf8_lossy(&piped.stderr).contains("--config -"));
    let library = rb_cli::run(&["guard".to_owned(), "--watch".to_owned()]);
    assert_eq!(library.code, 3);
    assert!(library.stderr.contains("standard input"));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
