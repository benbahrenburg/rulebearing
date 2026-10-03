//! Fuzzes the `--cache` entry reader: the manifest, and the stored extraction it names, plain and
//! compressed. Neither may make a run panic; each is either an entry or a named miss.
//!
//! - Plan: [Wave 3 § 1.7](../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#17-quality-attributes)
//!   ("fuzz target for the cache manifest")
//! - Requirement: [NFR-SEC-01](../../docs/prd.md#nfr-sec-01)
#![no_main]

use std::collections::{BTreeMap, BTreeSet};

use libfuzzer_sys::fuzz_target;
use rb_cli::cache::manifest::{self, Key, Manifest, digest, extraction_file, load};
use rb_model::{CacheOptions, CacheStrategy};

fuzz_target!(|data: &[u8]| {
    let (manifest_bytes, extraction) = data.split_at(data.len() / 2);
    let dir = std::env::temp_dir().join(format!("rb-fuzz-cache-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let key = Key {
        tool_version: "v".into(),
        config_hash: "sha256:c".into(),
        worktree: "/w".into(),
        head: None,
    };
    let named = digest(extraction);
    for compress in [false, true] {
        let _ = std::fs::write(dir.join(extraction_file(&named, compress)), extraction);
    }
    // The manifest as given, then a well-formed one naming the given extraction.
    let _ = std::fs::write(dir.join(manifest::MANIFEST_FILE), manifest_bytes);
    let written = Manifest {
        tool_version: key.tool_version.clone(),
        config_hash: key.config_hash.clone(),
        worktree: key.worktree.clone(),
        head: None,
        strategy: CacheStrategy::Metadata,
        inputs: BTreeMap::new(),
        stamps: BTreeMap::new(),
        probes: BTreeMap::new(),
        watched: BTreeSet::new(),
        extraction: named,
    };
    for step in 0..2 {
        if step == 1 {
            let _ = manifest::store_manifest(&dir, &written);
        }
        for compress in [false, true] {
            let options = CacheOptions {
                compress: Some(compress),
                ..CacheOptions::in_folder("x")
            };
            if let Ok(entry) = load(&dir, &key, &options) {
                let _ = entry.with_states();
            }
        }
    }
});
