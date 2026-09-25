//! End-to-end checks on the agent surface: `rules`, `explain`, `test`, `can-import`, `count`,
//! `config`, `hooks`, `summary`, `impact` and `attest`, run as a process against a small tree.
//!
//! - Plan: [Wave 1, Step 14](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-14-rules---json-explain-explain---plain-test-can-import-1e),
//!   [Step 15](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-15-hooks-install---claude-code-summary---format-agent-impact-attest---require-comment-token-1e)
//! - Contract: [ADR-0008](../../../docs/adr/0008-exit-code-contract.md) (exit codes)
//! - Requirements: [FR-CLI-01](../../../docs/prd.md#fr-cli-01), [FR-CLI-03](../../../docs/prd.md#fr-cli-03)

use std::error::Error;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");
/// The command `hooks install --claude-code` writes for `Stop`.
const STOP_HOOK: &str = "rulebearing cruise --output-type agent --from-hook";

const CONFIG: &str = r#"forbidden:
  - name: domain-not-to-web
    severity: error
    comment: "The domain stays independent of the web layer (decision: adr:0010)"
    fix: Move the shared type into src/domain
    from: { path: "^src/domain/" }
    to: { path: "^src/web/" }
    examples:
      forbidden: ["src/domain/a.ts -> src/web/b.ts"]
      allowed: ["src/web/b.ts -> src/domain/a.ts"]
rules:
  ratchets:
    - name: domain-web-edges
      from: { path: "^src/domain/" }
      to: { path: "^src/web/" }
      budget: budgets/domain-web.json
"#;

/// A tree with one violation: `src/domain/model.ts` imports `src/web/view.ts`.
fn tree(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-agent-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/domain"))?;
    std::fs::create_dir_all(dir.join("src/web"))?;
    std::fs::write(
        dir.join("src/domain/model.ts"),
        "import { w } from \"../web/view\";\nexport const d = w;\n",
    )?;
    std::fs::write(dir.join("src/web/view.ts"), "export const w = 1;\n")?;
    std::fs::write(
        dir.join("src/main.ts"),
        "import { d } from \"./domain/model\";\nconsole.log(d);\n",
    )?;
    std::fs::write(dir.join("rulebearing.yaml"), CONFIG)?;
    Ok(dir)
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(BIN).args(args).current_dir(dir).output()?)
}

fn run_with_stdin(dir: &Path, args: &[&str], stdin: &str) -> Result<Output, Box<dyn Error>> {
    let mut child = Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut input) = child.stdin.take() {
        input.write_all(stdin.as_bytes())?;
    }
    Ok(child.wait_with_output()?)
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn json(output: &Output) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_slice(&output.stdout)?)
}

#[test]
fn rules_explain_and_test_describe_the_rule_set() -> Result<(), Box<dyn Error>> {
    let dir = tree("rules")?;
    let rules = run(&dir, &["rules", "--json"])?;
    assert_eq!(rules.status.code(), Some(0));
    let listed = json(&rules)?;
    assert_eq!(listed["rules"][0]["name"], "domain-not-to-web");
    assert_eq!(listed["rules"][0]["violations"], 1);
    assert_eq!(listed["rules"][0]["fromMatches"], 1);

    let table = stdout(&run(&dir, &["rules"])?);
    let header = table.lines().next().unwrap_or_default();
    let row = table.lines().nth(1).unwrap_or_default();
    assert_eq!(
        header.len(),
        row.len(),
        "the table columns line up:\n{table}"
    );

    let plain = run(&dir, &["explain", "domain-not-to-web", "--plain"])?;
    assert_eq!(
        stdout(&plain),
        "Files matching `src/domain/` may not import files matching `src/web/`.\n"
    );
    let explained = stdout(&run(&dir, &["explain", "domain-not-to-web"])?);
    assert!(explained.contains("fix: Move the shared type into src/domain"));
    assert!(explained.contains("src/domain/model.ts -> src/web/view.ts"));
    assert!(!explained.contains("null"), "{explained}");
    let unknown = run(&dir, &["explain", "no-such-rule"])?;
    assert_eq!(unknown.status.code(), Some(3));

    let tested = run(&dir, &["test"])?;
    assert_eq!(tested.status.code(), Some(0), "{}", stdout(&tested));
    assert!(stdout(&tested).contains("0 failing"));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn can_import_answers_from_the_saved_graph() -> Result<(), Box<dyn Error>> {
    let dir = tree("can-import")?;
    // With no cache entry yet, the first question extracts and writes one (plan 0002, Step 13).
    let first = run(
        &dir,
        &["can-import", "src/domain/model.ts", "src/web/view.ts"],
    )?;
    assert_eq!(first.status.code(), Some(1), "{}", stdout(&first));
    assert!(dir.join(".graph/cache").is_dir());
    std::fs::create_dir_all(dir.join("budgets"))?;
    std::fs::write(dir.join("budgets/domain-web.json"), "{\"ceiling\":1}\n")?;
    let saved = run(
        &dir,
        &["cruise", "-T", "json", "-f", ".graph/cruise.json", "src"],
    )?;
    assert_eq!(
        saved.status.code(),
        Some(0),
        "json does not gate (ADR-0030)"
    );
    let no = run(
        &dir,
        &["can-import", "src/domain/model.ts", "src/web/view.ts"],
    )?;
    assert_eq!(no.status.code(), Some(1));
    assert!(stdout(&no).contains("fix: Move the shared type into src/domain"));
    let yes = run(
        &dir,
        &["can-import", "src/web/view.ts", "src/domain/model.ts"],
    )?;
    assert_eq!(yes.status.code(), Some(0));
    assert_eq!(stdout(&yes), "yes\n");
    // Paths are read the way the graph writes them.
    let root = dir.canonicalize()?;
    let absolute = root
        .join("src/domain/model.ts")
        .to_string_lossy()
        .into_owned();
    for from in ["./src/domain/model.ts", absolute.as_str()] {
        let spelled = run(&dir, &["can-import", from, "./src/web/view.ts"])?;
        assert_eq!(spelled.status.code(), Some(1), "{from}");
    }
    // A target the graph has never seen and that is not a file here is not a silent yes.
    let unknown = run(
        &dir,
        &[
            "can-import",
            "src/web/view.ts",
            "node_modules/left-pad/index.js",
        ],
    )?;
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("is not in the graph"));
    let saved = run(
        &dir,
        &[
            "can-import",
            "--graph",
            ".graph/cruise.json",
            "src/web/view.ts",
            "node_modules/left-pad/index.js",
        ],
    )?;
    assert!(String::from_utf8_lossy(&saved.stderr).contains("not in .graph/cruise.json"));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn can_import_json_carries_the_gates_violation_id_and_fix() -> Result<(), Box<dyn Error>> {
    // Wave 2, Step 13: the ESLint plugin reads `can-import --json`, so its id must be the one
    // the gate gives the same edge, and its exit code the text form's.
    let dir = tree("can-import-json")?;
    let gate = run(&dir, &["cruise", "-T", "json", "src"])?;
    let gate_id = json(&gate)?["summary"]["violations"][0]["id"].clone();
    let no = run(
        &dir,
        &[
            "can-import",
            "--json",
            "src/domain/model.ts",
            "src/web/view.ts",
        ],
    )?;
    assert_eq!(no.status.code(), Some(1), "{}", stdout(&no));
    let answer = json(&no)?;
    assert_eq!(answer["verdict"], "no");
    assert_eq!(answer["from"], "src/domain/model.ts");
    assert_eq!(answer["to"], "src/web/view.ts");
    assert_eq!(answer["violations"][0]["name"], "domain-not-to-web");
    assert_eq!(answer["violations"][0]["severity"], "error");
    assert_eq!(
        answer["violations"][0]["fix"],
        "Move the shared type into src/domain"
    );
    assert!(gate_id.is_string());
    assert_eq!(answer["violations"][0]["id"], gate_id);
    let yes = run(
        &dir,
        &[
            "can-import",
            "--json",
            "src/web/view.ts",
            "src/domain/model.ts",
        ],
    )?;
    assert_eq!(yes.status.code(), Some(0));
    let answer = json(&yes)?;
    assert_eq!(answer["verdict"], "yes");
    assert_eq!(answer["violations"], serde_json::json!([]));
    // Two runs answer byte for byte alike.
    let again = run(
        &dir,
        &[
            "can-import",
            "--json",
            "src/domain/model.ts",
            "src/web/view.ts",
        ],
    )?;
    assert_eq!(again.stdout, no.stdout);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn can_import_takes_the_targets_kind_from_the_graph() -> Result<(), Box<dyn Error>> {
    let dir = tree("can-import-kind")?;
    let graph = serde_json::json!({
        "modules": [
            { "source": "src/test/a.test.ts", "dependencies": [
                { "module": "vitest", "resolved": "node_modules/vitest/index.js",
                  "dependencyTypes": ["npm-dev"], "coreModule": false, "couldNotResolve": false } ] },
            { "source": "src/domain/model.ts", "dependencies": [] }
        ],
        "summary": {}
    });
    std::fs::write(dir.join("graph.json"), graph.to_string())?;
    std::fs::write(
        dir.join("dev.yaml"),
        "forbidden:\n  - name: no-dev-deps-in-src\n    severity: error\n    from: { path: \"^src/domain/\" }\n    to: { dependencyTypes: [npm-dev] }\n",
    )?;
    let args = [
        "can-import",
        "--graph",
        "graph.json",
        "--config",
        "dev.yaml",
        "src/domain/model.ts",
        "node_modules/vitest/index.js",
    ];
    let no = run(&dir, &args)?;
    assert_eq!(no.status.code(), Some(1), "{}", stdout(&no));
    assert!(stdout(&no).contains("no-dev-deps-in-src"));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// The known violations of `options.knownViolations` as `cruise` applies them: a baselined edge
/// is `yes` with the entry noted, by id or by shape; an expired entry still blocks.
#[test]
fn can_import_applies_known_violations_as_the_gate_does() -> Result<(), Box<dyn Error>> {
    let dir = tree("can-import-known")?;
    let edge = ["src/domain/model.ts", "src/web/view.ts"];
    let before = json(&run(&dir, &["can-import", "--json", edge[0], edge[1]])?)?;
    let id = before["violations"][0]["id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(id.starts_with("RB-"), "{before}");
    // The fence alone: the ratchet's budget file is not part of this question.
    let base = CONFIG.split("rules:\n").next().unwrap_or_default();
    let with = |entries: &str| format!("{base}options:\n  knownViolations:\n{entries}");
    let by_id = format!("    - id: {id}\n      expires: 2999-01-01\n      owner: \"@me\"\n");
    let by_shape = "    - type: dependency\n      from: src/domain/model.ts\n      to: src/web/view.ts\n      rule: { name: domain-not-to-web }\n";
    let expired = format!("    - id: {id}\n      expires: 2020-01-01\n      owner: \"@me\"\n");
    for (name, entries, gate_passes) in [
        ("id", by_id.as_str(), true),
        ("shape", by_shape, true),
        ("expired", expired.as_str(), false),
    ] {
        std::fs::write(dir.join("known.yaml"), with(entries))?;
        let gate = run(&dir, &["cruise", "--config", "known.yaml", "src"])?;
        let asked = run(
            &dir,
            &[
                "can-import",
                "--config",
                "known.yaml",
                "--json",
                edge[0],
                edge[1],
            ],
        )?;
        let answer = json(&asked)?;
        assert_eq!(
            gate.status.code() == Some(0),
            gate_passes,
            "{name}: {}",
            String::from_utf8_lossy(&gate.stdout)
        );
        assert_eq!(
            asked.status.code(),
            Some(i32::from(!gate_passes)),
            "{name}: can-import agrees with cruise: {answer}"
        );
        if gate_passes {
            assert_eq!(answer["verdict"], "yes", "{name}");
            assert_eq!(answer["violations"], serde_json::json!([]));
            assert_eq!(answer["warnings"][0]["name"], "domain-not-to-web");
            assert_eq!(answer["warnings"][0]["severity"], "ignore");
            assert_eq!(answer["warnings"][0]["known"], true);
            assert_eq!(answer["warnings"][0]["id"], id.as_str());
            let text = stdout(&run(
                &dir,
                &["can-import", "--config", "known.yaml", edge[0], edge[1]],
            )?);
            assert!(
                text.starts_with("yes\n  (known violation: ") && text.contains("domain-not-to-web"),
                "{text}"
            );
        } else {
            assert_eq!(answer["verdict"], "no", "{name}");
            assert_eq!(answer["violations"][0]["severity"], "error");
        }
    }
    // An entry for another edge leaves this one blocked.
    std::fs::write(
        dir.join("known.yaml"),
        with("    - id: RB-00000000\n      expires: 2999-01-01\n"),
    )?;
    let other = run(
        &dir,
        &["can-import", "--config", "known.yaml", edge[0], edge[1]],
    )?;
    assert_eq!(other.status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// The hypothetical edge copies what the target is, never how another edge imports it.
#[test]
fn can_import_takes_the_target_not_the_import_form_of_another_edge() -> Result<(), Box<dyn Error>> {
    let dir = tree("can-import-form")?;
    let graph = serde_json::json!({
        "modules": [
            { "source": "src/legacy/a.ts", "language": "typescript", "dependencies": [
                { "module": "lodash", "resolved": "node_modules/lodash/index.js",
                  "dependencyTypes": ["npm", "require"], "coreModule": false, "couldNotResolve": false,
                  "dynamic": true, "dependencyKind": "import" },
                { "module": "./types", "resolved": "src/legacy/types.ts",
                  "dependencyTypes": ["local", "type-only", "type-import"], "coreModule": false, "couldNotResolve": false } ] },
            { "source": "src/legacy/types.ts", "language": "typescript", "dependencies": [] },
            { "source": "src/domain/model.ts", "language": "typescript", "dependencies": [] }
        ],
        "summary": {}
    });
    std::fs::write(dir.join("graph.json"), graph.to_string())?;
    std::fs::write(
        dir.join("form.yaml"),
        "forbidden:
  - name: no-require
    severity: error
    from: {}
    to: { dependencyTypes: [require] }
  - name: no-dynamic
    severity: error
    from: {}
    to: { dynamic: true }
  - name: no-type-only
    severity: error
    from: {}
    to: { dependencyTypes: [type-only, type-import] }
  - name: no-npm-in-domain
    severity: warn
    from: { path: \"^src/domain/\" }
    to: { dependencyTypes: [npm] }
  - name: es-imports-seen
    severity: info
    from: { path: \"^src/domain/\" }
    to: { dependencyTypes: [import] }
",
    )?;
    let ask = |to: &str| {
        run(
            &dir,
            &[
                "can-import",
                "--graph",
                "graph.json",
                "--config",
                "form.yaml",
                "--json",
                "src/domain/model.ts",
                to,
            ],
        )
    };
    let npm = ask("node_modules/lodash/index.js")?;
    let answer = json(&npm)?;
    assert_eq!(npm.status.code(), Some(0), "{answer}");
    let warned: Vec<&str> = answer["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| w["name"].as_str())
        .collect();
    assert_eq!(warned, ["no-npm-in-domain", "es-imports-seen"], "{answer}");
    let local = ask("src/legacy/types.ts")?;
    let answer = json(&local)?;
    assert_eq!(local.status.code(), Some(0), "{answer}");
    assert_eq!(answer["warnings"][0]["name"], "es-imports-seen", "{answer}");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn can_import_reads_the_cross_language_facts() -> Result<(), Box<dyn Error>> {
    // Wave 2, Step 8: the saved graph's `language`, `namespaces`, `project` and code-layer
    // assemblies answer the cross-language keys, and the hypothetical edge is an `import`.
    let dir = tree("can-import-cross")?;
    let graph = serde_json::json!({
        "modules": [
            { "source": "py/app/core.py", "language": "python", "namespaces": ["app.core"], "project": "app", "dependencies": [
                { "module": "app.util", "resolved": "py/app/util.py", "dependencyTypes": ["local"], "coreModule": false, "couldNotResolve": false } ] },
            { "source": "py/app/util.py", "language": "python", "namespaces": ["app.util"], "project": "app", "dependencies": [] },
            { "source": "py/app/web.py", "language": "python", "namespaces": ["app.web"], "project": "app", "dependencies": [] }
        ],
        "code": { "types": [
            { "fullName": "app.util.Helper", "name": "Helper", "kind": "class", "language": "python", "file": "py/app/util.py", "assembly": "app" }
        ] },
        "summary": {}
    });
    std::fs::write(dir.join("graph.json"), graph.to_string())?;
    std::fs::write(
        dir.join("cross.yaml"),
        "rules:\n  dependencies:\n    forbidden:\n      - name: web-not-to-util\n        severity: error\n        from: { language: python, namespace: \"^app\\\\.web$\", project: \"^app$\" }\n        to: { namespace: \"^app\\\\.util$\", assembly: \"^app$\", dependencyKind: import }\n",
    )?;
    let ask = |from: &str| {
        run(
            &dir,
            &[
                "can-import",
                "--graph",
                "graph.json",
                "--config",
                "cross.yaml",
                from,
                "py/app/util.py",
            ],
        )
    };
    let no = ask("py/app/web.py")?;
    assert_eq!(
        no.status.code(),
        Some(1),
        "{}{}",
        stdout(&no),
        String::from_utf8_lossy(&no.stderr)
    );
    assert!(stdout(&no).contains("web-not-to-util"));
    let yes = ask("py/app/core.py")?;
    assert_eq!(yes.status.code(), Some(0), "{}", stdout(&yes));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn count_writes_and_holds_the_budget() -> Result<(), Box<dyn Error>> {
    let dir = tree("count")?;
    let count = ["count", "--from", "^src/domain/", "--to", "^src/web/"];
    let bare = run(&dir, &count)?;
    assert_eq!(stdout(&bare), "1\n");
    let budget = [&count[..], &["--budget", "budgets/domain-web.json"]].concat();
    let unwritten = run(&dir, &budget)?;
    assert_eq!(
        unwritten.status.code(),
        Some(2),
        "a missing budget cannot be trusted"
    );
    let written = run(&dir, &[&budget[..], &["--write"]].concat())?;
    assert_eq!(written.status.code(), Some(0));
    let file: Value = serde_json::from_str(&std::fs::read_to_string(
        dir.join("budgets/domain-web.json"),
    )?)?;
    assert_eq!(file["ceiling"], 1);
    assert_eq!(run(&dir, &budget)?.status.code(), Some(0));
    std::fs::write(dir.join("budgets/domain-web.json"), "{\"ceiling\":0}\n")?;
    let over = run(&dir, &budget)?;
    assert_eq!(over.status.code(), Some(1), "over the ceiling is one error");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn config_commands_convert_expand_and_lint() -> Result<(), Box<dyn Error>> {
    let dir = tree("config")?;
    let converted = run(
        &dir,
        &[
            "config",
            "convert",
            "rulebearing.yaml",
            "--to",
            "dependency-cruiser",
        ],
    )?;
    assert_eq!(converted.status.code(), Some(0));
    let value = json(&converted)?;
    assert_eq!(value["forbidden"][0]["name"], "domain-not-to-web");
    assert!(
        String::from_utf8_lossy(&converted.stderr).contains("rules.ratchets[domain-web-edges]")
    );
    let expanded = stdout(&run(&dir, &["config", "expand", "rulebearing.yaml"])?);
    assert!(expanded.contains("dependencies:"));
    let linted = run(&dir, &["config", "lint", "--require-comment-token"])?;
    assert_eq!(linted.status.code(), Some(0), "{}", stdout(&linted));
    std::fs::write(
        dir.join("rulebearing.yaml"),
        CONFIG.replace(" (decision: adr:0010)", ""),
    )?;
    let untokened = run(&dir, &["config", "lint", "--require-comment-token"])?;
    assert_ne!(untokened.status.code(), Some(0));
    assert!(stdout(&untokened).contains("domain-not-to-web"));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn hooks_summary_and_impact_serve_an_agent() -> Result<(), Box<dyn Error>> {
    let dir = tree("hooks")?;
    assert_eq!(run(&dir, &["hooks", "install"])?.status.code(), Some(3));
    for _ in 0..2 {
        assert_eq!(
            run(&dir, &["hooks", "install", "--claude-code"])?
                .status
                .code(),
            Some(0)
        );
    }
    let settings: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(".claude/settings.json"))?)?;
    assert_eq!(
        settings["hooks"]["PreToolUse"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(settings["hooks"]["PreToolUse"][0]["matcher"], "Edit|Write");
    assert_eq!(
        settings["hooks"]["Stop"][0]["hooks"][0]["command"],
        STOP_HOOK
    );

    let summary = json(&run(&dir, &["summary"])?)?;
    assert_eq!(summary["openViolations"][0]["name"], "domain-not-to-web");
    assert_eq!(summary["ratchets"][0]["name"], "domain-web-edges");
    assert!(summary["ratchets"][0]["error"].is_string(), "no budget yet");
    assert!(stdout(&run(&dir, &["summary", "--format", "text"])?).contains("1 error violations"));

    let impact = json(&run(
        &dir,
        &["impact", "src/web/view.ts", "--depth", "2", "--json"],
    )?)?;
    assert_eq!(impact["rules"][0]["side"], "to");
    assert_eq!(
        impact["dependents"],
        serde_json::json!(["src/domain/model.ts", "src/main.ts"])
    );
    let root = dir.canonicalize()?;
    let hook =
        serde_json::json!({ "tool_input": { "file_path": root.join("src/domain/model.ts") } });
    // A PreToolUse hook's plain stdout never reaches the agent and exit 2 blocks the edit, so
    // the report goes back as additionalContext and the hook exits 0, even when it cannot answer.
    let from_hook = run_with_stdin(&dir, &["impact", "--from-hook"], &hook.to_string())?;
    assert_eq!(from_hook.status.code(), Some(0));
    let answer = json(&from_hook)?;
    assert_eq!(answer["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    let context = answer["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or_default();
    let value: Value = serde_json::from_str(
        context
            .strip_prefix("rulebearing impact:\n")
            .unwrap_or_default(),
    )?;
    assert_eq!(value["file"], "src/domain/model.ts");
    assert_eq!(value["rules"][0]["side"], "from");
    let bad = run_with_stdin(&dir, &["impact", "--from-hook"], "{}")?;
    assert_eq!(
        bad.status.code(),
        Some(0),
        "a hook that cannot answer does not block"
    );
    assert!(bad.stdout.is_empty());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("tool_input.file_path"));

    // The Stop hook says nothing while the run cannot be trusted (here the ratchet has no
    // budget yet), then blocks with the findings as the reason, once.
    let stop: Vec<&str> = STOP_HOOK.split(' ').skip(1).collect();
    let untrusted = run_with_stdin(&dir, &stop, "{}")?;
    assert_eq!(untrusted.status.code(), Some(0));
    assert!(untrusted.stdout.is_empty());
    std::fs::create_dir_all(dir.join("budgets"))?;
    std::fs::write(dir.join("budgets/domain-web.json"), r#"{ "ceiling": 1 }"#)?;
    let blocked = run_with_stdin(&dir, &stop, "{}")?;
    assert_eq!(blocked.status.code(), Some(0));
    let answer = json(&blocked)?;
    assert_eq!(answer["decision"], "block");
    assert!(
        answer["reason"]
            .as_str()
            .is_some_and(|r| r.contains("domain-not-to-web") && r.contains("\"cost\"")),
        "{answer}"
    );
    let again = run_with_stdin(&dir, &stop, r#"{"stop_hook_active": true}"#)?;
    assert_eq!(again.status.code(), Some(0));
    assert!(again.stdout.is_empty(), "never holds the agent in a loop");
    std::fs::write(dir.join("rulebearing.yaml"), "forbidden: []\n")?;
    let clean = run_with_stdin(&dir, &stop, "{}")?;
    assert_eq!(clean.status.code(), Some(0));
    assert!(clean.stdout.is_empty(), "no errors, nothing to say");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn attest_verifies_and_names_what_changed() -> Result<(), Box<dyn Error>> {
    let dir = tree("attest")?;
    let missing = run(&dir, &["attest", "--verify", "src"])?;
    assert_eq!(missing.status.code(), Some(2), "no receipt to verify");
    assert_eq!(run(&dir, &["attest", "src"])?.status.code(), Some(0));
    let receipt: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(".graph/attest.json"))?)?;
    for key in ["configHash", "inputsHash", "resultsHash"] {
        assert_eq!(receipt[key].as_str().map(str::len), Some(64), "{key}");
    }
    let verified = run(&dir, &["attest", "--verify", "src"])?;
    assert_eq!(
        verified.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    std::fs::write(dir.join("src/web/view.ts"), "export const w = 2;\n")?;
    let changed = run(&dir, &["attest", "--verify", "src"])?;
    assert_eq!(changed.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&changed.stderr);
    assert!(
        stderr.contains("inputsHash") && !stderr.contains("configHash"),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn cruise_and_fmt_enforce_the_ratchets() -> Result<(), Box<dyn Error>> {
    let dir = tree("ratchet")?;
    // The rule only warns, so the exit code is the ratchet's alone.
    std::fs::write(
        dir.join("rulebearing.yaml"),
        CONFIG.replace("severity: error", "severity: warn"),
    )?;
    let unbudgeted = run(&dir, &["cruise", "src"])?;
    assert_eq!(
        unbudgeted.status.code(),
        Some(2),
        "a missing budget cannot be trusted"
    );
    assert!(
        String::from_utf8_lossy(&unbudgeted.stderr)
            .contains("budgets/domain-web.json cannot be read")
    );

    std::fs::create_dir_all(dir.join("budgets"))?;
    std::fs::write(dir.join("budgets/domain-web.json"), "{\"ceiling\":1}\n")?;
    let held = run(&dir, &["cruise", "-T", "json", "src"])?;
    assert_eq!(held.status.code(), Some(0));
    let result = json(&held)?;
    assert_eq!(
        result["summary"]["ratchets"],
        serde_json::json!([{ "name": "domain-web-edges", "budget": "budgets/domain-web.json", "count": 1, "ceiling": 1, "status": "held" }])
    );
    let strict = json(&run(
        &dir,
        &["cruise", "-T", "json", "--strict-schema", "src"],
    )?)?;
    assert!(strict["summary"].get("ratchets").is_none());

    std::fs::write(dir.join("budgets/domain-web.json"), "{\"ceiling\":0}\n")?;
    let over = run(&dir, &["cruise", "-T", "err", "src"])?;
    assert_eq!(
        over.status.code(),
        Some(1),
        "an exceeded ratchet is one error"
    );
    assert!(String::from_utf8_lossy(&over.stderr).contains("over the ceiling of 0"));
    let saved = run(&dir, &["cruise", "-T", "json", "-f", "out.json", "src"])?;
    assert_eq!(
        saved.status.code(),
        Some(0),
        "json does not gate (ADR-0030)"
    );
    let json_gate = run(&dir, &["fmt", "--exit-code", "-T", "json", "out.json"])?;
    assert_eq!(json_gate.status.code(), Some(0), "nor does fmt -T json");
    let reported = run(&dir, &["fmt", "--exit-code", "-T", "err", "out.json"])?;
    assert_eq!(
        reported.status.code(),
        Some(1),
        "fmt agrees with the cruise"
    );
    let quiet = run(&dir, &["fmt", "-T", "err", "out.json"])?;
    assert_eq!(quiet.status.code(), Some(0));

    let vacuous = CONFIG.replace("severity: error", "severity: warn").replace(
        "from: { path: \"^src/domain/\" }\n      to",
        "from: { path: \"^nowhere/\" }\n      to",
    );
    std::fs::write(dir.join("rulebearing.yaml"), vacuous)?;
    let empty = run(&dir, &["cruise", "src"])?;
    assert_eq!(empty.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&empty.stderr).contains("ratchet `domain-web-edges` is vacuous")
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
