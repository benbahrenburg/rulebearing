//! The warm graph: the cruise result `serve --mcp` and `serve --lsp` answer from, kept in memory
//! and read again only when it changes.
//!
//! - Plan: [Wave 3, Step 18](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#25-steps-for-sub-wave-3e-serve---mcp-and-serve---lsp),
//!   [§ MCP and LSP over one warm graph](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#mcp-and-lsp-over-one-warm-graph)
//! - Decision: [ADR-0021](../../../../docs/adr/0021-agent-surface-cli-first.md) (the servers are thin
//!   loops over the commands and the cached graph)
//! - Requirement: [FR-CLI-06](../../../../docs/prd.md#fr-cli-06)
//!
//! Two files can hold the graph: [`SAVED_GRAPH`], which `cruise -T json -f .graph/cruise.json`
//! writes and the query commands read, and [`GUARD_GRAPH`], which `guard --watch` rewrites after a
//! save changes the graph. [`WarmGraph::ensure_fresh`] takes the newer of the two and reads it again
//! only when its modification time or size moved, and notes a change to the configuration files
//! the last load read. Each change bumps the [`WarmGraph::generation`], so a server that derives
//! something from the graph (the LSP's diagnostics) knows when to derive it again. The LSP's own
//! re-check hands its result over with [`WarmGraph::replace`]. The holder reads; it never writes.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rb_model::GraphDocument;

use crate::cli::ConfigArgs;
use crate::cmd::guard::GUARD_GRAPH;
use crate::context::Context;
use crate::pipeline::SAVED_GRAPH;

/// A file's modification time in nanoseconds and its size: what says it changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Stamp {
    modified: u128,
    length: u64,
}

/// The stamp of `path`, or `None` when it cannot be read.
fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    Some(Stamp {
        modified,
        length: metadata.len(),
    })
}

/// Where the warm graph came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A file, relative to the working directory, with the stamp it had when read.
    File(String),
    /// A re-check the server ran itself ([`WarmGraph::replace`]).
    Recheck,
}

/// The graph a server holds.
#[derive(Debug)]
struct Held {
    source: Source,
    stamp: Option<Stamp>,
    text: String,
    document: GraphDocument,
}

/// The cruise result held in memory for a server, and when it last changed.
#[derive(Debug)]
pub struct WarmGraph {
    config: ConfigArgs,
    config_files: Vec<(PathBuf, Option<Stamp>)>,
    held: Option<Held>,
    generation: u64,
}

/// What [`WarmGraph::ensure_fresh`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// Nothing changed since the last call.
    Unchanged,
    /// The graph or the configuration changed; the generation moved.
    Changed,
    /// There is no graph to hold: neither file exists and nothing was handed over.
    Missing,
}

impl WarmGraph {
    /// A holder for the working directory of `ctx`, with the configuration `config` names; the
    /// first [`WarmGraph::ensure_fresh`] reads the graph.
    pub fn open(config: &ConfigArgs) -> Self {
        Self {
            config: config.clone(),
            config_files: Vec::new(),
            held: None,
            generation: 0,
        }
    }

    /// The configuration arguments the server was started with.
    pub fn config(&self) -> &ConfigArgs {
        &self.config
    }

    /// Bumped each time the graph or the configuration changed.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The graph's JSON, when one is held.
    pub fn text(&self) -> Option<&str> {
        self.held.as_ref().map(|h| h.text.as_str())
    }

    /// The graph, when one is held.
    pub fn document(&self) -> Option<&GraphDocument> {
        self.held.as_ref().map(|h| &h.document)
    }

    /// Where the held graph came from.
    pub fn source(&self) -> Option<&Source> {
        self.held.as_ref().map(|h| &h.source)
    }

    /// The configuration files the last load read, for [`WarmGraph::ensure_fresh`] to watch.
    pub fn watch_config(&mut self, files: &[PathBuf]) {
        let watched: Vec<(PathBuf, Option<Stamp>)> =
            files.iter().map(|f| (f.clone(), stamp(f))).collect();
        if watched != self.config_files {
            self.config_files = watched;
        }
    }

    /// Reads the graph again when its file changed, the newer of [`SAVED_GRAPH`] and
    /// [`GUARD_GRAPH`] winning, and notes a change to the watched configuration files.
    ///
    /// # Errors
    /// A message naming the file when the newest graph cannot be read or is not a cruise result.
    pub fn ensure_fresh(&mut self, ctx: &Context<'_>) -> Result<Freshness, String> {
        let mut changed = false;
        let config_now: Vec<(PathBuf, Option<Stamp>)> = self
            .config_files
            .iter()
            .map(|(f, _)| (f.clone(), stamp(f)))
            .collect();
        if config_now != self.config_files {
            self.config_files = config_now;
            changed = true;
        }
        let newest = [SAVED_GRAPH, GUARD_GRAPH]
            .into_iter()
            .filter_map(|name| stamp(&ctx.resolve(name)).map(|s| (s, name)))
            .max_by_key(|(s, _)| s.modified);
        match newest {
            Some((found, name)) => {
                let current = self.held.as_ref().is_some_and(|h| {
                    h.source == Source::File(name.to_owned()) && h.stamp == Some(found)
                });
                // A re-check the server ran is newer than any file older than it.
                let rechecked_later = self.held.as_ref().is_some_and(|h| {
                    h.source == Source::Recheck && h.stamp.is_some_and(|s| s >= found)
                });
                if !current && !rechecked_later {
                    self.load(ctx, name, found)?;
                    changed = true;
                }
            }
            None if self.held.is_none() => return Ok(Freshness::Missing),
            None => {}
        }
        if changed {
            self.generation += 1;
            return Ok(Freshness::Changed);
        }
        Ok(Freshness::Unchanged)
    }

    fn load(&mut self, ctx: &Context<'_>, name: &str, found: Stamp) -> Result<(), String> {
        let path = ctx.resolve(name);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read the graph {}: {e}", path.display()))?;
        let document = rb_ingest::dependency_cruiser::read(&text)
            .map_err(|e| format!("{} is not a cruise result: {e}", path.display()))?;
        self.held = Some(Held {
            source: Source::File(name.to_owned()),
            stamp: Some(found),
            text,
            document,
        });
        Ok(())
    }

    /// Holds the result of a re-check the server ran itself, newer than either file as it stands.
    pub fn replace(&mut self, ctx: &Context<'_>, document: GraphDocument) {
        let text = serde_json::to_string(&document).unwrap_or_default();
        let now = [SAVED_GRAPH, GUARD_GRAPH]
            .into_iter()
            .filter_map(|name| stamp(&ctx.resolve(name)))
            .max_by_key(|s| s.modified);
        self.held = Some(Held {
            source: Source::Recheck,
            stamp: now,
            text,
            document,
        });
        self.generation += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(modules: &[&str]) -> String {
        let modules: Vec<serde_json::Value> = modules
            .iter()
            .map(|m| {
                serde_json::json!({ "source": m, "dependencies": [], "dependents": [],
                    "orphan": false, "valid": true })
            })
            .collect();
        serde_json::json!({ "modules": modules, "summary": { "violations": [], "error": 0,
            "warn": 0, "info": 0, "ignore": 0, "totalCruised": modules.len(),
            "totalDependenciesCruised": 0, "optionsUsed": {} } })
        .to_string()
    }

    fn sources(warm: &WarmGraph) -> Vec<String> {
        warm.document()
            .map(|d| d.modules.iter().map(|m| m.source.clone()).collect())
            .unwrap_or_default()
    }

    /// Writes `text` and moves its modification time on, so a coarse file clock still tells the
    /// two writes apart.
    fn write_later(path: &Path, text: &str, offset: u64) {
        let _ = std::fs::write(path, text);
        if let Ok(file) = std::fs::File::options().write(true).open(path) {
            let _ = file.set_modified(
                std::time::SystemTime::now() + std::time::Duration::from_secs(offset),
            );
        }
    }

    #[test]
    fn the_newer_file_wins_and_only_a_change_reads_again() -> Result<(), String> {
        let dir = std::env::temp_dir().join(format!("rb-warm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(dir.join(".graph/guard"));
        let mut stdin: &[u8] = b"";
        let ctx = Context {
            cwd: dir.clone(),
            stdin: &mut stdin,
            today: chrono::NaiveDate::default(),
            timestamp: String::new(),
            color_terminal: false,
            warm: None,
        };
        let mut warm = WarmGraph::open(&ConfigArgs::default());
        assert_eq!(warm.ensure_fresh(&ctx)?, Freshness::Missing);
        assert_eq!(warm.generation(), 0);

        write_later(&dir.join(SAVED_GRAPH), &graph(&["a.ts"]), 0);
        assert_eq!(warm.ensure_fresh(&ctx)?, Freshness::Changed);
        assert_eq!(sources(&warm), ["a.ts"]);
        assert_eq!(warm.source(), Some(&Source::File(SAVED_GRAPH.to_owned())));
        assert_eq!(warm.ensure_fresh(&ctx)?, Freshness::Unchanged);
        assert_eq!(warm.generation(), 1);

        // The guard's document, written later, is the newer graph.
        write_later(&dir.join(GUARD_GRAPH), &graph(&["a.ts", "b.ts"]), 10);
        assert_eq!(warm.ensure_fresh(&ctx)?, Freshness::Changed);
        assert_eq!(sources(&warm), ["a.ts", "b.ts"]);
        assert_eq!(warm.generation(), 2);

        // A re-check the server ran stands until a file newer than it appears.
        let rechecked =
            rb_ingest::dependency_cruiser::read(&graph(&["c.ts"])).map_err(|e| e.to_string())?;
        warm.replace(&ctx, rechecked);
        assert_eq!(warm.generation(), 3);
        assert_eq!(warm.ensure_fresh(&ctx)?, Freshness::Unchanged);
        assert_eq!(sources(&warm), ["c.ts"]);
        assert!(warm.text().is_some_and(|t| t.contains("c.ts")));
        write_later(&dir.join(SAVED_GRAPH), &graph(&["d.ts"]), 20);
        assert_eq!(warm.ensure_fresh(&ctx)?, Freshness::Changed);
        assert_eq!(sources(&warm), ["d.ts"]);

        // A watched configuration file that changes moves the generation, the graph unchanged.
        let config = dir.join("rulebearing.yaml");
        write_later(&config, "rules: {}\n", 0);
        warm.watch_config(std::slice::from_ref(&config));
        let before = warm.generation();
        assert_eq!(warm.ensure_fresh(&ctx)?, Freshness::Unchanged);
        write_later(&config, "rules: { forbidden: [] }\n", 30);
        assert_eq!(warm.ensure_fresh(&ctx)?, Freshness::Changed);
        assert_eq!(warm.generation(), before + 1);

        // A graph that is not a cruise result is named.
        write_later(&dir.join(SAVED_GRAPH), "[1]", 40);
        let refused = warm.ensure_fresh(&ctx);
        assert!(
            refused
                .as_ref()
                .is_err_and(|e| e.contains("is not a cruise result")),
            "{refused:?}"
        );
        assert!(warm.config().config.is_none());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
