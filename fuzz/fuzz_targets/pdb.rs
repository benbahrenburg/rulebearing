//! Fuzzes the portable PDB reader: documents, every method's sequence points, the type documents
//! record and SourceLink. Arbitrary bytes must produce an error, never a panic.
//!
//! - Plan: [Wave 2, Step 2](../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#22-step-2-the-metadata-il-and-pdb-readers-2a)
//!   (`fuzz/fuzz_targets/pdb.rs`, run nightly)
//! - Architecture: [Security posture](../../docs/architecture.md#security-posture)
//! - Requirement: [NFR-SEC-01](../../docs/prd.md#nfr-sec-01)
//!
//! Seeded with `TestAssembly.pdb` and the extraction fixture's `Sample.pdb` by `fuzz/run.sh`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rb_extract_dotnet::pdb::PortablePdb;

fuzz_target!(|data: &[u8]| {
    if let Ok(pdb) = PortablePdb::parse(data) {
        let _ = pdb.documents();
        let _ = pdb.type_definition_documents();
        let _ = pdb.source_link();
        for method in 0..64 {
            let _ = pdb.sequence_points(method);
        }
    }
});
