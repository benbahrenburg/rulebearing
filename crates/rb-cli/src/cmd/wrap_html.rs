//! `wrap-html`: dependency-cruiser's `depcruise-wrap-stream-in-html`. Standard input (an SVG,
//! typically `dot -Tsvg`'s) is written between the header and the footer of the page
//! `x-dot-webpage` writes, with its stylesheet and highlighting script.
//!
//! - Specification: dependency-cruiser 18.2.0's `bin/wrap-stream-in-html.mjs` and
//!   `src/cli/tools/wrap-stream-in-html.mjs`; byte-compared with upstream's output on
//!   `crates/rb-cli/tests/fixtures/wrap-html/`
//! - Coverage: [coverage § Command line](../../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#command-line),
//!   row `depcruise-wrap-stream-in-html`
//! - Plan: [Wave 3, Step 8](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//! - Requirement: [FR-CLI-08](../../../../docs/prd.md#fr-cli-08)
//!
//! Upstream streams rather than buffers, because the SVG can be large; so does the binary: its
//! `main` hands this command the process's stdin and stdout ([`crate::run_streaming`]), and the
//! bytes are copied through as they arrive, unchanged. [`run`], the path the in-process dispatch
//! takes, collects the page into the outcome instead.

use std::io::{Read, Write};

use clap::Args;
use rb_report::dot_webpage::{SCRIPT, STYLESHEET, footer, header};

use crate::{Context, Outcome, RunExit};

/// `wrap-html` takes no arguments; the SVG comes on stdin.
#[derive(Debug, Clone, Default, Args)]
pub struct WrapHtmlArgs {}

/// Writes the header, then everything `input` holds, then the footer, to `output`.
///
/// # Errors
/// The I/O error that stopped reading or writing.
pub fn stream(input: &mut dyn Read, output: &mut dyn Write) -> std::io::Result<()> {
    output.write_all(header(STYLESHEET).as_bytes())?;
    std::io::copy(input, output)?;
    output.write_all(footer(SCRIPT).as_bytes())?;
    output.flush()
}

/// The message for an I/O error while wrapping.
pub fn failure(error: &std::io::Error) -> String {
    format!("rulebearing wrap-html: cannot copy standard input to standard output: {error}\n")
}

/// Runs `wrap-html` in `ctx`, collecting the page. The page is text when the input is; bytes
/// that are not UTF-8 are replaced here, and copied unchanged by the streaming path.
pub fn run(ctx: &mut Context<'_>, _args: &WrapHtmlArgs) -> Outcome {
    let mut page = Vec::new();
    match stream(ctx.stdin, &mut page) {
        Ok(()) => Outcome::printed(String::from_utf8_lossy(&page).into_owned()),
        Err(e) => Outcome::failed(RunExit::Untrustworthy, failure(&e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader that fails after its first read.
    struct Broken(bool);

    impl Read for Broken {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.0 {
                return Err(std::io::Error::other("the pipe broke"));
            }
            self.0 = true;
            buf[..4].copy_from_slice(b"<svg");
            Ok(4)
        }
    }

    #[test]
    fn the_stream_goes_between_the_header_and_the_footer() {
        let mut out = Vec::new();
        let result = stream(&mut &b"<svg>\xff</svg>"[..], &mut out);
        assert!(result.is_ok());
        let mut expected = header(STYLESHEET).into_bytes();
        expected.extend_from_slice(b"<svg>\xff</svg>");
        expected.extend_from_slice(footer(SCRIPT).as_bytes());
        assert_eq!(out, expected, "bytes are copied unchanged");
        let mut out = Vec::new();
        assert!(stream(&mut Broken(false), &mut out).is_err());
        assert!(
            out.ends_with(b"<svg"),
            "what was read is written before the error"
        );
        let message = failure(&std::io::Error::other("x"));
        assert!(message.starts_with("rulebearing wrap-html: ") && message.ends_with("x\n"));
    }
}
