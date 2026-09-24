//! The worktree-aware graph cache the query commands answer from.
//!
//! - Architecture: [Agent surface](../../../../docs/architecture.md#agent-surface)
//! - Plan: [Wave 2, Step 13](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#213-step-13-worktree-aware-cache-and-the-eslint-plugin-2g)
//!   (the key; `can-import`, `propose`, `place` and `impact` read it; a miss re-extracts),
//!   [Step 12](../../../../docs/plans/pending/0002-wave-2-dotnet-python-element-rules.md#212-step-12-agent-subcommands-2g)
//!   ("`propose` and `place` read the worktree-aware cache ... so they answer in the same time
//!   as `can-import`")
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md)
//! - Requirement: [FR-CLI-05](../../../../docs/prd.md#fr-cli-05)
//!
//! An entry is `.graph/cache/<key>/graph.json`, the extracted graph document before evaluation,
//! beside `key.json`, the three inputs the name hashes ([`key`]). A query command given
//! `--graph FILE` reads that file; otherwise it reads the entry for its worktree, commit and
//! configuration, and on a miss extracts, writes the entry (to a temporary name, then renamed, so
//! a reader never sees half an entry) and answers. `--no-cache` extracts without reading or
//! writing. The `--cache` option with a strategy and compression is wave 3 and builds on this key.

pub mod key;

use std::path::PathBuf;

use rb_config::Config;
use rb_model::GraphDocument;
use serde_json::json;

use crate::context::Context;
use crate::pipeline;

pub use key::CacheKey;

/// The graph document inside an entry.
pub const GRAPH_FILE: &str = "graph.json";
/// The inputs of an entry's name, for a reader who wants to know what it holds.
pub const KEY_FILE: &str = "key.json";

/// Where a query command's graph came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// `--graph FILE`.
    File(String),
    /// The cache entry, already written.
    Hit(PathBuf),
    /// A fresh extraction, now written to the entry.
    Written(PathBuf),
    /// A fresh extraction, not cached (`--no-cache`).
    Extracted,
}

/// A graph as JSON text, with where it came from.
#[derive(Debug, Clone)]
pub struct Graph {
    /// The document's JSON.
    pub text: String,
    /// Where it came from.
    pub origin: Origin,
}

/// The graph a query command answers from: `file` when given, else the cache entry, else a fresh
/// extraction that is written to the entry unless `no_cache`.
///
/// # Errors
/// A message naming the file that cannot be read, or why the extraction cannot be trusted.
pub fn graph(
    ctx: &Context<'_>,
    config: &Config,
    file: Option<&str>,
    no_cache: bool,
) -> Result<Graph, String> {
    if let Some(file) = file {
        let path = ctx.resolve(file);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read the graph {}: {e}", path.display()))?;
        return Ok(Graph {
            text,
            origin: Origin::File(file.to_owned()),
        });
    }
    let extract = || -> Result<String, String> {
        let document = pipeline::extract(ctx, config, &[]).map_err(|e| e.to_string())?;
        serde_json::to_string(&document).map_err(|e| e.to_string())
    };
    if no_cache {
        return Ok(Graph {
            text: extract()?,
            origin: Origin::Extracted,
        });
    }
    let key = CacheKey::compute(&ctx.cwd, config);
    let directory = key.directory(&ctx.cwd);
    let cached = directory.join(GRAPH_FILE);
    if let Ok(text) = std::fs::read_to_string(&cached)
        && serde_json::from_str::<serde_json::Value>(&text).is_ok()
    {
        return Ok(Graph {
            text,
            origin: Origin::Hit(directory),
        });
    }
    let text = extract()?;
    write_entry(&directory, &key, &text)?;
    Ok(Graph {
        text,
        origin: Origin::Written(directory),
    })
}

/// Writes an entry: the key, then the graph under a temporary name renamed into place.
fn write_entry(directory: &std::path::Path, key: &CacheKey, text: &str) -> Result<(), String> {
    std::fs::create_dir_all(directory)
        .map_err(|e| format!("cannot create {}: {e}", directory.display()))?;
    let inputs = json!({ "root": key.root, "head": key.head, "configHash": key.config_hash });
    let mut key_text = serde_json::to_string_pretty(&inputs).map_err(|e| e.to_string())?;
    key_text.push('\n');
    let key_file = directory.join(KEY_FILE);
    std::fs::write(&key_file, key_text)
        .map_err(|e| format!("cannot write {}: {e}", key_file.display()))?;
    let temporary = directory.join(format!("{GRAPH_FILE}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, text)
        .map_err(|e| format!("cannot write {}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, directory.join(GRAPH_FILE))
        .map_err(|e| format!("cannot write {}: {e}", directory.join(GRAPH_FILE).display()))
}

/// The graph as a document ready to evaluate: read through [`graph`], annotations cleared.
///
/// # Errors
/// As [`graph`], or a message when the text is not a graph document.
pub fn document(
    ctx: &Context<'_>,
    config: &Config,
    file: Option<&str>,
    no_cache: bool,
) -> Result<GraphDocument, String> {
    let graph = graph(ctx, config, file, no_cache)?;
    let mut document = rb_ingest::dependency_cruiser::read(&graph.text).map_err(|e| {
        let name = match &graph.origin {
            Origin::File(f) => f.clone(),
            Origin::Hit(d) | Origin::Written(d) => d.join(GRAPH_FILE).display().to_string(),
            Origin::Extracted => "the extraction".to_owned(),
        };
        format!("{name} is not a cruise result: {e}")
    })?;
    pipeline::reset(&mut document);
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context<'a>(dir: &std::path::Path, stdin: &'a mut &'static [u8]) -> Context<'a> {
        Context {
            cwd: dir.to_path_buf(),
            stdin,
            today: chrono::NaiveDate::default(),
            timestamp: String::new(),
            color_terminal: false,
        }
    }

    #[test]
    fn a_named_file_is_read_and_a_missing_one_is_named() {
        let dir = std::env::temp_dir().join(format!("rb-cache-file-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("g.json"), r#"{"modules":[],"summary":{}}"#);
        let mut empty: &'static [u8] = &[];
        let ctx = context(&dir, &mut empty);
        let config = Config::default();
        let read = graph(&ctx, &config, Some("g.json"), false);
        assert!(read.is_ok_and(|g| g.origin == Origin::File("g.json".into())));
        let missing = graph(&ctx, &config, Some("nope.json"), false);
        assert!(missing.is_err_and(|m| m.contains("nope.json")));
        let _ = std::fs::write(dir.join("bad.json"), "[1]");
        assert!(
            document(&ctx, &config, Some("bad.json"), false).is_err_and(|m| m.contains("bad.json"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_entry_is_a_miss() {
        let dir = std::env::temp_dir().join(format!("rb-cache-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let config = Config::default();
        let key = CacheKey::compute(&dir, &config);
        let _ = std::fs::create_dir_all(key.directory(&dir));
        let _ = std::fs::write(key.directory(&dir).join(GRAPH_FILE), "{ half");
        let mut empty: &'static [u8] = &[];
        let ctx = context(&dir, &mut empty);
        // Nothing to extract in an empty folder, so the miss surfaces the extraction's reason.
        assert!(graph(&ctx, &config, None, false).is_err());
        let _ = std::fs::write(
            key.directory(&dir).join(GRAPH_FILE),
            r#"{"modules":[],"summary":{}}"#,
        );
        let hit = graph(&ctx, &config, None, false);
        assert!(hit.is_ok_and(|g| g.origin == Origin::Hit(key.directory(&dir))));
        assert!(
            graph(&ctx, &config, None, true).is_err(),
            "--no-cache never reads"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
