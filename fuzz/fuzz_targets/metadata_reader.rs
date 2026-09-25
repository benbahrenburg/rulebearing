//! Fuzzes the ECMA-335 assembly reader and the portable PDB reader: arbitrary bytes must produce
//! an error, never a panic, an overflow or an unbounded allocation.
//!
//! - Plan: [Wave 0, Step 9](../../docs/plans/pending/0000-wave-0-spike.md#step-9-spike-b-rb-extract-dotnet-0d)
//!   (the fuzz target, ten clean minutes)
//! - Architecture: [Security posture](../../docs/architecture.md#security-posture)
//! - Requirement: [NFR-SEC-01](../../docs/prd.md#nfr-sec-01)
//!
//! The seed corpus is `conformance/archunitnet/fixtures/`: `fuzz/run.sh` copies the committed
//! `TestAssembly.dll` and `.pdb` into `corpus/metadata_reader/` before running.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rb_extract_dotnet::assembly::Assembly;
use rb_extract_dotnet::pdb::PortablePdb;

fuzz_target!(|data: &[u8]| {
    let _ = Assembly::read(data);
    if let Ok(pdb) = PortablePdb::parse(data) {
        let _ = pdb.documents();
        let _ = pdb.type_definition_documents();
        for method in 0..32 {
            let _ = pdb.first_point(method);
        }
    }
});
