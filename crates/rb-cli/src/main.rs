//! The `rulebearing` binary: reads the arguments, calls [`rb_cli::run`], writes the outcome.
//!
//! - Architecture: [`docs/architecture.md#outputs-and-ci-contract`](../../../docs/architecture.md#outputs-and-ci-contract)
//! - Decision: [ADR-0008](../../../docs/adr/0008-exit-code-contract.md)
//! - Plan: [Wave 0, Step 2](../../../docs/plans/pending/0000-wave-0-spike.md#step-2-cargo-workspace-and-the-ten-crates-0a)

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = rb_cli::run_with_input(&args, &mut std::io::stdin());
    print!("{}", outcome.stdout);
    eprint!("{}", outcome.stderr);
    ExitCode::from(outcome.code)
}
