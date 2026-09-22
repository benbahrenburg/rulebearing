//! `rb-config`: both configuration formats into one model.
//!
//! - Architecture: [`docs/architecture.md#configuration-and-the-rule-language`](../../../docs/architecture.md#configuration-and-the-rule-language)
//! - Decisions: [ADR-0005](../../../docs/adr/0005-native-config-superset-and-compat.md),
//!   [ADR-0006](../../../docs/adr/0006-embedded-quickjs-config-evaluator.md),
//!   [ADR-0016](../../../docs/adr/0016-linear-time-regex-and-strict-compat.md)
//! - Plan: [Wave 1, sub-wave 1A](../../../docs/plans/pending/0001-wave-1-typescript-parity.md)
//! - Requirements: [FR-CFG-01](../../../docs/prd.md#fr-cfg-01) to [FR-CFG-07](../../../docs/prd.md#fr-cfg-07)
//! - Source: [design § Configuration](../../../docs/artifacts/design.md#configuration-a-native-format-and-dependency-cruisers-as-it-is)
//!
//! Wave 0 ships the crate boundary and the error type only; the parsers land in wave 1.

use thiserror::Error;

/// Errors a configuration front-end can raise. Exit code 3 in the CLI
/// ([ADR-0008](../../../docs/adr/0008-exit-code-contract.md)).
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The file did not validate against the schema.
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
    fn errors_render() {
        let e = ConfigError::Unanswerable {
            rule: "r".into(),
            predicate: "areSealed".into(),
            language: "python".into(),
        };
        assert!(e.to_string().contains("areSealed"));
    }
}
