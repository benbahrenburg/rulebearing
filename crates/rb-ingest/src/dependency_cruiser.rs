//! dependency-cruiser's `cruise-result` in, the graph document out: `fmt --from dependency-cruiser`.
//!
//! - Plan: [Wave 1, Step 11](../../../docs/plans/pending/0001-wave-1-typescript-parity.md#step-11-rb-ingest-for-dependency-cruiser-json-1d)
//! - Decision: [ADR-0004](../../../docs/adr/0004-graph-document-is-cruise-result-superset.md)
//!   (the module layer is the same schema, so no translation is needed)
//! - Requirement: [FR-CLI-01](../../../docs/prd.md#fr-cli-01)
//!
//! The result deserialises straight into [`GraphDocument`]. Ingest then adds what Rulebearing
//! records and dependency-cruiser does not: `language` from each followed module's extension, and
//! the `inspected` receipt from `totalCruised`. `line` and `column` stay absent: dependency-cruiser
//! does not record them, and inventing them would be a lie.

use rb_model::{GraphDocument, Inspected, Language, Receipt};

/// The language of a source path, by extension; `None` for anything else.
pub fn language_of(source: &str) -> Option<Language> {
    let extension = source.rsplit_once('.').map(|(_, e)| e)?;
    match extension {
        "ts" | "tsx" | "mts" | "cts" => Some(Language::Typescript),
        "js" | "jsx" | "mjs" | "cjs" => Some(Language::Javascript),
        _ => None,
    }
}

/// Reads a result dependency-cruiser wrote.
///
/// # Errors
/// The serde error when the text is not a `cruise-result`.
pub fn read(json: &str) -> Result<GraphDocument, serde_json::Error> {
    let mut document: GraphDocument = serde_json::from_str(json)?;
    let mut typescript = 0u64;
    let mut javascript = 0u64;
    for module in &mut document.modules {
        let followed = module.followable != Some(false)
            && module.core_module != Some(true)
            && module.could_not_resolve != Some(true);
        if followed && module.language.is_none() {
            module.language = language_of(&module.source);
        }
        match module.language {
            Some(Language::Typescript) => typescript += 1,
            Some(Language::Javascript) => javascript += 1,
            _ => {}
        }
    }
    if document.summary.inspected.is_none() {
        let mut inspected = Inspected::new();
        let total = document.summary.total_cruised;
        let mut add = |language, files: u64| {
            if files > 0 {
                inspected.insert(language, Receipt::counts(files, 0, files));
            }
        };
        add(Language::Typescript, typescript);
        add(Language::Javascript, javascript);
        if inspected.is_empty() && total > 0 {
            inspected.insert(Language::Javascript, Receipt::counts(0, 0, total));
        }
        document.summary.inspected = Some(inspected);
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESULT: &str = r#"{
      "modules": [
        {"source": "src/a.ts", "valid": true, "dependencies": [{
          "module": "./b", "resolved": "src/b.js", "dependencyTypes": ["local"], "couldNotResolve": false,
          "circular": false, "coreModule": false, "exoticallyRequired": false, "dynamic": false,
          "followable": true, "moduleSystem": "es6", "valid": true }]},
        {"source": "src/b.js", "valid": true, "dependencies": []},
        {"source": "fs", "valid": true, "dependencies": [], "coreModule": true, "followable": false},
        {"source": "README.md", "valid": true, "dependencies": []}
      ],
      "summary": {"violations": [], "error": 0, "warn": 0, "info": 0, "totalCruised": 4, "optionsUsed": {}}
    }"#;

    #[test]
    fn languages_and_receipt_are_added() -> Result<(), serde_json::Error> {
        let document = read(RESULT)?;
        assert_eq!(document.modules[0].language, Some(Language::Typescript));
        assert_eq!(document.modules[1].language, Some(Language::Javascript));
        assert_eq!(
            document.modules[2].language, None,
            "core modules are not files"
        );
        assert_eq!(document.modules[3].language, None);
        assert!(document.modules[0].dependencies[0].line.is_none());
        let inspected = document.summary.inspected.unwrap_or_default();
        assert_eq!(
            inspected.get(&Language::Typescript).map(|r| r.files),
            Some(1)
        );
        assert_eq!(
            inspected.get(&Language::Javascript).map(|r| r.modules),
            Some(1)
        );
        Ok(())
    }

    #[test]
    fn a_result_with_no_known_extension_still_has_a_receipt() -> Result<(), serde_json::Error> {
        let document = read(
            r#"{"modules":[{"source":"x.vue","valid":true,"dependencies":[]}],"summary":{"violations":[],"error":0,"warn":0,"info":0,"totalCruised":1,"optionsUsed":{}}}"#,
        )?;
        let inspected = document.summary.inspected.unwrap_or_default();
        assert_eq!(
            inspected.get(&Language::Javascript).map(|r| r.modules),
            Some(1)
        );
        assert!(read("[]").is_err());
        assert_eq!(language_of("a.mts"), Some(Language::Typescript));
        assert_eq!(language_of("noext"), None);
        Ok(())
    }
}
