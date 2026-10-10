//! The graph document as a JSON value for a reporter, made and released without holding up the
//! run: the modules are serialised in parallel, and the value is dropped on a worker thread.
//!
//! - Plan: [Wave 3, Step 16](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#24-steps-for-sub-wave-3d---mode-source-guard---watch-the-2-s-proof)
//!   (`guard --watch` checks a saved file within 100 ms on the 5,500-module tree)
//! - Requirement: [NFR-PERF-03](../../../docs/prd.md#nfr-perf-03)
//! - Architecture: [§ Performance model](../../../docs/architecture.md#performance-model)
//!
//! On a large graph the reporter's input is most of a report's time: serialising every module
//! one after the other, and freeing the value afterwards. Neither changes what is written.

use rayon::prelude::*;
use rb_model::{CodeLayer, Folder, GraphDocument, RevisionData, Summary};
use serde::Serialize;
use serde_json::Value;

/// The document without its modules, serialised as [`GraphDocument`] is.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Shell<'a> {
    modules: [(); 0],
    #[serde(skip_serializing_if = "absent")]
    folders: &'a Option<Vec<Folder>>,
    #[serde(skip_serializing_if = "absent")]
    projects: &'a Option<Vec<Folder>>,
    summary: &'a Summary,
    #[serde(skip_serializing_if = "absent")]
    revision_data: &'a Option<RevisionData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'a CodeLayer>,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "the signature serde's skip_serializing_if calls"
)]
fn absent<T>(value: &&Option<T>) -> bool {
    value.is_none()
}

/// `serde_json::to_value(document)`: the same value, key for key and in the same order, with
/// the modules serialised in parallel.
///
/// # Errors
/// As `serde_json::to_value`.
pub fn document(document: &GraphDocument) -> serde_json::Result<Value> {
    with_code(document, true)
}

/// [`document`] without the code layer, for a reporter that does not read it
/// ([`rb_report::reads_code`]): the code layer is most of a compiled .NET graph, and is not
/// converted when nothing reads it.
///
/// # Errors
/// As `serde_json::to_value`.
pub fn without_code(document: &GraphDocument) -> serde_json::Result<Value> {
    with_code(document, false)
}

fn with_code(document: &GraphDocument, include_code: bool) -> serde_json::Result<Value> {
    // Every field is named, so a field added to the document does not compile until it is here.
    let GraphDocument {
        modules,
        folders,
        projects,
        summary,
        revision_data,
        code,
    } = document;
    let (shell, modules) = rayon::join(
        || {
            serde_json::to_value(Shell {
                modules: [],
                folders,
                projects,
                summary,
                revision_data,
                code: code.as_ref().filter(|_| include_code),
            })
        },
        || {
            modules
                .par_iter()
                .map(serde_json::to_value)
                .collect::<serde_json::Result<Vec<Value>>>()
        },
    );
    let mut shell = shell?;
    if let Some(slot) = shell.get_mut("modules") {
        *slot = Value::Array(modules?);
    }
    Ok(shell)
}

/// `document.clone()`, with the modules cloned in parallel.
pub fn copy(document: &GraphDocument) -> GraphDocument {
    let GraphDocument {
        modules,
        folders,
        projects,
        summary,
        revision_data,
        code,
    } = document;
    GraphDocument {
        modules: modules.par_iter().cloned().collect(),
        folders: folders.clone(),
        projects: projects.clone(),
        summary: summary.clone(),
        revision_data: revision_data.clone(),
        code: code.clone(),
    }
}

/// Frees `value` on a worker thread, so the caller does not wait for it.
pub fn release<T: Send + 'static>(value: T) {
    rayon::spawn(move || drop(value));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_model::{Dependency, Module, ModuleSystem};
    use serde_json::json;

    #[test]
    fn without_code_is_the_document_less_its_code_layer() -> serde_json::Result<()> {
        let document = GraphDocument {
            code: Some(CodeLayer::default()),
            ..GraphDocument::default()
        };
        let full = super::document(&document)?;
        assert!(full.get("code").is_some());
        let mut expected = full.clone();
        if let Some(map) = expected.as_object_mut() {
            map.remove("code");
        }
        assert_eq!(without_code(&document)?, expected);
        Ok(())
    }

    fn module(source: &str, to: &[&str]) -> Module {
        Module {
            dependencies: to
                .iter()
                .map(|t| Dependency::new(*t, *t, ModuleSystem::Es6))
                .collect(),
            ..Module::new(source)
        }
    }

    #[test]
    fn the_value_is_serde_s_whatever_the_document_holds() -> serde_json::Result<()> {
        let mut full = GraphDocument {
            modules: (0..200)
                .map(|i| module(&format!("src/m{i}.ts"), &["src/a.ts", "lodash"]))
                .collect(),
            folders: Some(Vec::new()),
            revision_data: Some(serde_json::from_value(
                json!({ "SHA1": "1111111111111111111111111111111111111111", "changes": [] }),
            )?),
            code: Some(CodeLayer::default()),
            ..GraphDocument::default()
        };
        full.summary.error = 3;
        full.summary
            .options_used
            .insert("args".into(), json!("src"));
        for document in [GraphDocument::default(), full] {
            assert_eq!(copy(&document), document);
            let made = super::document(&document)?;
            assert_eq!(made, serde_json::to_value(&document)?);
            // The same text too, which a reporter writes: the keys keep their order.
            assert_eq!(
                serde_json::to_string(&made)?,
                serde_json::to_string(&document)?
            );
        }
        Ok(())
    }

    #[test]
    fn a_released_value_is_freed_on_a_worker_thread() {
        // A value whose last owner reports the thread it was dropped on.
        struct Witness(std::sync::mpsc::Sender<std::thread::ThreadId>);
        impl Drop for Witness {
            fn drop(&mut self) {
                let _ = self.0.send(std::thread::current().id());
            }
        }
        let (sent, received) = std::sync::mpsc::channel();
        release((json!({ "modules": [{ "source": "a.ts" }] }), Witness(sent)));
        let dropped_on = received.recv_timeout(std::time::Duration::from_secs(5));
        assert!(dropped_on.is_ok_and(|id| id != std::thread::current().id()));
    }
}
