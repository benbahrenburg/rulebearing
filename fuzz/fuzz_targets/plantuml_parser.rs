//! Fuzzes the `PlantUML` diagram parser `adhereTo` reads (`rb_rules::plantuml`): parsing,
//! associating the components (the stereotypes compiled as patterns), and placing a namespace.
//! A malformed `.puml` must be a named `DiagramError`, never a panic.
//!
//! - Plan: [Wave 3 § 1.7](../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#17-quality-attributes)
//!   ("fuzz target for the cache manifest and the PlantUML parser")
//! - Requirement: [NFR-SEC-01](../../docs/prd.md#nfr-sec-01)
#![no_main]

use libfuzzer_sys::fuzz_target;
use rb_rules::plantuml::{Association, parse};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(diagram) = parse(text) else {
        return;
    };
    let names: Vec<String> = diagram.components.iter().map(|c| c.name.clone()).collect();
    let Ok(association) = Association::new(diagram) else {
        return;
    };
    for name in names
        .iter()
        .map(String::as_str)
        .chain(["", "A.B", "System"])
    {
        let _ = association.contains(name);
        let _ = association.namespace_identifiers_of("T", name);
        let _ = association.target_namespace_identifiers("T", name);
    }
});
