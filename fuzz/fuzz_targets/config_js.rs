//! Fuzzes the QuickJS configuration sandbox: arbitrary source must produce a configuration or a
//! named error, never a panic, a hang past the time limit or an escape.
//!
//! - Plan: [Wave 1, Step 2](../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-2-the-quickjs-evaluator-1a)
//!   (fuzz target `config_js`)
//! - Decision: [ADR-0006](../../docs/adr/0006-embedded-quickjs-config-evaluator.md)
//! - Requirement: [NFR-SEC-01](../../docs/prd.md#nfr-sec-01)
#![no_main]

use std::time::Duration;

use libfuzzer_sys::fuzz_target;
use rb_config::js::{Kind, Limits, evaluate_text};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let limits = Limits {
        time: Duration::from_millis(250),
        memory: 32 * 1024 * 1024,
    };
    let root = std::env::temp_dir().join("rb-fuzz-config-js");
    for kind in [Kind::CommonJs, Kind::Module, Kind::Json5] {
        let _ = evaluate_text(&root.join("c.cjs"), text, kind, &root, limits);
    }
});
