//! Times one extraction over a directory: the first synthetic benchmark of NFR-PERF-01.
//!
//! - Plan: [Wave 1, Step 20](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-20-performance-measurement-for-nfr-perf-01-1g-started-in-1c)
//!   item 2 (the synthetic tree from `testbeds/synth/gen.mjs`), started in
//!   [Wave 1C](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#wave-1c-rb-extract-ts-completion)
//! - Requirement: [NFR-PERF-01](../../../docs/prd.md#nfr-perf-01)
//! - Architecture: [Performance model](../../../docs/architecture.md#performance-model)
//!
//! ```sh
//! node testbeds/synth/gen.mjs /tmp/synth
//! cargo run --release -p rb-extract-ts --example extract-timing -- /tmp/synth
//! cargo run --release -p rb-extract-ts --example extract-timing -- /tmp/synth --ts-pre-compilation-deps
//! ```
//!
//! The directory is the base directory; its `apps` and `packages` folders are the roots when they
//! exist, the directory itself otherwise, and its `tsconfig.json` is the `tsConfig` when present.
//! Prints the module count, the edge count and the elapsed milliseconds.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use rb_extract_ts::{extract_with, prepare};
use rb_model::TypeScriptOptions;
use rb_model::options::{FileReference, TsPreCompilationDeps};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(directory) = args.next().map(PathBuf::from) else {
        eprintln!("usage: extract-timing <directory> [--ts-pre-compilation-deps]");
        return ExitCode::from(2);
    };
    let pre_compilation = args.any(|a| a == "--ts-pre-compilation-deps");
    let directory = directory.canonicalize().unwrap_or(directory);
    let tsconfig = directory.join("tsconfig.json");
    let options = TypeScriptOptions {
        ts_config: tsconfig.is_file().then(|| FileReference {
            file_name: Some(tsconfig.to_string_lossy().into_owned()),
        }),
        ts_pre_compilation_deps: pre_compilation.then_some(TsPreCompilationDeps::Enabled(true)),
        ..TypeScriptOptions::default()
    };
    let mut roots: Vec<PathBuf> = ["apps", "packages"]
        .iter()
        .filter(|r| directory.join(r).is_dir())
        .map(PathBuf::from)
        .collect();
    if roots.is_empty() {
        roots.push(PathBuf::from("."));
    }
    let started = Instant::now();
    let result = prepare(&options, &directory).and_then(|(s, c)| extract_with(&roots, &s, &c));
    let elapsed = started.elapsed();
    match result {
        Ok(extraction) => {
            let edges: usize = extraction
                .modules
                .iter()
                .map(|m| m.dependencies.len())
                .sum();
            let unresolved = extraction
                .modules
                .iter()
                .flat_map(|m| &m.dependencies)
                .filter(|d| d.could_not_resolve)
                .count();
            println!(
                "extract-timing: modules={} edges={edges} unresolved={unresolved} ms={} threads={} tsPreCompilationDeps={pre_compilation}",
                extraction.modules.len(),
                elapsed.as_millis(),
                rayon::current_num_threads(),
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("extract-timing: {error}");
            ExitCode::from(2)
        }
    }
}
