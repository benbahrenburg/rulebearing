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
//! beside `key.json`, the inputs the name hashes ([`key`]: the plan's worktree root, `HEAD` and
//! configuration hash, extended with the build's version and a working-tree fingerprint). A query
//! command given `--graph FILE` reads that file; otherwise it reads the entry for its key, and on
//! a miss extracts, writes the entry (to a temporary name, then renamed, so a reader never sees
//! half an entry) and answers. An entry that does not read back as a graph document is a miss, so
//! a truncated or foreign file is re-extracted rather than answered from. After a write, only the
//! newest [`KEEP`] entries of each worktree root are kept, so `.graph/cache` does not grow with
//! every edit. `--no-cache` extracts without reading or writing. The `--cache` option with a
//! strategy and compression is wave 3 and builds on this key.

pub mod key;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rb_config::Config;
use rb_model::GraphDocument;

use crate::context::Context;
use crate::pipeline;

pub use key::CacheKey;

/// The graph document inside an entry.
pub const GRAPH_FILE: &str = "graph.json";
/// The inputs of an entry's name, for a reader who wants to know what it holds.
pub const KEY_FILE: &str = "key.json";
/// How many entries of one worktree root are kept after a write.
pub const KEEP: usize = 8;

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
        && serde_json::from_str::<GraphDocument>(&text).is_ok()
    {
        return Ok(Graph {
            text,
            origin: Origin::Hit(directory),
        });
    }
    let text = extract()?;
    write_entry(&directory, &key, &text)?;
    prune(&ctx.cwd.join(key::CACHE_DIR), KEEP);
    Ok(Graph {
        text,
        origin: Origin::Written(directory),
    })
}

/// Writes an entry: the key, then the graph under a temporary name renamed into place.
fn write_entry(directory: &Path, key: &CacheKey, text: &str) -> Result<(), String> {
    std::fs::create_dir_all(directory)
        .map_err(|e| format!("cannot create {}: {e}", directory.display()))?;
    let mut key_text = serde_json::to_string_pretty(&key.to_json()).map_err(|e| e.to_string())?;
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

/// Removes all but the newest `keep` entries of each worktree root under `folder`, by the
/// modification time of their graph (an entry without one counts as oldest). An entry whose
/// `key.json` cannot be read is grouped on its own root, `""`. Removal is best effort: another
/// process may be reading or removing the same entry.
pub fn prune(folder: &Path, keep: usize) {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return;
    };
    let mut by_root: BTreeMap<String, Vec<(SystemTime, PathBuf)>> = BTreeMap::new();
    for entry in listing.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let root = std::fs::read_to_string(path.join(KEY_FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("root").and_then(|r| r.as_str()).map(str::to_owned))
            .unwrap_or_default();
        let modified = std::fs::metadata(path.join(GRAPH_FILE))
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        by_root.entry(root).or_default().push((modified, path));
    }
    for entries in by_root.values_mut() {
        // Newest first; the name breaks a tie so the choice does not depend on listing order.
        entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        for (_, path) in entries.iter().skip(keep) {
            let _ = std::fs::remove_dir_all(path);
        }
    }
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
        // Valid JSON that is not a graph document is a miss too, never an answer.
        for foreign in [
            "[1]",
            r#"{"modules":7}"#,
            r#"{"modules":[{"source":3}]}"#,
            "null",
        ] {
            let _ = std::fs::write(key.directory(&dir).join(GRAPH_FILE), foreign);
            assert!(graph(&ctx, &config, None, false).is_err(), "{foreign}");
        }
        let empty_graph = serde_json::to_string(&GraphDocument::default()).unwrap_or_default();
        let _ = std::fs::write(key.directory(&dir).join(GRAPH_FILE), empty_graph);
        let hit = graph(&ctx, &config, None, false);
        assert!(hit.is_ok_and(|g| g.origin == Origin::Hit(key.directory(&dir))));
        assert!(
            graph(&ctx, &config, None, true).is_err(),
            "--no-cache never reads"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pruning_keeps_the_newest_entries_of_each_root() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("rb-cache-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let base = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let entry = |name: &str, root: &str, age: u64| -> std::io::Result<()> {
            let folder = dir.join(name);
            std::fs::create_dir_all(&folder)?;
            std::fs::write(folder.join(KEY_FILE), format!("{{\"root\":\"{root}\"}}"))?;
            let graph = folder.join(GRAPH_FILE);
            std::fs::write(&graph, "{}")?;
            std::fs::File::options()
                .write(true)
                .open(&graph)?
                .set_modified(base + std::time::Duration::from_secs(age))
        };
        for n in 0..10u64 {
            entry(&format!("a{n:02}"), "/one", n)?;
        }
        for n in 0..3u64 {
            entry(&format!("b{n:02}"), "/two", n)?;
        }
        std::fs::create_dir_all(dir.join("stray"))?;
        std::fs::write(dir.join("loose-file"), "")?;
        prune(&dir, 8);
        let mut left: Vec<String> = std::fs::read_dir(&dir)?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "a02",
                "a03",
                "a04",
                "a05",
                "a06",
                "a07",
                "a08",
                "a09",
                "b00",
                "b01",
                "b02",
                "loose-file",
                "stray"
            ],
            "the two oldest of /one go; /two and the rootless entry are under the limit"
        );
        prune(&dir.join("absent"), 8);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
