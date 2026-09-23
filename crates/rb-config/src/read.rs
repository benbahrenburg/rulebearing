//! Reading one configuration file into JSON, whatever its format.
//!
//! - Decision: [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md)
//! - Plan: [Wave 1, Step 1](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-1-config-model-and-the-two-front-ends-1a)
//! - Requirements: [FR-CFG-01](../../../docs/prd.md#fr-cfg-01), [FR-CFG-02](../../../docs/prd.md#fr-cfg-02)
//!
//! | Extension | Reader |
//! | --- | --- |
//! | `.yaml`, `.yml` | `serde_yaml` |
//! | `.json` | `serde_json`, falling back to JSON5 (dependency-cruiser reads JSON with `json5`) |
//! | `.jsonc` | comments and trailing commas removed, then `serde_json` |
//! | `.toml` | `toml` |
//! | `.js`, `.cjs`, `.mjs` | the QuickJS sandbox, or Node with `--config-via-node` |

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::ConfigError;
use crate::js::{self, Kind, Limits};

/// The syntax a file is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syntax {
    /// YAML.
    Yaml,
    /// JSON, or JSON5.
    Json,
    /// JSON with comments.
    Jsonc,
    /// TOML.
    Toml,
    /// JavaScript, evaluated.
    JavaScript,
}

impl Syntax {
    /// The syntax a file name implies.
    pub fn of(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "yaml" | "yml" => Some(Self::Yaml),
            "json" | "json5" => Some(Self::Json),
            "jsonc" => Some(Self::Jsonc),
            "toml" => Some(Self::Toml),
            "js" | "cjs" | "mjs" => Some(Self::JavaScript),
            _ => None,
        }
    }
}

/// How JavaScript is evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Evaluation {
    /// Evaluate with a local Node instead of the sandbox.
    pub via_node: bool,
    /// Sandbox limits.
    pub limits: Limits,
}

/// A file read into JSON, with every file the read touched.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    /// The value.
    pub value: Value,
    /// The files read, the entry included.
    pub files: Vec<PathBuf>,
}

/// Reads `path` as `syntax`.
///
/// # Errors
/// [`ConfigError::Read`], [`ConfigError::Parse`] or [`ConfigError::Js`].
pub fn read_file(
    path: &Path,
    syntax: Syntax,
    root: &Path,
    evaluation: Evaluation,
) -> Result<Read, ConfigError> {
    if syntax == Syntax::JavaScript {
        if evaluation.via_node {
            let value = js::via_node::evaluate(path)?;
            return Ok(Read {
                value,
                files: vec![path.to_path_buf()],
            });
        }
        let evaluated = js::evaluate(path, root, evaluation.limits)?;
        return Ok(Read {
            value: evaluated.value,
            files: evaluated.files,
        });
    }
    let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Read {
        file: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    let value = parse_text(&text, syntax, path, root, evaluation.limits)?;
    Ok(Read {
        value,
        files: vec![path.to_path_buf()],
    })
}

/// Parses text of a data syntax (`--config -` reads stdin through this).
///
/// # Errors
/// [`ConfigError::Parse`], or [`ConfigError::Js`] for JavaScript and JSON5.
pub fn parse_text(
    text: &str,
    syntax: Syntax,
    name: &Path,
    root: &Path,
    limits: Limits,
) -> Result<Value, ConfigError> {
    let parse = |reason: String| ConfigError::Parse {
        file: name.to_path_buf(),
        reason,
    };
    match syntax {
        Syntax::Yaml => {
            if text.trim().is_empty() {
                return Ok(Value::Object(serde_json::Map::new()));
            }
            serde_yaml::from_str(text).map_err(|e| parse(e.to_string()))
        }
        Syntax::Json => match serde_json::from_str(text) {
            Ok(value) => Ok(value),
            // dependency-cruiser parses JSON configs with json5: comments, trailing commas,
            // unquoted keys. JSON5 is a subset of a JavaScript expression.
            Err(strict) => js::evaluate_text(name, text, Kind::Json5, root, limits)
                .map(|e| e.value)
                .map_err(|_| parse(strict.to_string())),
        },
        Syntax::Jsonc => serde_json::from_str(&strip_jsonc(text)).map_err(|e| parse(e.to_string())),
        Syntax::Toml => toml::from_str(text).map_err(|e| parse(e.to_string())),
        Syntax::JavaScript => {
            let kind = js::kind_of(name, text, root);
            js::evaluate_text(name, text, kind, root, limits)
                .map(|e| e.value)
                .map_err(ConfigError::from)
        }
    }
}

/// Removes `//` and `/* */` comments and trailing commas from JSON-with-comments text, leaving
/// strings untouched.
pub fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '"' => {
                out.push(c);
                i += 1;
                while i < chars.len() {
                    out.push(chars[i]);
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        out.push(chars[i + 1]);
                        i += 2;
                        continue;
                    }
                    i += 1;
                    if chars[i - 1] == '"' {
                        break;
                    }
                }
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i += 2;
            }
            ',' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if !matches!(chars.get(j), Some('}' | ']')) {
                    out.push(c);
                }
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str, syntax: Syntax) -> Result<Value, ConfigError> {
        parse_text(
            text,
            syntax,
            Path::new("/r/c"),
            Path::new("/r"),
            Limits::default(),
        )
    }

    #[test]
    fn every_data_syntax_reads() -> Result<(), ConfigError> {
        assert_eq!(parse("a: 1\nb: [x]", Syntax::Yaml)?["b"][0], "x");
        assert_eq!(parse("", Syntax::Yaml)?, serde_json::json!({}));
        assert_eq!(parse(r#"{"a": 1}"#, Syntax::Json)?["a"], 1);
        assert_eq!(parse("{a: 1, /* c */ b: 'x',}", Syntax::Json)?["b"], "x");
        assert_eq!(
            parse("{ // c\n \"a\": \"//not\", \"b\": [1,], }", Syntax::Jsonc)?["a"],
            "//not"
        );
        assert_eq!(parse("a = 1\n[b]\nc = \"x\"", Syntax::Toml)?["b"]["c"], "x");
        assert_eq!(
            parse("module.exports = { a: 2 };", Syntax::JavaScript)?["a"],
            2
        );
        Ok(())
    }

    #[test]
    fn parse_errors_name_the_file() {
        for (text, syntax) in [
            ("a: [", Syntax::Yaml),
            ("{", Syntax::Json),
            ("{", Syntax::Jsonc),
            ("a =", Syntax::Toml),
        ] {
            let error = parse(text, syntax)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(error.contains("/r/c"), "{syntax:?}: {error}");
        }
    }

    #[test]
    fn jsonc_keeps_escaped_quotes() {
        assert_eq!(
            strip_jsonc(r#"{"a": "x\"/*y*/", /* z */ "b": 1}"#),
            r#"{"a": "x\"/*y*/",  "b": 1}"#
        );
    }

    #[test]
    fn syntax_follows_the_extension() {
        assert_eq!(Syntax::of(Path::new("a.yml")), Some(Syntax::Yaml));
        assert_eq!(Syntax::of(Path::new("a.jsonc")), Some(Syntax::Jsonc));
        assert_eq!(Syntax::of(Path::new("a.toml")), Some(Syntax::Toml));
        assert_eq!(Syntax::of(Path::new("a.mjs")), Some(Syntax::JavaScript));
        assert_eq!(Syntax::of(Path::new("a.json5")), Some(Syntax::Json));
        assert_eq!(Syntax::of(Path::new("a.ts")), None);
        assert_eq!(Syntax::of(Path::new("a")), None);
    }

    #[test]
    fn a_missing_file_is_a_read_error() {
        let error = read_file(
            Path::new("/definitely/not/here.yaml"),
            Syntax::Yaml,
            Path::new("/"),
            Evaluation::default(),
        );
        assert!(matches!(error, Err(ConfigError::Read { .. })));
    }
}
