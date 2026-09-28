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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::code::CodeLayer;
use crate::document::{Module, Receipt, SidecarReceipt};

/// What one extractor produced. It serialises, so a cache can keep it and a later run can
/// reuse it ([Wave 3, Step 2](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extraction {
    /// Module-layer nodes with their edges.
    pub modules: Vec<Module>,
    /// Code-layer elements, when the extractor fills them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<CodeLayer>,
    /// The receipt: what was inspected.
    pub inspected: Receipt,
    /// Problems that did not stop the run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<Warning>,
    /// Per file, what a later incremental run needs that the modules do not carry, keyed by the
    /// module's `source`. Empty for an extractor whose modules carry everything.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, FileState>,
    /// The Node sidecar's part in this extraction, when it extracted any file
    /// ([ADR-0017](../../../docs/adr/0017-coffeescript-livescript-sidecar.md)); the run's
    /// `summary.sidecar`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidecar: Option<SidecarReceipt>,
}

/// What an extractor keeps of one file so a later run can reuse the file's result without
/// reading it again: the parts that cannot be recovered from the file's module.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileState {
    /// The file's own code layer before anything graph-wide (linking, base chains) was applied,
    /// in the shape the extractor that wrote it reads back; opaque to every other crate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<serde_json::Value>,
    /// The warnings the file produced, in the order the extractor reported them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<Warning>,
}

/// A request to extract again after some files changed: which files changed, which did not, and
/// the earlier extraction the unchanged files' results are taken from
/// ([Wave 3, Step 2](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#21-steps-for-sub-wave-3a-cache---affected-diff---exit-code-mode-strict)).
/// Paths are relative to the working directory, as module sources are. A file in neither list
/// is read, as a changed one is.
///
/// Reuse is sound only while nothing an unchanged file resolves against has changed: the caller
/// sends a request only when no file was added, deleted or renamed and no manifest changed, and
/// an extractor that cannot reproduce a file's result exactly from `previous` reads the file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtractRequest {
    /// Files whose content changed since `previous`.
    pub changed: Vec<PathBuf>,
    /// Files whose content is the same as when `previous` was extracted.
    pub unchanged: Vec<PathBuf>,
    /// The earlier extraction.
    pub previous: Extraction,
}

impl ExtractRequest {
    /// The unchanged files as module sources: `/`-separated, without a leading `./`.
    pub fn unchanged_sources(&self) -> BTreeSet<String> {
        self.unchanged.iter().map(|p| source_name(p)).collect()
    }
}

/// A path as a module `source` spells it: `/`-separated, without a leading `./`.
pub fn source_name(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut rest = text.as_str();
    while let Some(stripped) = rest.strip_prefix("./") {
        rest = stripped;
    }
    rest.to_owned()
}

/// A problem that did not stop extraction, reported with the file it concerns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    /// The file concerned, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

    #[test]
    fn an_extraction_round_trips_through_json() -> Result<(), serde_json::Error> {
        let mut files = BTreeMap::new();
        files.insert(
            "a.py".to_owned(),
            FileState {
                code: Some(serde_json::json!({ "types": [] })),
                warnings: vec![Warning::about("a.py", "cannot parse")],
            },
        );
        let extraction = Extraction {
            modules: vec![Module::new("a.py")],
            code: None,
            inspected: Receipt::counts(1, 0, 1),
            warnings: vec![Warning {
                path: None,
                message: "general".to_owned(),
            }],
            files,
            sidecar: Some(SidecarReceipt {
                tool: "dependency-cruiser".to_owned(),
                version: "18.2.0".to_owned(),
                files: 1,
            }),
        };
        let text = serde_json::to_string(&extraction)?;
        assert!(text.contains(r#""files":{"a.py""#), "{text}");
        assert!(
            text.contains(
                r#""sidecar":{"tool":"dependency-cruiser","version":"18.2.0","files":1}"#
            ),
            "{text}"
        );
        assert!(!text.contains(r#""code":null"#), "{text}");
        assert_eq!(serde_json::from_str::<Extraction>(&text)?, extraction);
        let empty = serde_json::to_string(&Extraction::default())?;
        assert!(
            !empty.contains(r#""files":{"#)
                && !empty.contains("warnings")
                && !empty.contains("sidecar"),
            "{empty}"
        );
        Ok(())
    }

    #[test]
    fn unchanged_files_are_named_as_module_sources() {
        let request = ExtractRequest {
            changed: vec![PathBuf::from("src/b.ts")],
            unchanged: vec![
                PathBuf::from("./src/a.ts"),
                PathBuf::from("src\\c.ts"),
                PathBuf::from("././d.ts"),
            ],
            previous: Extraction::default(),
        };
        let sources: Vec<String> = request.unchanged_sources().into_iter().collect();
        assert_eq!(sources, ["d.ts", "src/a.ts", "src/c.ts"]);
        assert_eq!(source_name(Path::new("x/y.py")), "x/y.py");
        assert_eq!(source_name(Path::new("")), "");
    }
}
