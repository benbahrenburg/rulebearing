//! Writes `schema/v1.json` from the `rb-model` types.
//!
//! - Plan: [Wave 0, Step 3](../../../docs/plans/pending/0000-wave-0-spike.md#step-3-rb-model-graph-document-schema-violation-id-0a)
//!   item 6
//! - Decision: [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//!
//! Run `cargo run -p rb-model --example emit-schema`; the `schema-check` job runs it and fails if
//! the committed file changes.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join(rb_model::schema::PATH);
    match std::fs::write(&path, rb_model::schema::render()) {
        Ok(()) => {
            println!("wrote {}", rb_model::schema::PATH);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("could not write {}: {error}", path.display());
            ExitCode::FAILURE
        }
    }
}
