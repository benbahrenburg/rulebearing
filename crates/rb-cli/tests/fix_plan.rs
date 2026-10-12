//! End-to-end checks on `fix --plan` and `cruise --with-plans`: one expected plan document per
//! rule shape of the mutation branch, every printed candidate checked independently, and the
//! `agent` report with its plans.
//!
//! - Plan: [plan 0004, Step 9](../../../docs/plans/pending/0004-wave-4-reach.md#23-steps-for-sub-wave-4c-fix---plan-and-the-eval-harness-funded-only-by-the--12-rule)
//! - Contract: [plan 0004 § 1.6](../../../docs/plans/pending/0004-wave-4-reach.md#16-interfaces-and-contracts-this-wave-freezes)
//! - Decisions: [ADR-0077](../../../docs/adr/0077-fix-plan-candidates-are-verified-and-the-eval-harness-scores-fix-text.md),
//!   [ADR-0015](../../../docs/adr/0015-stable-violation-id.md)
//! - Requirement: [FR-REACH-03](../../../docs/prd.md#fr-reach-03)
//!
//! The mutation branch is [conformance gate 1 layer 5's patch](../../../conformance/dependency-cruiser/mutations/README.md):
//! twelve rules, one per rule shape, over dependency-cruiser's own tree. This test writes the
//! patch's files into a scratch folder with the four modules of that tree they import, so every
//! shape is planned on every pull request; layer 5 plans the same rules over the whole tree. The
//! expected documents live in `tests/fixtures/fix-plan/`; `RB_UPDATE_SNAPSHOTS=1` rewrites them.
//!
//! Each printed plan is then checked without the planner's code: the change is applied to the
//! cruise result as JSON, every edge it adds must answer `yes` from `can-import --graph`, and
//! `rules --json --graph` must count fewer violations of the rule and no more of any other.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");
const MUTATIONS_CONFIG: &str = ".dependency-cruiser.mutations.cjs";

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fix-plan")
}

fn patch() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/dependency-cruiser/mutations/mutations.patch")
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(BIN).args(args).current_dir(dir).output()?)
}

fn scratch(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-fix-plan-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The new files of a `git diff` of new files only, by path.
fn new_files(patch: &str) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in patch.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            current = Some(path.to_owned());
            files.insert(path.to_owned(), String::new());
        } else if line.starts_with("diff --git ") {
            current = None;
        } else if let (Some(path), Some(text)) = (&current, line.strip_prefix('+'))
            && let Some(body) = files.get_mut(path)
        {
            body.push_str(text);
            body.push('\n');
        }
    }
    files
}

/// The mutation branch's files, and the four modules of dependency-cruiser's tree they import.
fn mutation_tree() -> Result<PathBuf, Box<dyn Error>> {
    let dir = scratch("mutations")?;
    let mut files = new_files(&std::fs::read_to_string(patch())?);
    for (path, body) in [
        (
            "src/cli/format-meta-info.mjs",
            "export const metaInfo = 1;\n",
        ),
        ("src/utl/bus.mjs", "export const bus = 1;\n"),
        ("src/utl/array-util.mjs", "export const uniq = (a) => a;\n"),
        (
            "src/graph-utl/compare.mjs",
            "export const compareModules = 1;\n",
        ),
    ] {
        files.insert(path.to_owned(), body.to_owned());
    }
    for (path, body) in files {
        let file = dir.join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(file, body)?;
    }
    Ok(dir)
}

/// The tree under `tests/fixtures/fix-plan/apps`, copied so the cache never writes into it.
fn apps_tree(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = scratch(name)?;
    copy(&fixtures().join("apps"), &dir)?;
    Ok(dir)
}

fn copy(from: &Path, to: &Path) -> Result<(), Box<dyn Error>> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Compares `actual` with the committed file, or rewrites it under `RB_UPDATE_SNAPSHOTS`.
fn snapshot(file: &Path, actual: &str) -> Result<(), Box<dyn Error>> {
    if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(file, actual)?;
        return Ok(());
    }
    let expected = std::fs::read_to_string(file)?;
    assert_eq!(
        actual.replace("\r\n", "\n"),
        expected.replace("\r\n", "\n"),
        "{} changed; if that was intended, regenerate with RB_UPDATE_SNAPSHOTS=1",
        file.display()
    );
    Ok(())
}

/// Each rule's violation count from `rules --json`.
fn counts(dir: &Path, config: &str, graph: &str) -> Result<BTreeMap<String, u64>, Box<dyn Error>> {
    let out = run(
        dir,
        &["rules", "--json", "--config", config, "--graph", graph],
    )?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout)?;
    let mut counts = BTreeMap::new();
    for r in value["rules"].as_array().into_iter().flatten() {
        let name = r["name"].as_str().unwrap_or_default();
        // The `allowed` entries are listed one by one; their violations are `not-in-allowed`'s.
        let name = if name.starts_with("allowed[") {
            "not-in-allowed"
        } else {
            name
        };
        *counts.entry(name.to_owned()).or_default() += r["violations"].as_u64().unwrap_or_default();
    }
    Ok(counts)
}

/// A copy of an existing local edge, retargeted.
fn edge_to(template: &Value, target: &str) -> Value {
    let mut edge = template.clone();
    edge["resolved"] = json!(target);
    edge["module"] = json!(target);
    edge["circular"] = json!(false);
    edge
}

fn modules(graph: &mut Value) -> Result<&mut Vec<Value>, Box<dyn Error>> {
    graph["modules"]
        .as_array_mut()
        .ok_or_else(|| "a cruise result has modules".into())
}

/// An edge, `(from, to)`.
type Edge = (String, String);

/// `plan` applied to the result as JSON, and the edges it adds.
fn apply(
    graph: &Value,
    from: &str,
    to: &str,
    plan: &Value,
) -> Result<(Value, Vec<Edge>), Box<dyn Error>> {
    let mut trial = graph.clone();
    let template = graph["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|m| m["dependencies"].as_array().into_iter().flatten())
        .find(|d| {
            d["dependencyTypes"]
                .as_array()
                .is_some_and(|t| t.iter().any(|x| x == "local"))
        })
        .cloned()
        .ok_or("no local edge to copy")?;
    let folder = |key: &str| -> String {
        let text = plan[key].as_str().unwrap_or_default();
        match text.trim_end_matches('/') {
            "." | "" => String::new(),
            other => format!("{other}/"),
        }
    };
    let mut added = Vec::new();
    match plan["kind"].as_str() {
        Some("move") => {
            let moved = format!(
                "{}{}",
                folder("target"),
                from.rsplit('/').next().unwrap_or(from)
            );
            for m in modules(&mut trial)? {
                if m["source"] == from {
                    m["source"] = json!(moved);
                    for d in m["dependencies"].as_array().into_iter().flatten() {
                        added.push((
                            moved.clone(),
                            d["resolved"].as_str().unwrap_or_default().to_owned(),
                        ));
                    }
                }
                let source = m["source"].as_str().unwrap_or_default().to_owned();
                if let Some(Value::Array(list)) = m.get_mut("dependencies") {
                    for d in list.iter_mut().filter(|d| d["resolved"] == from) {
                        d["resolved"] = json!(moved);
                        d["module"] = json!(moved);
                        added.push((source.clone(), moved.clone()));
                    }
                }
            }
        }
        Some("port") => {
            let port = format!("{}new-module.js", folder("package"));
            let mut module = graph["modules"][0].clone();
            module["source"] = json!(port);
            module["dependencies"] = json!([]);
            modules(&mut trial)?.push(module);
            for m in modules(&mut trial)? {
                let (is_from, is_to) = (m["source"] == from, m["source"] == to);
                if let Some(Value::Array(list)) = m.get_mut("dependencies") {
                    if is_from {
                        list.retain(|d| d["resolved"] != to);
                        list.push(edge_to(&template, &port));
                    } else if is_to {
                        list.push(edge_to(&template, &port));
                    }
                }
            }
            added.push((from.to_owned(), port.clone()));
            added.push((to.to_owned(), port));
        }
        Some("invert") => {
            for m in modules(&mut trial)? {
                let is_from = m["source"] == from;
                let is_to = m["source"] == to;
                if let Some(Value::Array(list)) = m.get_mut("dependencies") {
                    if is_from {
                        list.retain(|d| d["resolved"] != to);
                    } else if is_to && !list.iter().any(|d| d["resolved"] == from) {
                        list.push(edge_to(&template, from));
                    }
                }
            }
            added.push((to.to_owned(), from.to_owned()));
        }
        other => return Err(format!("not a plan kind: {other:?}").into()),
    }
    Ok((trial, added))
}

/// Every plan in `document`, applied and checked with `can-import` and `rules --json`.
fn verify(dir: &Path, config: &str, document: &Value) -> Result<usize, Box<dyn Error>> {
    let base_file = "base.json";
    let base: Value = serde_json::from_str(&std::fs::read_to_string(dir.join(base_file))?)?;
    let before = counts(dir, config, base_file)?;
    let rule = document["rule"].as_str().unwrap_or_default();
    let mut checked = 0;
    for v in document["violations"].as_array().into_iter().flatten() {
        let (from, to) = (
            v["from"].as_str().unwrap_or_default(),
            v["to"].as_str().unwrap_or_default(),
        );
        let plans = v["plans"].as_array().cloned().unwrap_or_default();
        if plans.is_empty() {
            assert!(
                v["reason"].as_str().is_some_and(|r| !r.is_empty()),
                "{rule}: no plan and no reason"
            );
        }
        for plan in plans {
            let to = if v["from"] == v["to"] { from } else { to };
            // A reachability finding is planned on the first step of its path.
            let hop = base["summary"]["violations"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|x| x["id"] == v["id"])
                .and_then(|x| {
                    x["via"].as_array().and_then(|via| {
                        via.iter()
                            .filter_map(|step| step["name"].as_str())
                            .find(|name| *name != from)
                            .map(str::to_owned)
                    })
                });
            let to = hop.as_deref().unwrap_or(to);
            let (trial, added) = apply(&base, from, to, &plan)?;
            std::fs::write(dir.join("trial.json"), serde_json::to_string(&trial)?)?;
            // A name that resolves to no file (`./does-not-exist.mjs`) cannot be passed to
            // `can-import`, which reads its arguments as paths; the moved module keeps that import
            // as it is, and the rule counts below still check the change.
            for (a, b) in added.iter().filter(|(_, b)| !b.starts_with("./")) {
                let out = run(
                    dir,
                    &[
                        "can-import",
                        a,
                        b,
                        "--graph",
                        "trial.json",
                        "--config",
                        config,
                    ],
                )?;
                assert!(
                    out.status.success() && out.stdout.starts_with(b"yes"),
                    "{rule}: {plan}: can-import {a} {b} said {}",
                    String::from_utf8_lossy(&out.stdout)
                );
            }
            let after = counts(dir, config, "trial.json")?;
            assert!(
                after.get(rule) < before.get(rule),
                "{rule}: {plan} does not clear the violation"
            );
            for (name, count) in &after {
                assert!(
                    *count <= before.get(name).copied().unwrap_or(0),
                    "{rule}: {plan} adds a violation of {name}"
                );
            }
            checked += 1;
        }
    }
    Ok(checked)
}

#[test]
fn every_rule_shape_of_the_mutation_branch_is_planned_and_every_plan_is_legal()
-> Result<(), Box<dyn Error>> {
    let dir = mutation_tree()?;
    let expected: Value = serde_json::from_str(&std::fs::read_to_string(
        patch().with_file_name("expected.json"),
    )?)?;
    let out = run(
        &dir,
        &[
            "cruise",
            "--config",
            MUTATIONS_CONFIG,
            "-T",
            "json",
            "--no-liveness",
            "-f",
            "base.json",
        ],
    )?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut planned = 0;
    let mut checked = 0;
    for rule in expected
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["rule"].as_str())
    {
        let out = run(
            &dir,
            &[
                "fix",
                "--plan",
                rule,
                "--config",
                MUTATIONS_CONFIG,
                "--graph",
                "base.json",
            ],
        )?;
        assert_eq!(
            out.status.code(),
            Some(0),
            "{rule}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8(out.stdout)?;
        snapshot(
            &fixtures().join(format!("expected/mutations/{rule}.json")),
            &text,
        )?;
        let document: Value = serde_json::from_str(&text)?;
        assert_eq!(document["rule"], rule);
        assert_eq!(
            document["violations"].as_array().map(Vec::len),
            Some(1),
            "{rule}: one violation each"
        );
        planned += usize::from(
            document["violations"][0]["plans"]
                .as_array()
                .is_some_and(|p| !p.is_empty()),
        );
        checked += verify(&dir, MUTATIONS_CONFIG, &document)?;
    }
    // The three module shapes (orphan, required, dependents) have no edge; the other nine do.
    assert_eq!(planned, 9);
    assert!(checked >= 20, "{checked} plans checked");
    let _ = std::fs::remove_dir_all(dir);
    Ok(())
}

#[test]
fn the_contract_example_a_layer_edge_and_a_rule_no_folder_satisfies() -> Result<(), Box<dyn Error>>
{
    let dir = apps_tree("apps-plans")?;
    let out = run(
        &dir,
        &["cruise", "-T", "json", "--no-liveness", "-f", "base.json"],
    )?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for rule in ["no-cross-app-imports", "domain-not-to-web", "no-builtins"] {
        let first = run(&dir, &["fix", "--plan", rule, "--graph", "base.json"])?;
        let second = run(&dir, &["fix", "--plan", rule, "--graph", "base.json"])?;
        assert_eq!(
            first.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        assert_eq!(
            first.stdout, second.stdout,
            "{rule}: two runs print the same bytes"
        );
        let text = String::from_utf8(first.stdout)?;
        snapshot(
            &fixtures().join(format!("expected/apps/{rule}.json")),
            &text,
        )?;
        verify(&dir, "rulebearing.yaml", &serde_json::from_str(&text)?)?;
    }
    let none: Value = serde_json::from_slice(
        &run(
            &dir,
            &["fix", "--plan", "no-builtins", "--graph", "base.json"],
        )?
        .stdout,
    )?;
    assert_eq!(none["violations"][0]["plans"], json!([]));
    assert!(
        none["violations"][0]["reason"]
            .as_str()
            .is_some_and(|r| r.starts_with("no candidate is legal"))
    );
    // Without --graph the cache's extraction is planned, with the same answer.
    let cached = run(
        &dir,
        &["fix", "--plan", "no-cross-app-imports", "--no-cache"],
    )?;
    let saved = run(
        &dir,
        &[
            "fix",
            "--plan",
            "no-cross-app-imports",
            "--graph",
            "base.json",
        ],
    )?;
    assert_eq!(cached.stdout, saved.stdout);
    let _ = std::fs::remove_dir_all(dir);
    Ok(())
}

#[test]
fn the_agent_report_carries_the_plans_with_each_finding() -> Result<(), Box<dyn Error>> {
    let dir = apps_tree("apps-agent")?;
    let out = run(
        &dir,
        &["cruise", "-T", "agent", "--with-plans", "--no-cache"],
    )?;
    assert_eq!(
        out.status.code(),
        Some(3),
        "three error findings: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout)?;
    snapshot(
        &fixtures().join("expected/apps/agent-with-plans.json"),
        &text,
    )?;
    let report: Value = serde_json::from_str(&text)?;
    for rule in report["rules"].as_array().into_iter().flatten() {
        for finding in rule["violations"].as_array().into_iter().flatten() {
            assert!(finding["plans"].is_array(), "{finding}");
        }
    }
    let plain = run(&dir, &["cruise", "-T", "agent", "--no-cache"])?;
    let plain: Value = serde_json::from_slice(&plain.stdout)?;
    assert!(plain["rules"][0]["violations"][0].get("plans").is_none());
    Ok(())
}

#[test]
fn misuse_exits_with_a_named_reason() -> Result<(), Box<dyn Error>> {
    let dir = apps_tree("apps-misuse")?;
    let other = run(&dir, &["cruise", "-T", "json", "--with-plans"])?;
    assert_eq!(other.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&other.stderr)
            .contains("--with-plans applies to --output-type agent, not json")
    );
    let unknown = run(&dir, &["fix", "--plan", "no-such-rule"])?;
    assert_eq!(unknown.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("no-such-rule"));
    let missing = run(
        &dir,
        &["fix", "--plan", "no-builtins", "--graph", "missing.json"],
    )?;
    assert_eq!(missing.status.code(), Some(2));
    let bare = run(&dir, &["fix"])?;
    assert_eq!(
        bare.status.code(),
        Some(3),
        "a usage error, as every subcommand's: --plan is required"
    );
    let _ = std::fs::remove_dir_all(dir);
    Ok(())
}
