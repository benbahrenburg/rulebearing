//! `x-dot-webpage`: the `dot` output drawn as SVG by GraphViz' `dot` and wrapped in an HTML page
//! with upstream's stylesheet and highlighting script; and the page's header and footer, which
//! `rulebearing wrap-html` writes around a stream. dependency-cruiser 18.2.0's
//! `src/report/dot-webpage/dot-module.mjs` and `wrap-in-html.mjs`, ported; the stylesheet and
//! the script are upstream's, verbatim, in `svg_in_html/` with upstream's licence.
//!
//! - Specification: `test/report/dot-webpage/dot-module.spec.mjs`, run unmodified by conformance
//!   gate 1 layer 3, and upstream's reporter over every `test/report` mock
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Decision: [ADR-0053](../../../docs/adr/0053-x-dot-webpage-draws-with-graphviz-dot.md) (the
//!   one GraphViz spawn)
//! - Coverage: [coverage § Output types](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#output-types),
//!   row `x-dot-webpage`; [coverage § Command line](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line),
//!   row `depcruise-wrap-stream-in-html`
//! - Plan: [Wave 3, Step 6 and Step 8](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01), [FR-CLI-08](../../../docs/prd.md#fr-cli-08)
//!
//! This crate does not start processes. The reporter asks a [`Graphviz`] for the two calls
//! upstream makes (`dot -V`, then `dot -Tsvg` with the program on stdin) and decides from the
//! answers exactly as upstream does: `rb-cli` answers them by running `dot`, and the conformance
//! protocol answers them from the `spawnFunction` option upstream's specs pass.

use std::fmt;
use std::sync::Arc;

use serde_json::Value;

use crate::{Rendered, ReportError, dot};

/// The stylesheet, verbatim from dependency-cruiser 18.2.0.
pub const STYLESHEET: &str = include_str!("svg_in_html/style.css");
/// The highlighting script, verbatim from dependency-cruiser 18.2.0.
pub const SCRIPT: &str = include_str!("svg_in_html/script.cjs");

/// What a spawn of `dot` gave: the fields of Node's `spawnSync` result upstream reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Spawned {
    /// The exit status; `None` when the process did not run or was killed.
    pub status: Option<i32>,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// The error that kept the process from running, when there was one.
    pub error: Option<String>,
}

/// Runs GraphViz' `dot`.
pub trait Graphviz {
    /// `dot <args>`, with `input` on stdin when given.
    fn run(&self, args: &[&str], input: Option<&str>) -> Spawned;
}

/// A shared [`Graphviz`] for [`crate::ReportOptions`]; equal when it is the same runner.
#[derive(Clone)]
pub struct GraphvizRunner(pub Arc<dyn Graphviz + Send + Sync>);

impl fmt::Debug for GraphvizRunner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GraphvizRunner")
    }
}

impl PartialEq for GraphvizRunner {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::addr_eq(Arc::as_ptr(&self.0), Arc::as_ptr(&other.0))
    }
}

impl Eq for GraphvizRunner {}

/// Upstream's message when `dot` is missing or is not GraphViz' `dot`.
pub const NOT_AVAILABLE: &str = "GraphViz dot, which is required for the 'x-dot-webpage' reporter doesn't seem to be available on this system. See the GraphViz download page for instruction on how to get it on your system: https://www.graphviz.org/download/";

/// `getHeader(stylesheet)`: the page up to where the SVG goes.
pub fn header(stylesheet: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en" dir="ltr">
  <head>
    <meta charset="utf-8" />
    <title>dependency graph</title>
    <style>
      {stylesheet}
    </style>
  </head>
  <body>
    <button id="button_help">?</button>
    <div id="hints" class="hint" style="display: none">
      <button id="close-hints">x</button>
      <span id="hint-text"></span>
      <ul>
        <li><b>Hover</b> - highlight</li>
        <li><b>Right-click</b> - pin highlight</li>
        <li><b>ESC</b> - clear</li>
      </ul>
    </div>
"#
    )
}

/// `getFooter(script)`: the page after the SVG.
pub fn footer(script: &str) -> String {
    format!(
        "    <script>
      {script}
    </script>
  </body>
</html>
"
    )
}

/// `wrapInHTML(svg)`: the SVG between upstream's header and footer.
pub fn wrap_in_html(svg: &str) -> String {
    format!("{}{svg}{}", header(STYLESHEET), footer(SCRIPT))
}

/// `isAvailable()`: `dot -V` exits 0 and names GraphViz on stderr.
fn available(graphviz: &dyn Graphviz) -> bool {
    let answer = graphviz.run(&["-V"], None);
    answer.status == Some(0) && answer.stderr.starts_with("dot - graphviz version")
}

/// `convert(program)`: the SVG `dot -Tsvg` draws, or upstream's error.
fn convert(graphviz: &dyn Graphviz, program: &str) -> Result<String, ReportError> {
    let answer = graphviz.run(&["-Tsvg"], Some(program));
    if answer.status == Some(0) {
        return Ok(answer.stdout);
    }
    Err(ReportError::Graphviz(answer.error.unwrap_or_else(|| {
        format!(
            "GraphViz' dot returned an error (exit code {})",
            answer
                .status
                .map_or_else(|| "null".to_owned(), |s| s.to_string())
        )
    })))
}

/// Renders `x-dot-webpage`: the module-level `dot` output with `section` as its options, drawn
/// by `graphviz` and wrapped. Exits 0, as upstream's reporter does.
///
/// # Errors
/// [`ReportError::Graphviz`] when `dot` is missing, is not GraphViz', or fails, with upstream's
/// message.
pub fn render(
    result: &Value,
    section: Option<&Value>,
    graphviz: &dyn Graphviz,
) -> Result<Rendered, ReportError> {
    if !available(graphviz) {
        return Err(ReportError::Graphviz(NOT_AVAILABLE.into()));
    }
    let program = dot::render(result, dot::Granularity::Module, section).output;
    Ok(Rendered {
        output: wrap_in_html(&convert(graphviz, &program)?),
        exit_code: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    /// Answers `-V` and `-Tsvg` from fixed replies and records the program it was given.
    struct Stub {
        version: Spawned,
        convert: Spawned,
        program: Mutex<Option<String>>,
    }

    impl Graphviz for Stub {
        fn run(&self, args: &[&str], input: Option<&str>) -> Spawned {
            if args == ["-V"] {
                return self.version.clone();
            }
            if let Ok(mut program) = self.program.lock() {
                *program = input.map(str::to_owned);
            }
            self.convert.clone()
        }
    }

    fn graphviz() -> Spawned {
        Spawned {
            status: Some(0),
            stderr: "dot - graphviz version 1.2.3.4".into(),
            ..Spawned::default()
        }
    }

    fn stub(version: Spawned, convert: Spawned) -> Stub {
        Stub {
            version,
            convert,
            program: Mutex::new(None),
        }
    }

    fn minimal() -> Value {
        json!({ "modules": [], "summary": { "error": 0, "violations": [], "optionsUsed": {} } })
    }

    #[test]
    fn wraps_what_dot_draws() -> Result<(), ReportError> {
        let svg = Spawned {
            status: Some(0),
            stdout: "<svg></svg>".into(),
            ..Spawned::default()
        };
        let runner = stub(graphviz(), svg);
        let rendered = render(&minimal(), None, &runner)?;
        assert_eq!(rendered.exit_code, 0);
        assert_eq!(rendered.output, wrap_in_html("<svg></svg>"));
        assert!(
            rendered
                .output
                .starts_with("<!doctype html>\n<html lang=\"en\" dir=\"ltr\">\n")
        );
        assert!(
            rendered
                .output
                .contains("    </div>\n<svg></svg>    <script>\n      var gMode")
        );
        assert!(
            rendered
                .output
                .ends_with("\n    </script>\n  </body>\n</html>\n")
        );
        let program = runner
            .program
            .lock()
            .map(|p| p.clone().unwrap_or_default())
            .unwrap_or_default();
        assert_eq!(
            program,
            dot::render(&minimal(), dot::Granularity::Module, None).output
        );
        Ok(())
    }

    #[test]
    fn fails_as_upstream_does() {
        let missing = Spawned {
            status: Some(1),
            stderr: "error: command not found: dot".into(),
            ..Spawned::default()
        };
        let not_graphviz = Spawned {
            status: Some(0),
            stderr: "dot - sneaky template engine version 0.0.1".into(),
            ..Spawned::default()
        };
        for version in [missing, not_graphviz, Spawned::default()] {
            let runner = stub(version, Spawned::default());
            assert_eq!(
                render(&minimal(), None, &runner),
                Err(ReportError::Graphviz(NOT_AVAILABLE.into()))
            );
        }
        let error = Spawned {
            status: Some(1),
            error: Some("some error, doesn't really matter which".into()),
            ..Spawned::default()
        };
        let runner = stub(graphviz(), error);
        assert_eq!(
            render(&minimal(), None, &runner),
            Err(ReportError::Graphviz(
                "some error, doesn't really matter which".into()
            ))
        );
        let failed = Spawned {
            status: Some(42),
            ..Spawned::default()
        };
        let message = render(&minimal(), None, &stub(graphviz(), failed))
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            message.contains("GraphViz' dot returned an error (exit code 42)"),
            "{message}"
        );
        let killed = render(&minimal(), None, &stub(graphviz(), Spawned::default()));
        assert_eq!(
            killed,
            Err(ReportError::Graphviz(
                "GraphViz' dot returned an error (exit code null)".into()
            ))
        );
    }

    #[test]
    fn header_and_footer_hold_the_snippets() {
        let head = header(STYLESHEET);
        assert!(head.contains("<style>\n      ") && head.ends_with("    </div>\n"));
        assert!(footer(SCRIPT).starts_with("    <script>\n      "));
        let a: GraphvizRunner = GraphvizRunner(Arc::new(stub(graphviz(), Spawned::default())));
        let b = a.clone();
        assert_eq!(a, b);
        assert_ne!(
            a,
            GraphvizRunner(Arc::new(stub(graphviz(), Spawned::default())))
        );
        assert_eq!(format!("{a:?}"), "GraphvizRunner");
    }
}
