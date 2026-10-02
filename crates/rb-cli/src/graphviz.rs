//! GraphViz' `dot`, started for `--output-type x-dot-webpage` only, as dependency-cruiser starts
//! it; and the runner the conformance protocol builds from the `spawnFunction` option upstream's
//! specs pass.
//!
//! - Decision: [ADR-0053](../../../docs/adr/0053-x-dot-webpage-draws-with-graphviz-dot.md)
//! - Architecture: [`docs/architecture.md#security-posture`](../../../docs/architecture.md#security-posture)
//! - Plan: [Wave 3, Step 6](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01)

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::Arc;

use rb_report::dot_webpage::{Graphviz, GraphvizRunner, Spawned};
use serde_json::Value;

/// The `dot` on `PATH`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemDot;

impl Graphviz for SystemDot {
    fn run(&self, args: &[&str], input: Option<&str>) -> Spawned {
        spawn_sync("dot", args, input)
    }
}

/// The `code` Node's `spawnSync` puts in its error for `error` (`ENOENT`, `EPIPE`, ...), so the
/// message is the one upstream's reporter throws: `spawnSync dot <code>`.
fn node_code(error: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match error.kind() {
        ErrorKind::NotFound => "ENOENT".into(),
        ErrorKind::PermissionDenied => "EACCES".into(),
        ErrorKind::BrokenPipe => "EPIPE".into(),
        _ => error.to_string(),
    }
}

/// Node's `spawnSync(program, args, { input })`: the status, both streams, and the error that
/// kept the program from running or from reading all of its input.
fn spawn_sync(program: &str, args: &[&str], input: Option<&str>) -> Spawned {
    let spawn_error = |e: &std::io::Error| format!("spawnSync {program} {}", node_code(e));
    let child = Command::new(program)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => {
            return Spawned {
                error: Some(spawn_error(&e)),
                ..Spawned::default()
            };
        }
    };
    // The program is written from a thread so a large SVG on stdout cannot block it.
    let writer = match (child.stdin.take(), input) {
        (Some(mut stdin), Some(text)) => {
            let text = text.to_owned();
            Some(std::thread::spawn(move || stdin.write_all(text.as_bytes())))
        }
        _ => None,
    };
    let output = child.wait_with_output();
    let written = writer.map(std::thread::JoinHandle::join);
    match output {
        Ok(output) => Spawned {
            status: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            error: match written {
                Some(Ok(Err(e))) => Some(spawn_error(&e)),
                _ => None,
            },
        },
        Err(e) => Spawned {
            error: Some(spawn_error(&e)),
            ..Spawned::default()
        },
    }
}

/// The runner `cruise` and `fmt` give the reporters.
pub fn system() -> GraphvizRunner {
    GraphvizRunner(Arc::new(SystemDot))
}

/// Upstream's `spawnFunction` option in the conformance protocol's JSON: what the spec's function
/// answered for `dot -V` (`version`) and for `dot -Tsvg` (`convert`), each
/// `{ status, stdout, stderr, error }`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answers {
    version: Spawned,
    convert: Spawned,
}

fn spawned(value: Option<&Value>) -> Spawned {
    let text = |key: &str| {
        value
            .and_then(|v| v.get(key))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    Spawned {
        status: value
            .and_then(|v| v.get("status"))
            .and_then(Value::as_i64)
            .and_then(|s| i32::try_from(s).ok()),
        stdout: text("stdout").unwrap_or_default(),
        stderr: text("stderr").unwrap_or_default(),
        error: text("error"),
    }
}

impl Answers {
    /// The answers in `options.spawnFunction`, when the request carries them.
    pub fn from_options(options: Option<&Value>) -> Option<Self> {
        let answers = options?.get("spawnFunction")?.as_object()?;
        Some(Self {
            version: spawned(answers.get("version")),
            convert: spawned(answers.get("convert")),
        })
    }
}

impl Graphviz for Answers {
    fn run(&self, args: &[&str], _input: Option<&str>) -> Spawned {
        if args.first() == Some(&"-V") {
            self.version.clone()
        } else {
            self.convert.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn answers_come_from_the_spawn_function_option() {
        let options = json!({ "spawnFunction": {
            "version": { "status": 0, "stderr": "dot - graphviz version 1.2.3.4", "stdout": null, "error": null },
            "convert": { "status": 42, "stdout": "foo bar", "stderr": "baz", "error": "boom" } } });
        let answers = Answers::from_options(Some(&options)).unwrap_or_default();
        assert_eq!(
            answers.run(&["-V"], None),
            Spawned {
                status: Some(0),
                stdout: String::new(),
                stderr: "dot - graphviz version 1.2.3.4".into(),
                error: None
            }
        );
        let convert = answers.run(&["-Tsvg"], Some("digraph {}"));
        assert_eq!(convert.status, Some(42));
        assert_eq!(convert.error.as_deref(), Some("boom"));
        assert_eq!(convert.stdout, "foo bar");
        assert!(Answers::from_options(Some(&json!({}))).is_none());
        assert!(Answers::from_options(None).is_none());
        assert_eq!(spawned(None), Spawned::default());
    }

    #[test]
    fn the_system_dot_runs_or_says_why_not() {
        let version = SystemDot.run(&["-V"], None);
        match version.status {
            // GraphViz is installed: it names itself on stderr and draws a program.
            Some(0) => {
                assert!(
                    version.stderr.starts_with("dot - graphviz version"),
                    "{version:?}"
                );
                let svg = SystemDot.run(&["-Tsvg"], Some("digraph { a -> b }"));
                assert_eq!(svg.status, Some(0), "{svg:?}");
                assert!(svg.stdout.contains("<svg"), "{svg:?}");
                let bad = SystemDot.run(&["-Tsvg"], Some("this is not dot"));
                assert_ne!(bad.status, Some(0));
            }
            // It is not: the spawn error is named.
            _ => assert!(
                version.error.is_some() || version.status.is_some(),
                "{version:?}"
            ),
        }
        let runner = system();
        assert_eq!(runner.clone(), runner);
    }

    /// Errors carry the code Node's `spawnSync` gives, so the message is upstream's.
    #[cfg(unix)]
    #[test]
    fn spawn_errors_are_named_as_node_names_them() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = std::env::temp_dir().join(format!("rb-cli-fake-dot-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        // A dot that exits at once without reading its input: writing a program larger than a
        // pipe holds meets a closed pipe.
        let early = dir.join("dot");
        std::fs::write(&early, "#!/bin/sh\nexit 3\n")?;
        std::fs::set_permissions(&early, std::fs::Permissions::from_mode(0o755))?;
        let program = "digraph {}\n".repeat(200_000);
        let spawned = spawn_sync(&early.to_string_lossy(), &["-Tsvg"], Some(&program));
        assert_eq!(spawned.status, Some(3));
        assert_eq!(
            spawned.error,
            Some(format!("spawnSync {} EPIPE", early.display()))
        );
        let missing = spawn_sync("rulebearing-no-such-program", &["-V"], None);
        assert_eq!(
            missing.error.as_deref(),
            Some("spawnSync rulebearing-no-such-program ENOENT")
        );
        assert_eq!(missing.status, None);
        let denied = dir.join("not-executable");
        std::fs::write(&denied, "")?;
        let refused = spawn_sync(&denied.to_string_lossy(), &[], None);
        assert_eq!(
            refused.error,
            Some(format!("spawnSync {} EACCES", denied.display()))
        );
        assert_eq!(
            node_code(&std::io::Error::other("odd")),
            "odd",
            "any other error as it prints"
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
