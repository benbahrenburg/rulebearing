//! `babelConfig`: the `babel-plugin-module-resolver` aliases a Babel config declares, applied to
//! specifiers the way the plugin rewrites them before dependency-cruiser's acorn extractor reads
//! the transpiled source.
//!
//! - Plan: [Wave 1, Step 10](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-10-rb-extract-ts-to-100-and-the-option-set-1c)
//!   (`babelConfig` alias table)
//! - Source: [coverage § Options](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#options),
//!   row `babelConfig.fileName` ("`babel-plugin-module-resolver` aliases are read from the
//!   config"); [coverage § Extraction and resolution](../../../docs/artifacts/dependency-cruiser-18.2.0-coverage.md#extraction-and-resolution)
//! - Decision: [ADR-0012](../../../docs/adr/0012-oxc-for-typescript.md) (`oxc` parses the syntax
//!   Babel would have transformed, so only the aliases need reading)
//! - Specification: dependency-cruiser 18.2.0 `src/extract/transpile/babel-wrap.mjs` and
//!   `babel-plugin-module-resolver`'s `resolvePath` (`alias`, `cwd`)
//!
//! Only JSON configs are read (`.babelrc`, `.babelrc.json`, `babel.config.json`, and the `babel`
//! key of a `package.json`): a JavaScript config is a program, and this crate runs none. Such a
//! config is a named error, never a silent skip. Of the plugin's options only `alias` and `cwd`
//! apply; `root` and custom `resolvePath` functions are outside the coverage row.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::Value;

use crate::resolve::{self, is_relative};

/// A Babel config could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("babelConfig {path}: {reason}", path = path.display())]
pub struct BabelError {
    /// The config file.
    pub path: PathBuf,
    /// What is wrong and how to fix it.
    pub reason: String,
}

/// One `alias` entry, compiled.
#[derive(Debug, Clone)]
struct Alias {
    /// The key as a regular expression.
    pattern: Regex,
    /// The value, with `\1` style references when the key was a regular expression.
    substitute: String,
    /// Whether the key was written as a regular expression (`^...`).
    regex_key: bool,
}

/// The module-resolver aliases of one Babel config.
#[derive(Debug, Clone)]
pub struct BabelAliases {
    aliases: Vec<Alias>,
    /// The directory relative alias targets are taken against (the plugin's `cwd`).
    cwd: PathBuf,
}

impl BabelAliases {
    /// Reads the aliases from the Babel config at `path`. `cwd` is the working directory the
    /// plugin would default its own `cwd` to.
    ///
    /// # Errors
    /// When the file cannot be read, is not JSON, is a JavaScript config, or an alias key is not
    /// a valid regular expression.
    pub fn load(path: &Path, cwd: &Path) -> Result<Self, BabelError> {
        let fail = |reason: String| BabelError {
            path: path.to_path_buf(),
            reason,
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if [".js", ".cjs", ".mjs", ".ts", ".cts", ".mts"]
            .iter()
            .any(|e| name.ends_with(e))
        {
            return Err(fail(
                "a JavaScript Babel config is a program and is not evaluated; move the \
                 module-resolver options to babel.config.json or .babelrc"
                    .to_owned(),
            ));
        }
        let text = std::fs::read_to_string(path).map_err(|e| fail(e.to_string()))?;
        let mut json: Value =
            serde_json::from_str(&text).map_err(|e| fail(format!("not valid JSON: {e}")))?;
        if name == "package.json" {
            json = json.get("babel").cloned().unwrap_or(Value::Null);
        }
        let directory = path.parent().unwrap_or(Path::new("")).to_path_buf();
        let mut aliases = Vec::new();
        let mut plugin_cwd = cwd.to_path_buf();
        for options in module_resolver_options(&json) {
            if let Some(value) = options.get("cwd").and_then(Value::as_str) {
                plugin_cwd = match value {
                    "babelrc" | "packagejson" => directory.clone(),
                    other => cwd.join(other),
                };
            }
            for (key, value) in options
                .get("alias")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                // An array of targets: the plugin takes the first that resolves; the first is
                // taken here.
                let target = value
                    .as_str()
                    .or_else(|| value.as_array()?.first()?.as_str());
                let Some(target) = target else {
                    continue;
                };
                aliases.push(compile(key, target).map_err(fail)?);
            }
        }
        Ok(Self {
            aliases,
            cwd: plugin_cwd,
        })
    }

    /// Whether there are no aliases.
    pub fn is_empty(&self) -> bool {
        self.aliases.is_empty()
    }

    /// The specifier module-resolver writes in place of `specifier` in `file` (an absolute
    /// path), or `None` when no alias matches.
    pub fn rewrite(&self, specifier: &str, file: &Path) -> Option<String> {
        let aliased = self.aliases.iter().find_map(|alias| {
            let captures = alias.pattern.captures(specifier)?;
            Some(if alias.regex_key {
                let mut out = String::new();
                captures.expand(&regex_substitute(&alias.substitute), &mut out);
                out
            } else {
                format!(
                    "{}{}",
                    alias.substitute,
                    captures.get(1).map_or("", |m| m.as_str())
                )
            })
        })?;
        if !is_relative(&aliased) && !aliased.starts_with(".\\") && !aliased.starts_with("..\\") {
            return Some(aliased);
        }
        let from = file.parent().unwrap_or(Path::new(""));
        let relative = resolve::relative(from, &self.cwd.join(&aliased));
        Some(if relative.is_empty() {
            ".".to_owned()
        } else if is_relative(&relative) {
            relative
        } else {
            format!("./{relative}")
        })
    }
}

/// The options objects of every `module-resolver` entry in a config's `plugins`.
fn module_resolver_options(config: &Value) -> Vec<&serde_json::Map<String, Value>> {
    config
        .get("plugins")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|plugin| {
            let entry = plugin.as_array()?;
            let name = entry.first()?.as_str()?;
            matches!(name, "module-resolver" | "babel-plugin-module-resolver")
                .then(|| entry.get(1)?.as_object())
                .flatten()
        })
        .collect()
}

/// A key as module-resolver matches it: `^...` is a regular expression, `x$` matches `x` exactly,
/// anything else matches `x` and `x/...`.
fn compile(key: &str, target: &str) -> Result<Alias, String> {
    let (pattern, regex_key) = if key.starts_with('^') {
        (key.to_owned(), true)
    } else if let Some(exact) = key.strip_suffix('$') {
        (format!("^{}()$", regex::escape(exact)), false)
    } else {
        (format!("^{}(/.*|)$", regex::escape(key)), false)
    };
    let pattern = Regex::new(&pattern).map_err(|e| format!("alias `{key}`: {e}"))?;
    Ok(Alias {
        pattern,
        substitute: target.to_owned(),
        regex_key,
    })
}

/// module-resolver's `\1` group references, in the `regex` crate's `${1}` spelling.
fn regex_substitute(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek().is_some_and(char::is_ascii_digit) => {
                let mut digits = String::new();
                while let Some(d) = chars.peek().copied().filter(char::is_ascii_digit) {
                    digits.push(d);
                    chars.next();
                }
                let _ = write!(out, "${{{digits}}}");
            }
            '$' => out.push_str("$$"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aliases(json: &str) -> Result<BabelAliases, BabelError> {
        let directory = std::env::temp_dir().join(format!("rb-babel-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&directory);
        let path = directory.join(format!("{}.babelrc", json.len()));
        let _ = std::fs::write(&path, json);
        let loaded = BabelAliases::load(&path, Path::new("/repo"));
        let _ = std::fs::remove_file(&path);
        loaded
    }

    #[test]
    fn prefix_exact_and_regex_keys_rewrite_as_module_resolver_does() {
        let loaded = aliases(
            r#"{"plugins": [["module-resolver", {"alias": {
                "@app": "./src/app",
                "exact$": "./src/exact.js",
                "^#(.+)$": "./src/hash/\\1",
                "lib": "some-package"
            }}]]}"#,
        );
        let Ok(loaded) = loaded else {
            unreachable!("{loaded:?}");
        };
        let file = Path::new("/repo/src/app/x/index.js");
        assert_eq!(loaded.rewrite("@app/y", file).as_deref(), Some("../y"));
        assert_eq!(loaded.rewrite("@app", file).as_deref(), Some(".."));
        assert_eq!(loaded.rewrite("@apple", file), None);
        assert_eq!(
            loaded.rewrite("exact", Path::new("/repo/a.js")).as_deref(),
            Some("./src/exact.js")
        );
        assert_eq!(loaded.rewrite("exact/no", file), None);
        assert_eq!(
            loaded.rewrite("#z", Path::new("/repo/a.js")).as_deref(),
            Some("./src/hash/z")
        );
        assert_eq!(
            loaded.rewrite("lib/q", file).as_deref(),
            Some("some-package/q")
        );
        assert!(!loaded.is_empty());
    }

    #[test]
    fn a_babelrc_cwd_is_the_config_folder_and_arrays_take_their_first_target() {
        let loaded = aliases(
            r#"{"plugins": ["other", ["babel-plugin-module-resolver", {"cwd": "babelrc", "alias": {"~": ["./lib", "./other"], "n": 1}}]]}"#,
        );
        let Ok(loaded) = loaded else {
            unreachable!("{loaded:?}");
        };
        let directory = std::env::temp_dir().join(format!("rb-babel-{}", std::process::id()));
        assert_eq!(
            loaded.rewrite("~/a", &directory.join("main.js")).as_deref(),
            Some("./lib/a")
        );
    }

    #[test]
    fn unusable_configs_are_named_errors() {
        assert!(
            aliases("{ not json")
                .err()
                .is_some_and(|e| e.reason.contains("not valid JSON"))
        );
        assert!(
            aliases(r#"{"plugins": [["module-resolver", {"alias": {"^(": "x"}}]]}"#)
                .err()
                .is_some_and(|e| e.reason.contains("alias `^(`"))
        );
        let js = BabelAliases::load(Path::new("/nowhere/babel.config.js"), Path::new("/"));
        assert!(
            js.err()
                .is_some_and(|e| e.to_string().contains("babel.config.json"))
        );
        let missing = BabelAliases::load(Path::new("/nowhere/.babelrc"), Path::new("/"));
        assert!(missing.is_err());
        let empty = aliases("{}");
        assert!(empty.is_ok_and(|a| a.is_empty()));
    }

    #[test]
    fn group_references_are_translated() {
        assert_eq!(regex_substitute(r"./a/\1/\12$"), "./a/${1}/${12}$$");
    }
}
