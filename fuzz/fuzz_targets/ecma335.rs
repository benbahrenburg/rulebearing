//! Fuzzes the whole .NET loader: PE image, metadata tables, signatures, IL bodies, custom
//! attribute values and cross-assembly naming. Arbitrary bytes must produce an error, never a
//! panic, an overflow or an unbounded allocation.
//!
//! - Plan: [Wave 2, Step 2](../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#22-step-2-the-metadata-il-and-pdb-readers-2a)
//!   (`fuzz/fuzz_targets/ecma335.rs`, run nightly)
//! - Architecture: [Security posture](../../docs/architecture.md#security-posture)
//! - Requirement: [NFR-SEC-01](../../docs/prd.md#nfr-sec-01)
//!
//! Seeded with `TestAssembly.dll`, the extraction fixture's `Sample.dll` and the crate's
//! `tests/fuzz-regressions/` inputs by `fuzz/run.sh`. Every `TypeSpec` is named too, and every
//! attribute kept undecoded is decoded again with every enum unknown, so a spec chain or a
//! nested attribute value must stay bounded.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rb_extract_dotnet::loader::{Loaded, decode_attribute};
use rb_extract_dotnet::names::{Generics, Universe};

fuzz_target!(|data: &[u8]| {
    if let Ok(loaded) = Loaded::read(data) {
        let universe = Universe::new(vec![&loaded]);
        for ty in &loaded.types {
            for method in &ty.methods {
                let _ = universe.sig_name(0, &method.sig.ret, Generics::default(), true);
                for param in &method.sig.params {
                    let _ = universe.sig_name(0, param, Generics::default(), false);
                }
            }
            if let Some(base) = ty.extends {
                let _ = universe.resolve(0, base);
                let _ = universe.token_name(0, base, Generics::default(), false);
            }
            for attribute in &ty.attributes {
                if let Some(undecoded) = &attribute.undecoded {
                    let _ = decode_attribute(&undecoded.value, &undecoded.params, &|_| None);
                }
            }
        }
        for spec in &loaded.type_specs {
            let _ = universe.sig_name(0, spec, Generics::default(), true);
        }
    }
});
