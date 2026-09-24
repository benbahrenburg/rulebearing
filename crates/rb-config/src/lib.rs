//! `rb-config`: both configuration formats into one model.
//!
//! - Architecture: [`docs/architecture.md#configuration-and-the-rule-language`](../../../docs/architecture.md#configuration-and-the-rule-language)
//! - Decisions: [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md),
//!   [ADR-0006](../../../docs/adr/0006-embedded-quickjs-config-evaluator.md),
//!   [ADR-0016](../../../docs/adr/0016-linear-time-regex-and-strict-compat.md),
//!   [ADR-0027](../../../docs/adr/0027-pure-path-and-url-modules-in-the-config-sandbox.md)
//! - Plan: [Wave 1, sub-wave 1A](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#wave-1a-rb-config)
//! - Requirements: [FR-CFG-01](../../../docs/prd.md#fr-cfg-01) to [FR-CFG-07](../../../docs/prd.md#fr-cfg-07),
//!   [FR-RULE-07](../../../docs/prd.md#fr-rule-07), [FR-RULE-10](../../../docs/prd.md#fr-rule-10)
//! - Source: [design § Configuration](../../../docs/artifacts/design.md#configuration-a-native-format-and-dependency-cruisers-as-it-is)
//!
//! | Module | Does |
//! | --- | --- |
//! | [`model`] | the configuration model both formats load into |
//! | [`capability`] | which language answers each element predicate, and how |
//! | [`mod@load`] | one file in, one [`Config`] out |
//! | [`read`] | YAML, JSON, JSON5, JSONC, TOML and JavaScript into JSON |
//! | [`js`] | the sandboxed QuickJS evaluator and `--config-via-node` |
//! | [`native`] | the native shape onto dependency-cruiser's and back |
//! | [`extends`] | `extends` resolution and dependency-cruiser's merge |
//! | [`defines`] | `defines` and `${name}` substitution |
//! | [`shorthands`] | `layers` and `independence` |
//! | [`normalize`] | key checks and dependency-cruiser's rule-set normalisation |
//! | [`pattern`] | JavaScript patterns on the linear-time engine; the compatibility table |
//! | [`convert`] | `config convert` and `config expand` |
//! | [`lint`] | `config lint` |
//! | [`schema`] | the native format's JSON schema, `schema/config-v1.json` |
//! | [`webpack`] | `webpackConfig`'s `resolve` block, evaluated in the sandbox |

pub mod capability;
pub mod convert;
pub mod defines;
pub mod elements;
pub mod extends;
pub mod js;
pub mod lint;
pub mod load;
pub mod model;
pub mod native;
pub mod normalize;
pub mod pattern;
pub mod read;
pub mod schema;
pub mod shorthands;
pub mod webpack;

use std::path::PathBuf;

use thiserror::Error;

pub use load::{LoadOptions, load, load_text};
pub use model::{CompatMode, Config, ConfigWarning, Family, Rule, RuleMeta};

/// Errors a configuration front-end can raise. Every one is exit 3 in the CLI
/// ([ADR-0008](../../../docs/adr/0008-exit-code-contract.md)).
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The configuration does not have the shape the schema requires.
    #[error("invalid configuration: {0}")]
    Invalid(String),
    /// A predicate names a concept the language lacks
    /// ([ADR-0014](../../../docs/adr/0014-no-invented-cross-language-edges.md)).
    #[error("rule `{rule}`: predicate `{predicate}` is not answerable for {language}")]
    Unanswerable {
        /// The rule name.
        rule: String,
        /// The predicate key.
        predicate: String,
        /// The language it was applied to.
        language: String,
    },
    /// The file could not be read.
    #[error("cannot read {file}: {reason}", file = file.display())]
    Read {
        /// The file.
        file: PathBuf,
        /// Why.
        reason: String,
    },
    /// The file is not valid YAML, JSON, JSONC or TOML.
    #[error("{file} does not parse: {reason}", file = file.display())]
    Parse {
        /// The file.
        file: PathBuf,
        /// What the parser said.
        reason: String,
    },
    /// A JavaScript configuration could not be evaluated.
    #[error(transparent)]
    Js(#[from] js::JsError),
    /// An `extends` entry could not be resolved, or the chain is circular.
    #[error("extends `{spec}`: {reason}")]
    Extends {
        /// The entry as written.
        spec: String,
        /// Why.
        reason: String,
    },
    /// A `defines` entry could not be evaluated, or a `${name}` is undefined.
    #[error("defines `{name}`: {reason}")]
    Define {
        /// The define.
        name: String,
        /// Why.
        reason: String,
    },
    /// A pattern the linear-time engine cannot run, or that does not parse.
    #[error("rule `{rule}`, {at}: {source}")]
    Pattern {
        /// The rule.
        rule: String,
        /// Where in the rule.
        at: String,
        /// The pattern error.
        source: pattern::PatternError,
    },
    /// `--strict-compat` refused something dependency-cruiser would refuse.
    #[error("--strict-compat: {rule}{sep}{message}", sep = if rule.is_empty() { "" } else { ": " })]
    Strict {
        /// The rule, or empty for the file as a whole.
        rule: String,
        /// What dependency-cruiser would refuse.
        message: String,
    },
    /// A key a later wave implements.
    #[error(
        "`{key}` is delivered in wave {wave} and cannot be evaluated yet; remove it or run a release that supports it"
    )]
    NotYetSupported {
        /// The key.
        key: String,
        /// The wave.
        wave: u8,
    },
    /// `--require-comment-token` and a rule whose comment carries no decision token.
    #[error(
        "rule `{rule}` has no decision token in its comment; add `adr:NNNN` or `plan:<slug>` (--require-comment-token)"
    )]
    MissingToken {
        /// The rule.
        rule: String,
    },
}

/// Which front-end read a configuration file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// `rulebearing.{yaml,json,jsonc,toml}`
    Native,
    /// `.dependency-cruiser.{json,yaml,yml,cjs,js,mjs}`
    DependencyCruiser,
}

impl ConfigFormat {
    /// Detects the format from a file name, as
    /// [design § The dependency-cruiser format](../../../docs/artifacts/design.md#the-dependency-cruiser-format)
    /// specifies. `None` means the name is not a configuration file the tool recognises.
    pub fn detect(file_name: &str) -> Option<Self> {
        let name = file_name.rsplit('/').next().unwrap_or(file_name);
        if name.starts_with(".dependency-cruiser.") {
            let ext = name.rsplit('.').next().unwrap_or("");
            return matches!(ext, "json" | "yaml" | "yml" | "cjs" | "js" | "mjs")
                .then_some(Self::DependencyCruiser);
        }
        if name.starts_with("rulebearing.") {
            let ext = name.rsplit('.').next().unwrap_or("");
            return matches!(ext, "yaml" | "yml" | "json" | "jsonc" | "toml")
                .then_some(Self::Native);
        }
        None
    }

    /// Parses `--config-format`.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "native" | "rulebearing" => Some(Self::Native),
            "dependency-cruiser" | "dc" => Some(Self::DependencyCruiser),
            _ => None,
        }
    }
}

/// The configuration file names searched for when `--config` is not given, in order.
pub const DEFAULT_NAMES: &[&str] = &[
    "rulebearing.yaml",
    "rulebearing.yml",
    "rulebearing.json",
    "rulebearing.jsonc",
    "rulebearing.toml",
    ".dependency-cruiser.js",
    ".dependency-cruiser.cjs",
    ".dependency-cruiser.mjs",
    ".dependency-cruiser.json",
    ".dependency-cruiser.yaml",
    ".dependency-cruiser.yml",
];

/// The decision token in a comment: `adr:NNNN` or `plan:<slug>`
/// ([design § Rules an agent writes](../../../docs/artifacts/design.md#rules-an-agent-writes-held-to-the-same-bar)).
pub fn decision_token(comment: &str) -> Option<String> {
    let bytes = comment.as_bytes();
    let boundary = |i: usize| i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
    for (i, _) in comment.match_indices("adr:") {
        let digits: String = comment[i + 4..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if boundary(i) && !digits.is_empty() {
            return Some(format!("adr:{digits}"));
        }
    }
    for (i, _) in comment.match_indices("plan:") {
        let slug: String = comment[i + 5..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if boundary(i) && !slug.is_empty() {
            return Some(format!("plan:{slug}"));
        }
    }
    None
}

/// `--require-comment-token`: every dependency rule's comment carries a decision token.
///
/// # Errors
/// [`ConfigError::MissingToken`] for the first rule without one.
pub fn require_comment_tokens(config: &Config) -> Result<(), ConfigError> {
    for (_, rule) in config.rules.all_dependency_rules() {
        if rule
            .meta
            .comment
            .as_deref()
            .and_then(decision_token)
            .is_none()
        {
            return Err(ConfigError::MissingToken {
                rule: rule.name().to_owned(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_dependency_cruiser_names() {
        for ext in ["json", "yaml", "yml", "cjs", "js", "mjs"] {
            assert_eq!(
                ConfigFormat::detect(&format!("repo/.dependency-cruiser.{ext}")),
                Some(ConfigFormat::DependencyCruiser)
            );
        }
    }

    #[test]
    fn detects_native_names() {
        for ext in ["yaml", "yml", "json", "jsonc", "toml"] {
            assert_eq!(
                ConfigFormat::detect(&format!("rulebearing.{ext}")),
                Some(ConfigFormat::Native)
            );
        }
    }

    #[test]
    fn rejects_unknown_names() {
        assert_eq!(ConfigFormat::detect("package.json"), None);
        assert_eq!(ConfigFormat::detect(".dependency-cruiser.ts"), None);
        assert_eq!(ConfigFormat::detect("rulebearing.txt"), None);
    }

    #[test]
    fn config_format_flag_values() {
        assert_eq!(ConfigFormat::parse("native"), Some(ConfigFormat::Native));
        assert_eq!(
            ConfigFormat::parse("dependency-cruiser"),
            Some(ConfigFormat::DependencyCruiser)
        );
        assert_eq!(ConfigFormat::parse("xml"), None);
    }

    #[test]
    fn errors_render() {
        let e = ConfigError::Unanswerable {
            rule: "r".into(),
            predicate: "areSealed".into(),
            language: "python".into(),
        };
        assert!(e.to_string().contains("areSealed"));
        let strict = ConfigError::Strict {
            rule: String::new(),
            message: "m".into(),
        };
        assert_eq!(strict.to_string(), "--strict-compat: m");
        let wave = ConfigError::NotYetSupported {
            key: "rules.elements".into(),
            wave: 2,
        };
        assert!(wave.to_string().contains("wave 2"));
    }

    #[test]
    fn decision_tokens_are_found() {
        assert_eq!(decision_token("why. adr:0003").as_deref(), Some("adr:0003"));
        assert_eq!(
            decision_token("plan:wave-1 and adr:12").as_deref(),
            Some("adr:12")
        );
        assert_eq!(
            decision_token("see plan:0001-wave-1.").as_deref(),
            Some("plan:0001-wave-1")
        );
        assert_eq!(decision_token("badr:0003 adr: x"), None);
        assert_eq!(decision_token("no token"), None);
    }

    #[test]
    fn comment_tokens_are_required_of_every_rule() -> Result<(), ConfigError> {
        let mut config = Config::default();
        config.rules.dependencies.forbidden.push(Rule {
            meta: RuleMeta {
                name: Some("ok".into()),
                comment: Some("adr:0001".into()),
                ..RuleMeta::default()
            },
            ..Rule::default()
        });
        require_comment_tokens(&config)?;
        config.rules.dependencies.forbidden.push(Rule::default());
        assert!(matches!(
            require_comment_tokens(&config),
            Err(ConfigError::MissingToken { .. })
        ));
        Ok(())
    }
}
