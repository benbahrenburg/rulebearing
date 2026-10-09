//! `serve --lsp` driven by a headless client over the binary's standard input and output, and
//! both servers checked for sockets.
//!
//! - Plan: [Wave 3, Step 20](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#25-steps-for-sub-wave-3e-serve---mcp-and-serve---lsp)
//!   ("fixture transcripts; an editor smoke test in CI using a headless LSP client"),
//!   [§ 1.7](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#17-quality-attributes)
//!   ("`serve` opens no socket")
//! - Requirement: [FR-CLI-06](../../../docs/prd.md#fr-cli-06), [NFR-SEC-01](../../../docs/prd.md#nfr-sec-01)
//!
//! The client opens the violating file of `tests/fixtures/serve/tree`, takes its diagnostic and
//! quick fix, runs the fix's command, removes the import, saves, and sees the diagnostic go; then
//! shuts the server down. Every message the server sent, with the scratch folder written `<root>`,
//! is compared with `tests/fixtures/serve/lsp-expected.jsonl` (`RB_UPDATE_SNAPSHOTS=1` rewrites
//! it).

use std::error::Error;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

/// The variables git sets for a hook, removed so a run from a pre-push hook reaches the scratch
/// tree only.
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

/// The fixture tree in a scratch folder, canonical, its graph saved.
fn tree(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-serve-lsp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(&fixtures().join("tree"), &dir)?;
    let dir = rb_model::without_verbatim(&dir.canonicalize()?);
    let saved = command(
        &dir,
        &["cruise", "src", "-T", "json", "-f", ".graph/cruise.json"],
    )
    .output()?;
    assert_eq!(saved.status.code(), Some(0));
    Ok(dir)
}

fn uri(dir: &Path, file: &str) -> String {
    let path = dir.join(file).to_string_lossy().replace('\\', "/");
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    }
}

/// A headless LSP client over a running server.
struct Client {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    /// Every message the server sent, in order.
    received: Vec<Value>,
}

impl Client {
    fn start(dir: &Path) -> Result<Self, Box<dyn Error>> {
        let mut child = command(dir, &["serve", "--lsp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let input = child.stdin.take().ok_or("no stdin")?;
        let output = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        Ok(Self {
            child,
            input,
            output,
            received: Vec::new(),
        })
    }

    fn send(&mut self, message: &Value) -> Result<(), Box<dyn Error>> {
        let text = message.to_string();
        write!(self.input, "Content-Length: {}\r\n\r\n{text}", text.len())?;
        self.input.flush()?;
        Ok(())
    }

    fn read(&mut self) -> Result<Value, Box<dyn Error>> {
        let mut length = 0;
        loop {
            let mut line = String::new();
            if self.output.read_line(&mut line)? == 0 {
                return Err("the server closed its output".into());
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = value.parse()?;
            }
        }
        let mut body = vec![0; length];
        self.output.read_exact(&mut body)?;
        let message: Value = serde_json::from_slice(&body)?;
        self.received.push(message.clone());
        Ok(message)
    }

    /// Reads until a message for which `wanted` holds.
    fn until(&mut self, wanted: impl Fn(&Value) -> bool) -> Result<Value, Box<dyn Error>> {
        for _ in 0..50 {
            let message = self.read()?;
            if wanted(&message) {
                return Ok(message);
            }
        }
        Err("no such message".into())
    }
}

fn diagnostics_for(uri: &str) -> impl Fn(&Value) -> bool + '_ {
    move |m: &Value| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri
}

/// Compares every message the server sent, the scratch folder written `<root>`, with the
/// recorded transcript; `RB_UPDATE_SNAPSHOTS=1` records it.
fn compare_transcript(dir: &Path, received: &[Value]) -> Result<(), Box<dyn Error>> {
    // The root's URI first (`file:///C:/...` on Windows, `file:///tmp/...` elsewhere), then any
    // bare path, so the recording reads the same on every platform.
    let root_uri = uri(dir, "").trim_end_matches('/').to_owned();
    let root = dir.to_string_lossy().replace('\\', "/");
    let mut transcript = String::new();
    for message in received {
        let text = message
            .to_string()
            .replace(&root_uri, "file://<root>")
            .replace(&root, "<root>");
        transcript.push_str(&text);
        transcript.push('\n');
    }
    let expected_file = fixtures().join("lsp-expected.jsonl");
    if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&expected_file, &transcript)?;
    }
    let expected = std::fs::read_to_string(&expected_file)?;
    for (n, (got, want)) in transcript.lines().zip(expected.lines()).enumerate() {
        assert_eq!(got, want, "message {}", n + 1);
    }
    assert_eq!(transcript.lines().count(), expected.lines().count());
    Ok(())
}

#[test]
fn a_headless_client_sees_the_finding_its_fix_and_its_clearing() -> Result<(), Box<dyn Error>> {
    let dir = tree("client")?;
    let model = uri(&dir, "src/domain/model.ts");
    let view = uri(&dir, "src/web/view.ts");
    let mut client = Client::start(&dir)?;
    client.send(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "processId": null, "rootUri": uri(&dir, ""), "capabilities": {} } }))?;
    let initialized = client.until(|m| m["id"] == 1)?;
    assert_eq!(
        initialized["result"]["capabilities"]["codeActionProvider"]["codeActionKinds"],
        json!(["quickfix"])
    );
    client.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }))?;

    client.send(
        &json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
        "textDocument": { "uri": model, "languageId": "typescript", "version": 1, "text": "" } } }),
    )?;
    let published = client.until(diagnostics_for(&model))?;
    let diagnostics = published["params"]["diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(diagnostics.len(), 1, "{published}");
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic["source"], "rulebearing");
    assert_eq!(
        diagnostic["message"],
        "domain-not-to-web: Move the shared type into src/domain"
    );
    assert!(
        diagnostic["code"]
            .as_str()
            .is_some_and(|c| c.starts_with("RB-"))
    );
    assert_eq!(diagnostic["severity"], 1);
    assert_eq!(
        diagnostic["range"]["start"]["line"], 0,
        "the import is on the first line"
    );

    client.send(
        &json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
        "textDocument": { "uri": view, "languageId": "typescript", "version": 1, "text": "" } } }),
    )?;
    let clean = client.until(diagnostics_for(&view))?;
    assert_eq!(clean["params"]["diagnostics"], json!([]));

    client.send(
        &json!({ "jsonrpc": "2.0", "id": 2, "method": "textDocument/codeAction", "params": {
        "textDocument": { "uri": model }, "range": diagnostic["range"],
        "context": { "diagnostics": [diagnostic] } } }),
    )?;
    let actions = client.until(|m| m["id"] == 2)?;
    assert_eq!(
        actions["result"][0]["title"],
        "Move the shared type into src/domain"
    );
    assert_eq!(actions["result"][0]["kind"], "quickfix");
    let command = actions["result"][0]["command"].clone();

    client.send(
        &json!({ "jsonrpc": "2.0", "id": 3, "method": "workspace/executeCommand",
        "params": { "command": command["command"], "arguments": command["arguments"] } }),
    )?;
    let explained = client.until(|m| m["id"] == 3)?;
    assert!(
        explained["result"]
            .as_str()
            .is_some_and(|t| t.starts_with("domain-not-to-web  (forbidden, error)")),
        "{explained}"
    );

    // The import goes and the file is saved: the re-check clears the diagnostic.
    std::fs::write(dir.join("src/domain/model.ts"), "export const d = 1;\n")?;
    client.send(&json!({ "jsonrpc": "2.0", "method": "textDocument/didSave",
        "params": { "textDocument": { "uri": model } } }))?;
    let cleared = client.until(diagnostics_for(&model))?;
    assert_eq!(cleared["params"]["diagnostics"], json!([]), "{cleared}");

    client.send(
        &json!({ "jsonrpc": "2.0", "id": 4, "method": "textDocument/hover", "params": {} }),
    )?;
    let refused = client.until(|m| m["id"] == 4)?;
    assert_eq!(refused["error"]["code"], -32601);
    client.send(&json!({ "jsonrpc": "2.0", "id": 5, "method": "shutdown" }))?;
    let shut = client.until(|m| m["id"] == 5)?;
    assert!(shut["result"].is_null());
    client.send(&json!({ "jsonrpc": "2.0", "method": "exit" }))?;
    let status = client.child.wait()?;
    assert_eq!(status.code(), Some(0), "shut down before exit");

    compare_transcript(&dir, &client.received)?;
    // Nothing was written outside .graph/ but the edit the client made.
    assert!(!dir.join(".graph/cruise.json.tmp").exists());
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn exit_without_shutdown_is_exit_code_one() -> Result<(), Box<dyn Error>> {
    let dir = tree("exit")?;
    let mut client = Client::start(&dir)?;
    client.send(&json!({ "jsonrpc": "2.0", "method": "exit" }))?;
    assert_eq!(client.child.wait()?.code(), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// The sockets a process holds, by the means the platform offers: `/proc` on Linux, `lsof`
/// elsewhere. `None` when neither can be asked.
#[cfg(unix)]
fn sockets(pid: u32) -> Option<Vec<String>> {
    let fds = PathBuf::from(format!("/proc/{pid}/fd"));
    if fds.is_dir() {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(fds).ok()? {
            let target = std::fs::read_link(entry.ok()?.path()).ok()?;
            let target = target.to_string_lossy().into_owned();
            if target.starts_with("socket:") {
                found.push(target);
            }
        }
        return Some(found);
    }
    // `-a` ANDs every selector, so internet and Unix-domain sockets are asked for separately.
    let mut found = Vec::new();
    for kind in ["-i", "-U"] {
        let listed = Command::new("lsof")
            .args(["-a", "-p", &pid.to_string(), kind, "-F", "t"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&listed.stdout);
        found.extend(
            text.lines()
                .filter(|l| l.starts_with('t'))
                .map(str::to_owned),
        );
    }
    Some(found)
}

#[cfg(unix)]
#[test]
fn neither_server_opens_a_socket() -> Result<(), Box<dyn Error>> {
    let dir = tree("sockets")?;
    for protocol in ["--mcp", "--lsp"] {
        let mut child = command(&dir, &["serve", protocol])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut input = child.stdin.take().ok_or("no stdin")?;
        // A request that makes the server read the graph and run a command first.
        if protocol == "--mcp" {
            input.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"rules\",\"arguments\":{}}}\n")?;
        } else {
            let open = json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
                "textDocument": { "uri": uri(&dir, "src/main.ts") } } })
            .to_string();
            write!(input, "Content-Length: {}\r\n\r\n{open}", open.len())?;
        }
        input.flush()?;
        let mut first = [0_u8; 1];
        let mut output = child.stdout.take().ok_or("no stdout")?;
        output.read_exact(&mut first)?;
        match sockets(child.id()) {
            Some(found) => assert!(found.is_empty(), "serve {protocol} holds {found:?}"),
            None => println!("skipped: neither /proc nor lsof answers here"),
        }
        drop(input);
        let _ = child.wait()?;
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// The check sees a socket when there is one: a process listening on a local port is caught.
#[cfg(unix)]
#[test]
fn the_socket_check_sees_a_listening_socket() -> Result<(), Box<dyn Error>> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    match sockets(std::process::id()) {
        Some(found) => assert!(!found.is_empty(), "the test's own listener is not seen"),
        None => println!("skipped: neither /proc nor lsof answers here"),
    }
    drop(listener);
    Ok(())
}
