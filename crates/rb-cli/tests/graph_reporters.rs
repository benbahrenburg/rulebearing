//! The wave 2E reporters end to end: `dot`, `ddot`, `archi` / `cdot`, `flat` / `fdot`, `mermaid`,
//! `d2`, `metrics` and `err-html` through `cruise` and `fmt`, with their `reporterOptions`, the
//! `--metrics` / `--no-metrics` flags, and `fmt --collapse` reaching the reporter.
//!
//! - Plan: [Wave 2, Step 10](../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#210-step-10-reporters-and-baseline-semantics-2e)
//! - Coverage: [coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types),
//!   [coverage § Command line](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line)
//! - Contract: [ADR-0030](../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md)
//! - Requirements: [FR-OUT-01](../../../docs/prd.md#fr-out-01), [FR-CLI-08](../../../docs/prd.md#fr-cli-08)
//!
//! The byte-for-byte proof is conformance gate 1 layer 3, which runs upstream's own specs and
//! compares every reporter with upstream's implementation; these tests prove the wiring.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

const CONFIG: &str = r#"{
  "forbidden": [
    { "name": "domain-not-to-web", "severity": "error", "comment": "The domain stays apart. adr:0010",
      "from": { "path": "^src/domain/" }, "to": { "path": "^src/web/" } }
  ],
  "options": {
    "tsPreCompilationDeps": true,
    "prefix": "https://example.com/blob/main/",
    "reporterOptions": {
      "dot": {
        "showMetrics": true,
        "theme": { "graph": { "splines": "ortho", "bgcolor": "white", "aaa": "1" } }
      },
      "archi": { "collapsePattern": ["^src/[^/]+", "^lib"] },
      "mermaid": { "minify": false },
      "metrics": { "orderBy": "name" }
    }
  }
}
"#;

fn tree(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-graph-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let files = [
        (
            "src/domain/model.ts",
            "import { w } from \"../web/view\";\nexport const d = w;\n",
        ),
        (
            "src/web/view.ts",
            "import { h } from \"./helper\";\nexport const w = h;\n",
        ),
        ("src/web/helper.ts", "export const h = 1;\n"),
        (
            "src/main.ts",
            "import { d } from \"./domain/model\";\nconsole.log(d);\n",
        ),
        (".dependency-cruiser.json", CONFIG),
    ];
    for (file, text) in files {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    Ok(dir)
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .env("SOURCE_DATE_EPOCH", "1790000000")
        .env("NO_COLOR", "1")
        .output()?)
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn every_graph_reporter_renders_deterministically_and_exits_0() -> Result<(), Box<dyn Error>> {
    let dir = tree("all")?;
    for (output_type, start) in [
        ("dot", "strict digraph \"dependency-cruiser output\"{\n"),
        ("ddot", "strict digraph \"dependency-cruiser output\"{\n"),
        ("archi", "strict digraph \"dependency-cruiser output\"{\n"),
        ("cdot", "strict digraph \"dependency-cruiser output\"{\n"),
        ("flat", "strict digraph \"dependency-cruiser output\"{\n"),
        ("fdot", "strict digraph \"dependency-cruiser output\"{\n"),
        ("mermaid", "flowchart LR\n\n"),
        ("d2", "# modules\n\n"),
        ("metrics", "type "),
        ("err-html", "<!DOCTYPE html>\n"),
    ] {
        let out = run(&dir, &["cruise", "-T", output_type, "src"])?;
        assert_eq!(
            out.status.code(),
            Some(0),
            "{output_type} does not gate (ADR-0030): {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let again = run(&dir, &["cruise", "-T", output_type, "src"])?;
        assert_eq!(out.stdout, again.stdout, "{output_type}: two runs differ");
        assert!(
            text(&out).starts_with(start),
            "{output_type}: {}",
            text(&out)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn reporter_options_reach_each_reporter() -> Result<(), Box<dyn Error>> {
    let dir = tree("options")?;
    let dot = text(&run(&dir, &["cruise", "-T", "dot", "src"])?);
    // The theme's keys follow the default's, in the order the configuration gives them.
    assert!(
        dot.contains("compound=\"true\" bgcolor=\"white\" aaa=\"1\"\n")
            && dot.contains("splines=\"ortho\""),
        "{dot}"
    );
    // `showMetrics` on the reporter turns metrics on and prints the instability.
    assert!(
        dot.contains("<FONT color=\"#808080\" point-size=\"8\">"),
        "{dot}"
    );
    assert!(
        dot.contains("URL=\"https://example.com/blob/main/src/main.ts\""),
        "{dot}"
    );
    assert!(dot.contains("[xlabel=\"domain-not-to-web\" tooltip=\"domain-not-to-web\" fontcolor=\"red\" color=\"red\"]"), "{dot}");
    // `archi` collapses by its own pattern, the array joined as dependency-cruiser normalises it.
    let archi = text(&run(&dir, &["cruise", "-T", "archi", "src"])?);
    assert!(archi.contains("\"src/web\" [label=<web>"), "{archi}");
    assert!(!archi.contains("view.ts"), "{archi}");
    // `cdot` reads its own section and pries `archi`'s theme and pattern from the result.
    assert_eq!(archi, text(&run(&dir, &["cruise", "-T", "cdot", "src"])?));
    let mermaid = text(&run(&dir, &["cruise", "-T", "mermaid", "src"])?);
    assert!(
        mermaid.contains("subgraph src_web[\"web\"]"),
        "minify: false: {mermaid}"
    );
    let metrics = text(&run(&dir, &["cruise", "-T", "metrics", "src"])?);
    let names: Vec<&str> = metrics
        .lines()
        .filter(|l| l.starts_with("module"))
        .filter_map(|l| l.split_whitespace().nth(1))
        .collect();
    assert_eq!(
        names,
        [
            "src/domain/model.ts",
            "src/main.ts",
            "src/web/helper.ts",
            "src/web/view.ts"
        ],
        "orderBy name, and metrics computed for -T metrics without --metrics"
    );
    let html = text(&run(&dir, &["cruise", "-T", "err-html", "src"])?);
    assert!(html.contains("<strong>1</strong> errors"), "{html}");
    assert!(
        html.contains("dependency-cruiser@18.2.0</a> /\n      2026-09-21T14:13:20.000Z</p>"),
        "{html}"
    );
    assert!(
        html.contains("<a href=\"https://example.com/blob/main/src/domain/model.ts\">"),
        "{html}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn metrics_flags_and_fmt_collapse() -> Result<(), Box<dyn Error>> {
    let dir = tree("flags")?;
    let json = |args: &[&str]| -> Result<Value, Box<dyn Error>> {
        let all = [&["cruise", "-T", "json"][..], args, &["src"]].concat();
        Ok(serde_json::from_slice(&run(&dir, &all)?.stdout)?)
    };
    assert!(json(&["--metrics"])?.get("folders").is_some());
    assert!(json(&[])?.get("folders").is_none());
    assert!(
        json(&["--metrics", "--no-metrics"])?
            .get("folders")
            .is_none(),
        "the later flag wins"
    );
    assert!(
        json(&["--no-metrics", "--metrics"])?
            .get("folders")
            .is_some()
    );
    let saved = dir.join("saved.json");
    std::fs::write(
        &saved,
        run(&dir, &["cruise", "-T", "json", "--metrics", "src"])?.stdout,
    )?;
    let saved = saved.to_string_lossy().into_owned();
    // `fmt --collapse` reaches the reporter as `collapsePattern`, as `reportWrap` passes it.
    let collapsed = text(&run(
        &dir,
        &["fmt", "-T", "flat", "--collapse", "^src/[^/]+", &saved],
    )?);
    assert!(
        collapsed.contains("\"src/web\" [label=<src/<BR/><B>web</B>>"),
        "{collapsed}"
    );
    let metrics = run(&dir, &["fmt", "-T", "metrics", &saved])?;
    assert_eq!(metrics.status.code(), Some(0));
    assert!(
        text(&metrics).contains("folder  src "),
        "{}",
        text(&metrics)
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

const MULTI: &str = r#"languages:
  dotnet:
    assemblies: ["dotnet/*.dll"]
  python:
    roots: ["py"]
rules:
  dependencies:
    forbidden:
      - name: python-app-not-to-util
        comment: "A test rule over the Python half."
        severity: warn
        from: { path: "^py/app/core\\.py$" }
        to: { path: "^py/app/util\\.py$" }
"#;

/// Parity+: the graph reporters read the graph document, so .NET and Python modules render as
/// TypeScript ones do.
#[test]
fn dotnet_and_python_modules_render() -> Result<(), Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-graph-multi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (file, text) in [
        ("rulebearing.yaml", MULTI),
        ("py/app/__init__.py", ""),
        ("py/app/core.py", "from app import util\n"),
        ("py/app/util.py", "VALUE = 1\n"),
    ] {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    let built = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rb-extract-dotnet/tests/fixtures/sample/built");
    std::fs::create_dir_all(dir.join("dotnet"))?;
    for file in ["Sample.dll", "Sample.pdb"] {
        std::fs::copy(built.join(file), dir.join("dotnet").join(file))?;
    }
    for output_type in [
        "dot", "ddot", "archi", "flat", "mermaid", "d2", "metrics", "err-html",
    ] {
        let out = run(&dir, &["cruise", "-T", output_type, "--no-progress", "py"])?;
        let report = text(&out);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{output_type}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            report.contains("core.py") || report.contains("py/app"),
            "{output_type}: {report}"
        );
        assert!(
            report.contains(".cs")
                || output_type == "archi"
                || output_type == "ddot"
                || output_type == "err-html",
            "{output_type}: {report}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
