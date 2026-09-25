//! Fuzzes the data-format configuration front-ends (YAML, JSON, JSONC, TOML) and everything after
//! parsing: native mapping, shorthands, key checks, normalisation and pattern compilation.
//!
//! - Plan: [Wave 1 § 1.7](../../docs/plans/pending/0001-wave-1-typescript-parity.md#17-quality-attributes)
//!   ("fuzz targets for both config parsers")
//! - Requirement: [NFR-SEC-01](../../docs/prd.md#nfr-sec-01)
#![no_main]

use libfuzzer_sys::fuzz_target;
use rb_config::read::Syntax;
use rb_config::{ConfigFormat, LoadOptions, load_text};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let root = std::env::temp_dir().join("rb-fuzz-config-data");
    for syntax in [Syntax::Yaml, Syntax::Jsonc, Syntax::Toml] {
        for format in [ConfigFormat::Native, ConfigFormat::DependencyCruiser] {
            let options = LoadOptions {
                root: Some(root.clone()),
                format: Some(format),
                ..LoadOptions::default()
            };
            let _ = load_text(text, syntax, &root, &options);
        }
    }
});
