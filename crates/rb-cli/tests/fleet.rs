//! `fleet` and declared edges end to end, over a three-repository fixture: a TypeScript web app,
//! a compiled .NET api and a Python pipeline.
//!
//! - Plan: [plan 0004, Steps 11 and 12](../../../docs/plans/pending/0004-wave-4-reach.md#24-steps-for-sub-wave-4d-fleet-and-declared-edges)
//!   (the three-repository fixture with one fleet rule that fires and one that does not; a
//!   missing repository exits 2; `edges.yaml`; an OpenAPI pair with and without operation ids;
//!   the liveness case), [§ 3, Wave 4D](../../../docs/plans/pending/0004-wave-4-reach.md#wave-4d-fleet-and-declared-edges)
//!   (per-repository caches reused; `--strict-schema` strips the marks)
//! - Decisions: [ADR-0077](../../../docs/adr/0077-fleet-and-declared-edges.md),
//!   [ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md),
//!   [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md),
//!   [ADR-0015](../../../docs/adr/0015-stable-violation-id.md)
//! - Requirement: [FR-REACH-03](../../../docs/prd.md#fr-reach-03)
//! - Fixture: [`fixtures/fleet`](fixtures/fleet/README.md), with the api's assemblies from
//!   [`fixtures/init-layers`](fixtures/init-layers/PROVENANCE.md)
//!
//! `expected/fleet.err-long.txt` is the `err-long` report byte for byte; regenerate it
//! deliberately with `RB_UPDATE_SNAPSHOTS=1 cargo test -p rb-cli --test fleet`.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

/// The variables git sets for a hook; each command here runs without them, so a `pre-push` hook
/// cannot point git at this repository instead of the scratch one.
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

/// The .NET module paths, as the fixture's portable PDBs record them.
const API: &str = "api/crates/rb-cli/tests/fixtures/init-layers/src";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Copies a folder, dot-folders included.
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

fn isolated(program: &str, dir: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(dir)
        .env("SOURCE_DATE_EPOCH", "1790000000");
    for name in GIT_LOCAL_ENV {
        command.env_remove(name);
    }
    // The caller's virtual environment is not the fixture's.
    command.env_remove("VIRTUAL_ENV");
    command
}

fn git(dir: &Path, args: &[&str]) -> Result {
    let output = isolated("git", dir)
        .args([
            "-c",
            "user.name=rulebearing",
            "-c",
            "user.email=rulebearing@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "init.defaultBranch=main",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

/// The fixture in a fresh folder: the api holds the init-layers solution with each assembly
/// where `dotnet build` puts it, and each repository is a git repository with one commit, as a
/// fleet's repositories are.
fn fleet(tag: &str) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rb-cli-fleet-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(&fixtures().join("fleet"), &dir)?;
    let api = dir.join("api");
    copy(&fixtures().join("init-layers"), &api)?;
    for project in ["Domain", "Application", "Infrastructure", "Web"] {
        let output = api.join(format!("src/Shop.{project}/bin/Debug/net10.0"));
        std::fs::create_dir_all(&output)?;
        for extension in ["dll", "pdb"] {
            let file = format!("Shop.{project}.{extension}");
            std::fs::copy(api.join("built").join(&file), output.join(&file))?;
        }
    }
    for repo in ["web", "api", "pipeline"] {
        let root = dir.join(repo);
        std::fs::write(root.join(".gitignore"), ".graph/\n")?;
        git(&root, &["init", "-q"])?;
        git(&root, &["add", "-A"])?;
        git(&root, &["commit", "-q", "-m", "fixture"])?;
    }
    Ok(rb_model::without_verbatim(&dir.canonicalize()?))
}

fn run(dir: &Path, args: &[&str]) -> Result<Output> {
    Ok(isolated(BIN, dir).args(args).output()?)
}

/// `fleet -T json` with `extra`: the exit code, the document and stderr.
fn json(dir: &Path, extra: &[&str]) -> Result<(Option<i32>, Value, String)> {
    let output = run(dir, &[&["fleet", "-T", "json"], extra].concat())?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("not JSON ({e}); stderr: {stderr}"))?;
    Ok((output.status.code(), value, stderr))
}

fn violations<'v>(document: &'v Value, rule: &str) -> Vec<&'v Value> {
    document["summary"]["violations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["rule"]["name"] == rule)
        .collect()
}

/// Every dependency marked `declared`, as `(from, module, to)`.
fn declared(document: &Value) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for module in document["modules"].as_array().into_iter().flatten() {
        for dependency in module["dependencies"].as_array().into_iter().flatten() {
            if dependency["declared"] == true {
                out.push((
                    module["source"].as_str().unwrap_or_default().to_owned(),
                    dependency["module"].as_str().unwrap_or_default().to_owned(),
                    dependency["resolved"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                ));
            }
        }
    }
    out
}

#[test]
fn three_repositories_one_report_with_per_repository_and_fleet_rules() -> Result {
    let dir = fleet("report")?;
    let (code, document, stderr) = json(&dir, &[])?;
    assert_eq!(code, Some(0), "json does not gate: {stderr}");
    let receipt = &document["summary"]["fleet"];
    assert_eq!(receipt["repos"], 3);
    let per_repo = receipt["perRepo"].as_object().cloned().unwrap_or_default();
    assert_eq!(
        per_repo.keys().map(String::as_str).collect::<Vec<_>>(),
        ["api", "pipeline", "web"]
    );
    assert_eq!(per_repo["web"]["warn"], 1, "the web app's own rule fired");
    assert_eq!(per_repo["api"]["error"], 0);
    assert_eq!(per_repo["api"]["inspected"]["dotnet"]["assemblies"], 4);
    assert!(per_repo["pipeline"]["inspected"]["python"].is_object());
    assert!(per_repo["web"]["inspected"]["typescript"].is_object());
    // One fleet rule fires, through a declared edge; one does not.
    let fired = violations(&document, "web-only-through-endpoints");
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0]["from"], "web/src/legacy.ts");
    assert_eq!(
        fired[0]["to"],
        format!("{API}/Shop.Infrastructure/OrderStore.cs")
    );
    assert!(violations(&document, "web-never-reaches-pipeline").is_empty());
    // The repository's own finding, its paths prefixed and its id recomputed from them.
    let own = violations(&document, "generated-only-through-client");
    assert_eq!(own.len(), 1);
    assert_eq!(
        (own[0]["from"].as_str(), own[0]["to"].as_str()),
        (
            Some("web/src/app.ts"),
            Some("web/src/api/generated/orders.ts")
        )
    );
    let kind = document["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["source"] == "web/src/app.ts")
        .flat_map(|m| m["dependencies"].as_array().into_iter().flatten())
        .find(|d| d["resolved"] == "web/src/api/generated/orders.ts")
        .and_then(|d| d["dependencyKind"].as_str())
        .unwrap_or_default();
    assert_eq!(
        own[0]["id"].as_str(),
        Some(
            rb_model::violation_id::violation_id(
                "generated-only-through-client",
                "web/src/app.ts",
                "web/src/api/generated/orders.ts",
                kind
            )
            .as_str()
        )
    );
    assert_eq!(
        own[0]["fix"],
        "Import the generated client through src/api/client.ts."
    );
    let summary = &document["summary"];
    assert_eq!(
        (summary["error"].as_u64(), summary["warn"].as_u64()),
        (Some(1), Some(2))
    );
    assert!(
        stderr.contains("warning: declared edge web/src/gone.ts -> "),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn the_fleet_document_names_every_path_and_validates_against_the_superset_schema() -> Result {
    let dir = fleet("document")?;
    let (_, document, _) = json(&dir, &[])?;
    let summary = &document["summary"];
    // Every repository path is prefixed; a core module and a standard-library module are not.
    let sources: Vec<&str> = document["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["source"].as_str())
        .collect();
    for source in &sources {
        assert!(
            ["web/", "api/", "pipeline/"]
                .iter()
                .any(|p| source.starts_with(p))
                || ["System.Collections", "System.Runtime", "json"].contains(source),
            "{source}"
        );
    }
    assert!(sources.contains(&"pipeline/pipeline/worker.py"));
    // The fleet's rules and the repositories' are both in the rule set a reporter reads.
    let forbidden: Vec<&str> = summary["ruleSetUsed"]["forbidden"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["name"].as_str())
        .collect();
    for rule in [
        "web-only-through-endpoints",
        "generated-only-through-client",
        "jobs-do-not-start-the-worker",
    ] {
        assert!(forbidden.contains(&rule), "{rule}: {forbidden:?}");
    }
    // The whole document validates against the published superset schema.
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/v1.json"),
    )?)?;
    let validator = jsonschema::draft7::new(&schema)?;
    let errors: Vec<String> = validator
        .iter_errors(&document)
        .take(3)
        .map(|e| format!("{}: {e}", e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_gating_reporter_exits_with_the_fleet_and_repository_error_count() -> Result {
    let dir = fleet("gate")?;
    let output = run(&dir, &["fleet", "-T", "err-long"])?;
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let expected = fixtures().join("fleet/expected/fleet.err-long.txt");
    if std::env::var_os("RB_UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(expected.parent().unwrap_or(&dir))?;
        std::fs::write(&expected, &text)?;
    }
    assert_eq!(
        text,
        std::fs::read_to_string(&expected)?,
        "fleet.err-long.txt changed; if that was intended, regenerate with RB_UPDATE_SNAPSHOTS=1"
    );
    let strict = run(&dir, &["fleet", "-T", "err", "--exit-code-mode", "strict"])?;
    assert_eq!(strict.status.code(), Some(11));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// The document with each repository's cache verdict taken out, as text.
fn without_cache_verdicts(mut document: Value) -> String {
    if let Some(per_repo) = document["summary"]["fleet"]["perRepo"].as_object_mut() {
        for repo in per_repo.values_mut() {
            if let Some(repo) = repo.as_object_mut() {
                repo.remove("cache");
            }
        }
    }
    serde_json::to_string_pretty(&document).unwrap_or_default()
}

fn hits(document: &Value) -> Vec<bool> {
    document["summary"]["fleet"]["perRepo"]
        .as_object()
        .into_iter()
        .flat_map(|repos| repos.values())
        .map(|repo| repo["cache"]["hit"] == true)
        .collect()
}

#[test]
fn the_cache_of_each_repository_is_written_then_reused() -> Result {
    let dir = fleet("cache")?;
    let (_, cold, _) = json(&dir, &[])?;
    assert_eq!(hits(&cold), [false, false, false]);
    for repo in ["web", "api"] {
        assert!(dir.join(repo).join(".graph/cache").is_dir(), "{repo}");
    }
    // The pipeline's configuration names its own cache folder, and the fleet keeps it.
    assert!(dir.join("pipeline/.graph/pipeline-cache").is_dir());
    assert!(!dir.join("pipeline/.graph/cache").exists());
    let (_, warm, _) = json(&dir, &[])?;
    assert_eq!(
        hits(&warm),
        [true, true, true],
        "every repository's cache reused"
    );
    assert_eq!(
        without_cache_verdicts(cold),
        without_cache_verdicts(warm.clone())
    );
    // Two warm runs are byte for byte the same document.
    let (_, again, _) = json(&dir, &[])?;
    assert_eq!(
        serde_json::to_string(&warm)?,
        serde_json::to_string(&again)?
    );
    // --no-cache reads every repository afresh and writes no verdict.
    let (_, fresh, _) = json(&dir, &["--no-cache"])?;
    assert!(
        fresh["summary"]["fleet"]["perRepo"]["web"]
            .get("cache")
            .is_none()
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_missing_repository_or_a_malformed_input_stops_the_fleet_with_a_named_reason() -> Result {
    let dir = fleet("errors")?;
    let table: [(&str, &str, &[&str], i32, &str); 5] = [
        (
            "missing.yaml",
            "repos: [{ name: web, path: web }, { name: gone, path: nowhere }]\n",
            &[],
            2,
            "repository `gone`",
        ),
        (
            "elements.yaml",
            "repos: [{ name: web, path: web }]\nrules: { elements: [] }\n",
            &[],
            3,
            "rules.elements",
        ),
        (
            "edges-bad.yaml",
            "declared: [{ from: web/src/app.ts }]\n",
            &["--edges"],
            2,
            "missing field `to`",
        ),
        (
            "server-elsewhere.yaml",
            "openapi: [{ client: web/src/api/generated/, server: other/openapi.json }]\n",
            &["--edges"],
            2,
            "names no repository of the manifest",
        ),
        (
            "server-not-openapi.yaml",
            "openapi: [{ client: web/src/api/generated/, server: web/package.json }]\n",
            &["--edges"],
            2,
            "neither `openapi` nor `swagger`",
        ),
    ];
    for (file, text, flag, code, says) in table {
        std::fs::write(dir.join(file), text)?;
        let args: Vec<&str> = if flag.is_empty() {
            vec!["fleet", file]
        } else {
            vec!["fleet", "fleet.yaml", "--edges", file]
        };
        let output = run(&dir, &args)?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(code), "{file}: {stderr}");
        assert!(
            stderr.contains("rulebearing fleet: ") && stderr.contains(says),
            "{file}: {stderr}"
        );
    }
    // A repository whose own configuration is invalid stops the fleet with its exit 3.
    std::fs::write(
        dir.join("pipeline/architecture.yaml"),
        "rules: { nonsense: 1 }\n",
    )?;
    let output = run(&dir, &["fleet"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(3), "{stderr}");
    assert!(
        stderr.contains("repository `pipeline` (pipeline)"),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn declared_edges_are_marked_and_strict_schema_strips_the_marks() -> Result {
    let dir = fleet("declared")?;
    let (_, document, _) = json(&dir, &[])?;
    let marked = declared(&document);
    let infrastructure = format!("{API}/Shop.Infrastructure/OrderStore.cs");
    let place_order = format!("{API}/Shop.Application/PlaceOrder.cs");
    assert!(marked.contains(&(
        "web/src/legacy.ts".to_owned(),
        infrastructure.clone(),
        infrastructure.clone()
    )));
    assert!(marked.contains(&(
        place_order.clone(),
        "pipeline/pipeline/worker.py".to_owned(),
        "pipeline/pipeline/worker.py".to_owned()
    )));
    let queue = document["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["source"] == place_order.as_str())
        .flat_map(|m| m["dependencies"].as_array().into_iter().flatten())
        .find(|d| d["declared"] == true)
        .cloned()
        .unwrap_or_default();
    assert_eq!(queue["dependencyKind"], "declared");
    assert_eq!(queue["declaredKind"], "queue");
    assert_eq!(
        queue["comment"],
        "PlaceOrder enqueues an order the worker picks up. plan:fleet-fixture"
    );
    assert_eq!(queue["moduleSystem"], "clr");
    assert_eq!(queue["dependencyTypes"], serde_json::json!([]));
    assert_eq!(queue["followable"], false);
    let receipt = &document["summary"]["declared"];
    assert_eq!(
        (receipt["edgesYaml"].as_u64(), receipt["openapi"].as_u64()),
        (Some(2), Some(2))
    );
    assert_eq!(receipt["skipped"][0]["from"], "web/src/gone.ts");
    // No detected edge carries a mark.
    for module in document["modules"].as_array().into_iter().flatten() {
        for dependency in module["dependencies"].as_array().into_iter().flatten() {
            if dependency["declared"] != true {
                for key in ["declaredKind", "comment"] {
                    assert!(dependency.get(key).is_none(), "{dependency}");
                }
                assert_ne!(dependency["dependencyKind"], "declared");
            }
        }
    }
    let (_, strict, _) = json(&dir, &["--strict-schema"])?;
    let text = serde_json::to_string(&strict)?;
    for gone in [
        "\"declared\"",
        "\"declaredKind\"",
        "\"comment\":\"PlaceOrder",
        "\"dependencyKind\"",
        "\"fleet\"",
        "perRepo",
    ] {
        assert!(!text.contains(gone), "{gone} left after --strict-schema");
    }
    assert_eq!(
        declared(&strict).len(),
        0,
        "the marks are gone; the edges stay, as ADR-0004 strips additions"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_typescript_fleet_with_declared_edges_validates_against_cruise_result_after_stripping() -> Result
{
    let dir = fleet("strict")?;
    std::fs::write(
        dir.join("ts-only.yaml"),
        "repos: [{ name: web, path: web, config: rulebearing.yaml }, { name: shop, path: shop }]\n",
    )?;
    copy(&dir.join("web"), &dir.join("shop"))?;
    std::fs::remove_file(dir.join("shop/rulebearing.yaml"))?;
    std::fs::write(
        dir.join("ts-edges.yaml"),
        "declared:\n  - { from: web/src/legacy.ts, to: shop/src/api/client.ts, kind: http }\n",
    )?;
    let output = run(
        &dir,
        &[
            "fleet",
            "ts-only.yaml",
            "--edges",
            "ts-edges.yaml",
            "-T",
            "json",
            "--strict-schema",
        ],
    )?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    let document: Value = serde_json::from_slice(&output.stdout)?;
    let edge = document["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["source"] == "web/src/legacy.ts")
        .flat_map(|m| m["dependencies"].as_array().into_iter().flatten())
        .find(|d| d["resolved"] == "shop/src/api/client.ts")
        .cloned();
    assert!(edge.is_some(), "the declared edge stays, unmarked");
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../conformance/dependency-cruiser/fixtures/schemas/cruise-result.schema.json",
        ),
    )?)?;
    let validator = jsonschema::draft7::new(&schema)?;
    let errors: Vec<String> = validator
        .iter_errors(&document)
        .take(3)
        .map(|e| format!("{}: {e}", e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn openapi_edges_come_from_operation_ids_and_without_them_the_receipt_says_why() -> Result {
    let dir = fleet("openapi")?;
    let (_, document, _) = json(&dir, &[])?;
    let endpoint = format!("{API}/Shop.Web/OrdersEndpoint.cs");
    let from_openapi: Vec<(String, String, String)> = declared(&document)
        .into_iter()
        .filter(|(_, module, _)| !module.contains('/'))
        .collect();
    assert_eq!(
        from_openapi,
        [
            (
                "web/src/api/generated/orders.ts".to_owned(),
                "createOrder".to_owned(),
                endpoint.clone()
            ),
            (
                "web/src/api/generated/orders.ts".to_owned(),
                "listOrders".to_owned(),
                endpoint
            ),
        ]
    );
    let source = &document["summary"]["declared"]["openapiSources"][0];
    assert_eq!(
        (
            source["operations"].as_u64(),
            source["withoutOperationId"].as_u64(),
            source["withoutHandler"].as_u64(),
            source["edges"].as_u64()
        ),
        (Some(4), Some(1), Some(1), Some(2))
    );
    assert!(source.get("reason").is_none());
    let (code, without, stderr) = json(&dir, &["--edges", "edges-without-operation-ids.yaml"])?;
    assert_eq!(code, Some(0), "{stderr}");
    assert!(declared(&without).is_empty(), "no operation id, no edge");
    let receipt = &without["summary"]["declared"];
    assert_eq!(receipt["openapi"], 0);
    let source = &receipt["openapiSources"][0];
    assert_eq!(source["withoutOperationId"], 2);
    assert!(
        source["reason"]
            .as_str()
            .is_some_and(|r| r.contains("records no operationId")),
        "{source}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// `config lint --fleet fleet.yaml` with `extra`: the finding lines.
fn lint(dir: &Path, extra: &[&str]) -> Result<String> {
    let output = run(
        dir,
        &[&["config", "lint", "--fleet", "fleet.yaml"], extra].concat(),
    )?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[test]
fn a_cross_language_rule_that_matched_nothing_matches_through_a_declared_edge() -> Result {
    let dir = fleet("liveness")?;
    let rule = "api-hands-orders-to-the-worker";
    // Without declared edges, the rule from the .NET api to the Python pipeline matches nothing.
    std::fs::rename(dir.join("edges.yaml"), dir.join("edges.later"))?;
    let output = run(&dir, &["fleet", "-T", "json", "-f", "before.json"])?;
    assert_eq!(output.status.code(), Some(0));
    let before: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("before.json"))?)?;
    assert!(violations(&before, rule).is_empty());
    assert!(before["summary"].get("declared").is_none());
    let findings = lint(&dir, &["--graph", "before.json"])?;
    assert!(
        findings.contains(&format!(
            "never-matches {rule}: to.path matches nothing in the graph"
        )),
        "{findings}"
    );
    // With the edge edges.yaml declares, it matches the edge and fires.
    std::fs::rename(dir.join("edges.later"), dir.join("edges.yaml"))?;
    let output = run(&dir, &["fleet", "-T", "json", "-f", "after.json"])?;
    assert_eq!(output.status.code(), Some(0));
    let after: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("after.json"))?)?;
    let fired = violations(&after, rule);
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0]["to"], "pipeline/pipeline/worker.py");
    assert_eq!(fired[0]["rule"]["severity"], "warn");
    let findings = lint(&dir, &["--graph", "after.json"])?;
    assert!(
        !findings.contains(&format!("never-matches {rule}")),
        "{findings}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn config_lint_reports_declared_edges_that_no_longer_exist() -> Result {
    let dir = fleet("stale")?;
    let output = run(&dir, &["fleet", "-T", "json", "-f", "graph.json"])?;
    assert_eq!(output.status.code(), Some(0));
    let findings = lint(&dir, &["--graph", "graph.json"])?;
    assert!(
        findings.contains(
            "stale-declared-edge declared[2].from: `web/src/gone.ts` is not a module of the graph"
        ),
        "{findings}"
    );
    assert_eq!(
        findings.matches("stale-declared-edge").count(),
        1,
        "{findings}"
    );
    // Without a graph, against the repositories' files.
    std::fs::write(
        dir.join("files.yaml"),
        "declared:\n  - { from: web/src/app.ts, to: pipeline/pipeline/worker.py }\n  - { from: web/src/app.ts, to: pipeline/pipeline/gone.py }\nopenapi:\n  - { client: web/src/api/generated/, server: api/missing.json }\n",
    )?;
    let output = run(
        &dir,
        &[
            "config",
            "lint",
            "--fleet",
            "fleet.yaml",
            "--edges",
            "files.yaml",
        ],
    )?;
    let findings = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(2), "{findings}");
    assert!(findings.contains(
        "stale-declared-edge declared[1].to: `pipeline/pipeline/gone.py` is not a file of its repository"
    ));
    assert!(findings.contains(
        "stale-declared-edge openapi[0].server: `api/missing.json` is not a file of its repository"
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn the_report_flags_reach_the_reporter_and_a_vacuous_fleet_rule_fails_the_run() -> Result {
    let dir = fleet("flags")?;
    let agent = |extra: &[&str]| -> Result<Value> {
        let output = run(&dir, &[&["fleet", "-T", "agent"], extra].concat())?;
        Ok(serde_json::from_slice(&output.stdout)?)
    };
    assert_eq!(agent(&[])?["budget"]["truncated"], false);
    let capped = agent(&["--max-findings", "0"])?;
    assert_eq!(capped["budget"]["truncated"], true, "{capped}");
    let colored = run(&dir, &["fleet", "-T", "err-long", "--color", "always"])?;
    assert!(String::from_utf8_lossy(&colored.stdout).contains('\u{1b}'));
    let plain = run(&dir, &["fleet", "-T", "err-long", "--color", "never"])?;
    assert!(!String::from_utf8_lossy(&plain.stdout).contains('\u{1b}'));
    // A fleet rule whose `from` matches no module of the union fails the run, as in `cruise`.
    std::fs::write(dir.join("no-edges.yaml"), "declared: []\n")?;
    std::fs::write(
        dir.join("vacuous.yaml"),
        "repos: [{ name: web, path: web, config: rulebearing.yaml }]\nrules:\n  dependencies:\n    forbidden:\n      - { name: from-nowhere, from: { path: '^nowhere/' }, to: {} }\n",
    )?;
    let output = run(&dir, &["fleet", "vacuous.yaml", "--edges", "no-edges.yaml"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("rule `from-nowhere` is vacuous"),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn a_repository_read_from_source_decides_the_exit_code_only_when_allowed() -> Result {
    let dir = fleet("source")?;
    let config = dir.join("api/rulebearing.yaml");
    let text = std::fs::read_to_string(&config)?;
    std::fs::write(
        &config,
        format!("languages: {{ dotnet: {{ mode: source }} }}\n{text}"),
    )?;
    let refused = run(&dir, &["fleet", "-T", "err"])?;
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("approximate"), "{stderr}");
    let allowed = run(&dir, &["fleet", "-T", "err", "--allow-approximate-gate"])?;
    let stderr = String::from_utf8_lossy(&allowed.stderr);
    assert_ne!(allowed.status.code(), Some(2), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
