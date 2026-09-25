//! Prints the Spike B attribution report for one built solution as JSON.
//!
//! - Plan: [Wave 0, Step 9](../../../docs/plans/pending/0000-wave-0-spike.md#step-9-spike-b-rb-extract-dotnet-0d)
//!   (`examples/attribution.rs`, run by `conformance/archunitnet/scripts/spike-b-attribution.sh`)
//! - Decision: [ADR-0003](../../../docs/adr/0003-dotnet-extractor-fallback.md)
//!
//! Usage: `cargo run --release -p rb-extract-dotnet --example attribution -- --solution <path>
//! [--configuration Release] [--repository <root>]`. The repository defaults to the solution's
//! folder; it is what `/_/` in a deterministic build's document paths stands for.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let Some(solution) = value("--solution").map(PathBuf::from) else {
        eprintln!(
            "usage: attribution --solution <path> [--configuration Release] [--repository <root>]"
        );
        return ExitCode::from(2);
    };
    let configuration = value("--configuration").unwrap_or_else(|| "Release".to_owned());
    let repository = value("--repository")
        .map(PathBuf::from)
        .or_else(|| solution.parent().map(PathBuf::from))
        .unwrap_or_default();
    match rb_extract_dotnet::attribute_solution(&solution, &configuration, &repository) {
        Ok(report) => match serde_json::to_string_pretty(&report) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("attribution: {error}");
                ExitCode::from(2)
            }
        },
        Err(error) => {
            eprintln!("attribution: {error}");
            ExitCode::from(2)
        }
    }
}
