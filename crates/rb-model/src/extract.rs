//! The one signature every extractor implements.
//!
//! - Decision: [ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md)
//!   (extractors depend on `rb-model` only and write document types)
//! - Architecture: [Extractors](../../../docs/architecture.md#extractors)
//! - Plan: [Wave 0, Step 3](../../../docs/plans/pending/0000-wave-0-spike.md#step-3-rb-model-graph-document-schema-violation-id-0a)
//!   (the trait is frozen in § 1.5)
//! - Exit codes: [ADR-0008](../../../docs/adr/0008-exit-code-contract.md) (every
//!   [`ExtractError`] is a reason the run is untrustworthy, exit 2)
//!
//! No language reaches past this trait: `rb-cli` holds a list of extractors and merges what
//! each returns, and nothing after extraction knows which one produced a module.

use std::path::{Path, PathBuf};

use crate::code::CodeLayer;
use crate::document::{Module, Receipt};

/// What one extractor produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Extraction {
    /// Module-layer nodes with their edges.
    pub modules: Vec<Module>,
    /// Code-layer elements, when the extractor fills them.
    pub code: Option<CodeLayer>,
    /// The receipt: what was inspected.
    pub inspected: Receipt,
    /// Problems that did not stop the run.
    pub warnings: Vec<Warning>,
}

/// A problem that did not stop extraction, reported with the file it concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// The file concerned, when there is one.
    pub path: Option<PathBuf>,
    /// What happened and what to do about it.
    pub message: String,
}

impl Warning {
    /// A warning about one file.
    pub fn about(path: impl AsRef<Path>, message: impl Into<String>) -> Self {
        Self {
            path: Some(path.as_ref().to_path_buf()),
            message: message.into(),
        }
    }
}

/// Reasons that make a run untrustworthy. Each names the thing to fix.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// The roots held nothing this extractor reads.
    #[error("no modules found under the given roots; check the paths and the exclude patterns")]
    NoModulesFound,
    /// A solution was found but none of its projects had been built.
    #[error(
        "no built assemblies for {solution}; run `dotnet build` first, or set languages.dotnet.configuration"
    )]
    NoBuiltAssemblies {
        /// The solution file.
        solution: PathBuf,
    },
    /// An assembly's PDB is a Windows PDB, which carries no portable document table.
    #[error("{assembly} has a Windows PDB; build with -p:DebugType=portable")]
    NonPortablePdb {
        /// The assembly.
        assembly: PathBuf,
    },
    /// A file the extractor must read is malformed.
    #[error("{path}: {reason}", path = path.display())]
    UnsupportedFile {
        /// The file.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
    },
    /// Reading a file failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// An extractor: roots and options in, module-layer nodes out.
pub trait Extractor {
    /// The per-language options from [`crate::options`].
    type Options;

    /// Extracts every module under `roots`.
    ///
    /// # Errors
    /// Any [`ExtractError`]; the CLI maps each to exit code 2.
    fn extract(
        &self,
        roots: &[PathBuf],
        options: &Self::Options,
    ) -> Result<Extraction, ExtractError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Vec<&'static str>);

    impl Extractor for Fixed {
        type Options = ();

        fn extract(&self, roots: &[PathBuf], (): &()) -> Result<Extraction, ExtractError> {
            if roots.is_empty() || self.0.is_empty() {
                return Err(ExtractError::NoModulesFound);
            }
            Ok(Extraction {
                modules: self.0.iter().map(|s| Module::new(*s)).collect(),
                inspected: Receipt::counts(self.0.len() as u64, 0, self.0.len() as u64),
                ..Extraction::default()
            })
        }
    }

    #[test]
    fn the_trait_is_implementable_and_reports_empty_runs() {
        let roots = [PathBuf::from("src")];
        let found = Fixed(vec!["a.ts"]).extract(&roots, &()).ok();
        assert_eq!(found.map(|e| e.inspected.modules), Some(1));
        assert!(matches!(
            Fixed(vec![]).extract(&roots, &()),
            Err(ExtractError::NoModulesFound)
        ));
    }

    #[test]
    fn every_error_names_what_to_fix() {
        let cases = [
            (ExtractError::NoModulesFound.to_string(), "exclude patterns"),
            (
                ExtractError::NoBuiltAssemblies {
                    solution: PathBuf::from("A.sln"),
                }
                .to_string(),
                "dotnet build",
            ),
            (
                ExtractError::NonPortablePdb {
                    assembly: PathBuf::from("A.dll"),
                }
                .to_string(),
                "DebugType=portable",
            ),
            (
                ExtractError::UnsupportedFile {
                    path: PathBuf::from("x.dll"),
                    reason: "truncated CLI header".to_owned(),
                }
                .to_string(),
                "x.dll: truncated CLI header",
            ),
        ];
        for (message, needle) in cases {
            assert!(message.contains(needle), "{message}");
        }
        let io = ExtractError::from(std::io::Error::other("disk"));
        assert_eq!(io.to_string(), "disk");
        let warning = Warning::about("a.ts", "skipped");
        assert_eq!(warning.path, Some(PathBuf::from("a.ts")));
    }
}
