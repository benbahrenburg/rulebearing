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

/// How many graph-changing saves the latency test takes.
const SAVES: usize = 3;

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

/// Waits until the guard has seen everything written so far: a scan that started after this
/// call found nothing more to read. A change that adds a folder and a file in it can take two
/// full reads, and a save made before the second would be part of it, not checked alone.
fn settled(dir: &Path) -> Result {
    let asked = now_ms();
    until(dir, Duration::from_secs(30), |f| {
        f["seenUpTo"].as_u64().is_some_and(|seen| seen >= asked)
    })?;
    Ok(())
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

/// Saves that remove the violating import and then change which service the page uses: each
/// changes the graph and is checked again alone. The fastest of them is within 100 ms. One
/// save taken beside the rest of the suite is not a timing (ADR-0059); the p95 is the `bench`
/// workflow's (testbeds/synth/guard.sh, ADR-0060), and this proves the check can meet it.
/// Returns the contents the page was left with.
fn checked_within_100_ms(dir: &Path) -> Result<String> {
    let clean = |service: usize| {
        format!(
            "import {{ s{service} }} from \"../services/s{service}\";\nexport const page = s{service};\n"
        )
    };
    let mut latencies = Vec::new();
    for service in 0..SAVES {
        let saved_ms = now_ms();
        std::fs::write(dir.join("src/ui/page.ts"), clean(service))?;
        let after = until(dir, Duration::from_secs(10), |f| {
            f["rechecked"] == serde_json::json!(["src/ui/page.ts"])
                && f["latencyMs"].as_u64().is_some_and(|ms| {
                    // The answer for this save, not one carried from the last: the newest file
                    // it read was written at or after this save.
                    f["writtenAt"]
                        .as_u64()
                        .is_some_and(|at| at.saturating_sub(ms) >= saved_ms)
                })
        })?;
        assert_eq!(after["answer"], "", "the fix clears the finding: {after}");
        latencies.push(after["latencyMs"].as_u64().unwrap_or(u64::MAX));
        std::thread::sleep(Duration::from_millis(50));
    }
    let fastest = latencies.iter().copied().min().unwrap_or(u64::MAX);
    assert!(
        fastest < 100,
        "from save to findings written, the fastest of {SAVES} saves: {latencies:?} ms"
    );
    Ok(clean(SAVES - 1))
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
    let page = checked_within_100_ms(&dir)?;
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
    std::fs::write(dir.join("src/ui/page.ts"), &page)?;
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
    // The graph it answered for is written beside the findings for a server to pick up, and
    // follows the saves: the new file is in it.
    let graph_file = dir.join(".graph/guard/graph.json");
    let start = Instant::now();
    loop {
        let text = std::fs::read_to_string(&graph_file).unwrap_or_default();
        if text.contains("\"src/ui/other.ts\"") {
            let graph: serde_json::Value = serde_json::from_str(&text)?;
            assert!(graph["modules"].as_array().is_some_and(|m| !m.is_empty()));
            break;
        }
        if start.elapsed() > Duration::from_secs(10) {
            return Err(format!("{} never held the new file", graph_file.display()).into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
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
    assert!(!graph_file.exists(), "the graph goes with the findings");
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

/// Stops a daemon by closing its standard input, as the agent's session does.
fn stop(mut daemon: Daemon) -> Result {
    drop(daemon.0.stdin.take());
    let start = Instant::now();
    while daemon.0.try_wait()?.is_none() {
        if start.elapsed() > Duration::from_secs(10) {
            return Err("guard did not stop when standard input closed".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

/// The incremental checks start from the earlier walk's files, so whatever could change what the
/// walk finds is read in full: a file in a folder that held no source, or a new folder. Each
/// answer is the one a cold hook gives for the same tree.
#[test]
fn what_the_walk_would_find_anew_is_read_in_full_and_answers_as_a_cold_run() -> Result {
    let dir = tree("walk", 20)?;
    // A folder the walk lists that holds no source, so no source's folder is it.
    std::fs::create_dir_all(dir.join("src/ui/assets"))?;
    std::fs::write(dir.join("src/ui/assets/logo.svg"), "<svg/>\n")?;
    let daemon = watch(&dir)?;
    until(&dir, Duration::from_secs(60), |f| {
        f["answer"]
            .as_str()
            .is_some_and(|a| a.contains("ui-not-to-db"))
    })?;
    // A save that adds an import: checked again alone, from the earlier walk.
    settled(&dir)?;
    std::fs::write(
        dir.join("src/services/s3.ts"),
        "import { s2 } from \"./s2\";\nimport { store } from \"../db/store\";\nexport const s3 = s2 + store;\n",
    )?;
    until(&dir, Duration::from_secs(10), |f| {
        f["rechecked"] == serde_json::json!(["src/services/s3.ts"])
    })?;
    // A source added to the folder that held none.
    std::fs::write(
        dir.join("src/ui/assets/icon.ts"),
        "import { store } from \"../../db/store\";\nexport const icon = store;\n",
    )?;
    until(&dir, Duration::from_secs(60), |f| {
        f["rechecked"] == serde_json::json!([])
            && f["answer"]
                .as_str()
                .is_some_and(|a| a.contains("src/ui/assets/icon.ts"))
    })?;
    // A new folder with a source in it.
    std::fs::create_dir_all(dir.join("src/ui/nested"))?;
    std::fs::write(
        dir.join("src/ui/nested/deep.ts"),
        "import { store } from \"../../db/store\";\nexport const deep = store;\n",
    )?;
    until(&dir, Duration::from_secs(60), |f| {
        f["rechecked"] == serde_json::json!([])
            && f["answer"]
                .as_str()
                .is_some_and(|a| a.contains("src/ui/nested/deep.ts"))
    })?;
    // Then a save, checked alone again, whose answer is a cold run's.
    settled(&dir)?;
    std::fs::write(
        dir.join("src/ui/page.ts"),
        "import { s3 } from \"../services/s3\";\nexport const page = s3;\n",
    )?;
    let last = until(&dir, Duration::from_secs(10), |f| {
        f["rechecked"] == serde_json::json!(["src/ui/page.ts"])
    })?;
    stop(daemon)?;
    let cold = hook(&dir, &[])?;
    assert_eq!(
        last["answer"].as_str().map(str::as_bytes),
        Some(cold.stdout.as_slice())
    );
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// An incremental check that fails has used up the earlier extraction, so the next change reads
/// everything again, and the answer is a cold run's.
#[cfg(unix)]
#[test]
fn after_a_failed_check_the_next_change_reads_everything() -> Result {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tree("failed", 5)?;
    let daemon = watch(&dir)?;
    until(&dir, Duration::from_secs(60), |f| {
        f["answer"]
            .as_str()
            .is_some_and(|a| a.contains("ui-not-to-db"))
    })?;
    // A save the guard cannot read.
    let page = dir.join("src/ui/page.ts");
    std::fs::write(&page, "export const page = 0;\n")?;
    std::fs::set_permissions(&page, std::fs::Permissions::from_mode(0o000))?;
    let failed = until(&dir, Duration::from_secs(10), |f| f["error"].is_string());
    std::fs::set_permissions(&page, std::fs::Permissions::from_mode(0o644))?;
    failed?;
    std::fs::write(
        dir.join("src/services/s1.ts"),
        "import { s0 } from \"./s0\";\nexport const s1 = s0 + 1;\n",
    )?;
    // The failed check used up the earlier extraction, so this change is answered by reading
    // everything. The guard may check the save once more straight after (a file saved within
    // moments of a read's start is not taken as seen), so what is waited for is an answer from
    // a scan that started after the save, not the full read's own findings.
    let saved = now_ms();
    let again = until(&dir, Duration::from_secs(60), |f| {
        f["error"].is_null() && f["seenUpTo"].as_u64().is_some_and(|seen| seen >= saved)
    })?;
    stop(daemon)?;
    let cold = hook(&dir, &[])?;
    assert_eq!(
        again["answer"].as_str().map(str::as_bytes),
        Some(cold.stdout.as_slice())
    );
    assert_eq!(again["answer"], "", "the unreadable save was read: {again}");
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
    // A save made while the first read is still settling is part of a full read, not checked
    // alone.
    settled(&dir)?;
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
