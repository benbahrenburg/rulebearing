//! `serve --mcp` over the binary's standard input and output: a fixture transcript, request by
//! request against the expected responses, and every tool's text against the CLI command it
//! stands for.
//!
//! - Plan: [Wave 3, Step 19](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#25-steps-for-sub-wave-3e-serve---mcp-and-serve---lsp)
//!   ("fixture transcripts for every tool, and a test that asserts each tool result equals the
//!   CLI's `--json` on the same fixture")
//! - Decision: [ADR-0021](../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Requirement: [FR-CLI-06](../../../docs/prd.md#fr-cli-06)
//!
//! The fixture is `tests/fixtures/serve/tree`, cruised first into `.graph/cruise.json`, the warm
//! graph. `tests/fixtures/serve/mcp-requests.jsonl` is sent as it is and the responses are
//! compared with `mcp-expected.jsonl`; `RB_UPDATE_SNAPSHOTS=1` rewrites that file, a change to be
//! explained in review.

use std::error::Error;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

/// The variables git sets for a hook (`git rev-parse --local-env-vars`), removed so a run from a
/// pre-push hook reaches the scratch tree only.
const GIT_LOCAL_ENV: [&str; 15] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/serve")
}

fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
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

fn command(dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(BIN);
    command.args(args).current_dir(dir);
    for name in GIT_LOCAL_ENV {
        command.env_remove(name);
    }
    command
}

fn run(dir: &Path, args: &[&str], stdin: &str) -> Result<Output, Box<dyn Error>> {
    let mut child = command(dir, args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut input) = child.stdin.take() {
        input.write_all(stdin.as_bytes())?;
    }
    Ok(child.wait_with_output()?)
}

/// The fixture tree in a scratch folder, its graph saved.
fn tree(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-serve-mcp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(&fixtures().join("tree"), &dir)?;
    let saved = run(
        &dir,
        &["cruise", "src", "-T", "json", "-f", ".graph/cruise.json"],
        "",
    )?;
    assert_eq!(
        saved.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&saved.stderr)
    );
    let graph: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(".graph/cruise.json"))?)?;
    assert_eq!(
        graph["summary"]["violations"].as_array().map(Vec::len),
        Some(1),
        "one violation"
    );
    Ok(dir)
}

#[test]
fn the_transcript_answers_as_recorded() -> Result<(), Box<dyn Error>> {
    let dir = tree("transcript")?;
    let requests = std::fs::read_to_string(fixtures().join("mcp-requests.jsonl"))?;
    let served = run(&dir, &["serve", "--mcp"], &requests)?;
    assert_eq!(
        served.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&served.stderr)
    );
    let responses = String::from_utf8(served.stdout)?;
    let expected_file = fixtures().join("mcp-expected.jsonl");
    if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&expected_file, &responses)?;
    }
    let expected = std::fs::read_to_string(&expected_file)?;
    for (n, (got, want)) in responses.lines().zip(expected.lines()).enumerate() {
        assert_eq!(got, want, "response {}", n + 1);
    }
    assert_eq!(responses.lines().count(), expected.lines().count());
    // One response per request with an id, and one for the line that is not JSON; none for the
    // notification.
    assert_eq!(responses.lines().count(), 15);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn every_tool_answers_what_its_command_prints() -> Result<(), Box<dyn Error>> {
    let dir = tree("equal")?;
    let requests = std::fs::read_to_string(fixtures().join("mcp-requests.jsonl"))?;
    let served = run(&dir, &["serve", "--mcp"], &requests)?;
    let responses: Vec<Value> = String::from_utf8(served.stdout)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    let mut compared = 0;
    for request in requests
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
    {
        if request["method"] != "tools/call" {
            continue;
        }
        let name = request["params"]["name"].as_str().unwrap_or_default();
        let Ok(mut line) = rb_cli::serve::mcp::command_line(
            name,
            &request["params"]["arguments"],
            &rb_cli::cli::ConfigArgs::default(),
        ) else {
            continue;
        };
        // The CLI answers from the same file the server held warm.
        if name != "diff" {
            let at = line.iter().position(|a| a == "--").unwrap_or(line.len());
            line.splice(
                at..at,
                ["--graph".to_owned(), ".graph/cruise.json".to_owned()],
            );
        }
        let args: Vec<&str> = line.iter().map(String::as_str).collect();
        let cli = run(&dir, &args, "")?;
        let response = responses
            .iter()
            .find(|r| r["id"] == request["id"])
            .ok_or("no response")?;
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();
        let failed = cli.status.code().is_none_or(|c| c >= 2);
        assert_eq!(response["result"]["isError"], failed, "{name}: {response}");
        let printed = if failed {
            format!(
                "{}{}",
                String::from_utf8_lossy(&cli.stdout),
                String::from_utf8_lossy(&cli.stderr)
            )
        } else {
            String::from_utf8_lossy(&cli.stdout).into_owned()
        };
        assert_eq!(
            text, printed,
            "{name}: the tool's text and `rulebearing {args:?}`"
        );
        if !failed {
            assert_eq!(
                response["result"]["structuredContent"],
                serde_json::from_str::<Value>(&printed)?,
                "{name}"
            );
        }
        compared += 1;
    }
    assert_eq!(compared, 9, "the eight tools, and one call that fails");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_new_graph_is_read_before_the_next_call() -> Result<(), Box<dyn Error>> {
    let dir = tree("fresh")?;
    let call = |id: u32| {
        format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"tools/call\",\"params\":{{\"name\":\"count\",\"arguments\":{{\"from\":\"^src/\",\"to\":\"^src/\"}}}}}}\n"
        )
    };
    let mut child = command(&dir, &["serve", "--mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut input = child.stdin.take().ok_or("no stdin")?;
    let mut output = std::io::BufReader::new(child.stdout.take().ok_or("no stdout")?);
    let mut read = || -> Result<Value, Box<dyn Error>> {
        let mut line = String::new();
        std::io::BufRead::read_line(&mut output, &mut line)?;
        Ok(serde_json::from_str(&line)?)
    };
    input.write_all(call(1).as_bytes())?;
    input.flush()?;
    let before = read()?;
    assert_eq!(before["result"]["structuredContent"]["count"], 2);
    // The domain stops importing the web layer and the graph is saved again.
    std::fs::write(dir.join("src/domain/model.ts"), "export const d = 1;\n")?;
    let saved = run(
        &dir,
        &["cruise", "src", "-T", "json", "-f", ".graph/cruise.json"],
        "",
    )?;
    assert_eq!(saved.status.code(), Some(0));
    input.write_all(call(2).as_bytes())?;
    input.flush()?;
    let after = read()?;
    assert_eq!(after["result"]["structuredContent"]["count"], 1);
    drop(input);
    assert!(child.wait()?.success());
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn serve_refuses_a_configuration_on_standard_input() -> Result<(), Box<dyn Error>> {
    let dir = tree("stdin-config")?;
    let refused = run(&dir, &["serve", "--mcp", "--config", "-"], "")?;
    assert_eq!(refused.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("carries the protocol"));
    let none = run(&dir, &["serve"], "")?;
    assert_eq!(none.status.code(), Some(3), "a protocol is named");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
