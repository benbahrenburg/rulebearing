//! The wave 3B reporters end to end: `markdown`, `html`, `anon` and `x-dot-webpage` through
//! `cruise` and `fmt` with their `reporterOptions`, and `wrap-html` byte-compared with the page
//! dependency-cruiser's `depcruise-wrap-stream-in-html` wrote for the same SVG.
//!
//! - Plan: [Wave 3, Steps 6 and 8](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//! - Coverage: [coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types),
//!   [coverage § Command line](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line)
//! - Contracts: [ADR-0030](../../../docs/adr/0030-the-reporter-decides-the-error-count-exit.md),
//!   [ADR-0053](../../../docs/adr/0053-x-dot-webpage-draws-with-graphviz-dot.md)
//! - Requirements: [FR-OUT-01](../../../docs/prd.md#fr-out-01), [FR-CLI-08](../../../docs/prd.md#fr-cli-08)
//!
//! The byte-for-byte proof of the reporters is conformance gate 1 layer 3, which runs upstream's
//! specs and compares each reporter with upstream's implementation; these tests prove the wiring.
//! `tests/fixtures/wrap-html/expected.html` was written by dependency-cruiser 18.2.0's
//! `bin/wrap-stream-in-html.mjs` from `input.svg` (itself drawn by GraphViz' `dot`).

use std::error::Error;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_rulebearing");

const CONFIG: &str = r#"{
  "forbidden": [
    { "name": "domain-not-to-web", "severity": "error", "comment": "The domain stays apart. adr:0010",
      "from": { "path": "^src/domain/" }, "to": { "path": "^src/web/" } }
  ],
  "options": {
    "tsPreCompilationDeps": true,
    "reporterOptions": {
      "markdown": { "title": "Architecture check", "collapseDetails": false },
      "anon": { "wordlist": ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf"] },
      "dot": { "theme": { "graph": { "bgcolor": "white" } } }
    }
  }
}
"#;

fn tree(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("rb-cli-wave3-{name}-{}", std::process::id()));
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

fn run_env(dir: &Path, args: &[&str], path: Option<&str>) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new(BIN);
    command
        .args(args)
        .current_dir(dir)
        .env("SOURCE_DATE_EPOCH", "1790000000")
        .env("NO_COLOR", "1");
    if let Some(path) = path {
        command.env("PATH", path);
    }
    Ok(command.output()?)
}

fn run(dir: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    run_env(dir, args, None)
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn graphviz_installed() -> bool {
    Command::new("dot")
        .arg("-V")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn markdown_html_and_anon_render_deterministically_and_exit_0() -> Result<(), Box<dyn Error>> {
    let dir = tree("render")?;
    for (output_type, expected) in [
        ("markdown", "Architecture check\n\n"),
        ("html", "<!DOCTYPE html>\n<html>\n"),
        ("anon", "{\n  \"modules\": [\n"),
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
            text(&out).starts_with(expected),
            "{output_type}: {}",
            text(&out)
        );
    }
    // The markdown options reach the reporter; the footer carries the pinned clock.
    let markdown = text(&run(&dir, &["cruise", "-T", "markdown", "src"])?);
    assert!(!markdown.contains("<details>"), "{markdown}");
    assert!(
        markdown.contains(
            "|:exclamation:&nbsp;_domain-not-to-web_|src/domain/model.ts|src/web/view.ts|"
        ),
        "{markdown}"
    );
    assert!(
        markdown.contains(") / 2026-09-21T14:13:20.000Z\n\n"),
        "{markdown}"
    );
    // The matrix marks the forbidden edge with its severity and the rule.
    let html = text(&run(&dir, &["cruise", "-T", "html", "src"])?);
    assert!(
        html.contains("<td class=\"cell cell-error\" title=\"domain-not-to-web:\nsrc/domain/model.ts -> src/web/view.ts\"></td>"),
        "{html}"
    );
    // The word list replaces the names; the whitelisted `src` stays.
    let anon = text(&run(&dir, &["cruise", "-T", "anon", "src"])?);
    assert!(
        !anon.contains("src/domain/model.ts") && !anon.contains("helper.ts"),
        "{anon}"
    );
    assert!(
        anon.contains("\"resolved\": \"src/alpha/bravo.ts\""),
        "{anon}"
    );
    // fmt re-reports a saved result the same way.
    let json = run(&dir, &["cruise", "-T", "json", "-f", "result.json", "src"])?;
    assert_eq!(json.status.code(), Some(0));
    let formatted = run(&dir, &["fmt", "-T", "markdown", "result.json"])?;
    assert_eq!(text(&formatted), markdown);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn x_dot_webpage_draws_with_graphviz_or_exits_2() -> Result<(), Box<dyn Error>> {
    let dir = tree("webpage")?;
    // Without `dot` on PATH the report cannot be made: exit 2 with upstream's message.
    let empty = dir.join("no-graphviz");
    std::fs::create_dir_all(&empty)?;
    let empty = empty.to_string_lossy().into_owned();
    let out = run_env(
        &dir,
        &["cruise", "-T", "x-dot-webpage", "src"],
        Some(&empty),
    )?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("GraphViz dot, which is required for the 'x-dot-webpage' reporter"),
        "{stderr}"
    );
    assert!(out.stdout.is_empty());
    let saved = run(&dir, &["cruise", "-T", "json", "-f", "result.json", "src"])?;
    assert_eq!(saved.status.code(), Some(0));
    let fmt = run_env(
        &dir,
        &["fmt", "-T", "x-dot-webpage", "result.json"],
        Some(&empty),
    )?;
    assert_eq!(fmt.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&fmt.stderr).contains("GraphViz dot"));
    if graphviz_installed() {
        let out = run(&dir, &["cruise", "-T", "x-dot-webpage", "src"])?;
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let page = text(&out);
        assert!(page.starts_with("<!doctype html>\n<html lang=\"en\" dir=\"ltr\">\n"));
        assert!(page.contains("<svg") && page.ends_with("  </body>\n</html>\n"));
        assert!(
            page.contains("fill=\"white\""),
            "the dot theme reaches it: {page}"
        );
        let again = run(&dir, &["cruise", "-T", "x-dot-webpage", "src"])?;
        assert_eq!(out.stdout, again.stdout, "two runs differ");
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

fn fixture(name: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wrap-html")
        .join(name);
    Ok(std::fs::read(path)?)
}

#[test]
fn wrap_html_is_upstream_s_page_byte_for_byte() -> Result<(), Box<dyn Error>> {
    let input = fixture("input.svg")?;
    let expected = fixture("expected.html")?;
    let mut child = Command::new(BIN)
        .arg("wrap-html")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&input)?;
    }
    let out = child.wait_with_output()?;
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    assert_eq!(out.stdout, expected, "the page differs from upstream's");
    // The in-process dispatch writes the same page.
    let mut stdin = &input[..];
    let outcome = rb_cli::run_with_input(&["wrap-html".to_owned()], &mut stdin);
    assert_eq!(outcome.code, 0);
    assert_eq!(outcome.stdout.as_bytes(), expected.as_slice());
    // And an empty stream is the header and the footer.
    let mut nothing: &[u8] = &[];
    let empty = rb_cli::run_with_input(&["wrap-html".to_owned()], &mut nothing);
    assert!(empty.stdout.contains("    </div>\n    <script>\n"));
    Ok(())
}
