//! `rb-wasm`: the playground's facade, the library crates compiled to `wasm32-unknown-unknown`
//! behind exports that take and return strings, so the page needs no shared types.
//!
//! - Architecture: [`docs/architecture.md#crate-layout`](../../../docs/architecture.md#crate-layout)
//! - Decision: [ADR-0002](../../../docs/adr/0002-rust-as-implementation-language.md) (the
//!   consequence: "a WebAssembly playground is nearly free because the crates compile to
//!   `wasm32`"), [ADR-0010](../../../docs/adr/0010-crate-layout-and-extractor-boundary.md) (a
//!   facade outside the dependency direction: no crate depends on it, and it never links `rb-cli`)
//! - Plan: [Wave 4, Step 2](../../../docs/plans/pending/0004-wave-4-reach.md#21-steps-for-sub-wave-4a-docs-site-playground-cookbook-live-tables);
//!   the exports are frozen in [§ 1.6](../../../docs/plans/pending/0004-wave-4-reach.md#16-interfaces-and-contracts-this-wave-freezes)
//! - Requirements: [FR-REACH-01](../../../docs/prd.md#fr-reach-01)
//! - Source: [design § The developer relations hat](../../../docs/artifacts/design.md#the-developer-relations-hat-the-first-ten-minutes-and-the-brownfield-repo)
//!   (the playground: "try before install")
//!
//! - Decision: [ADR-0073](../../../docs/adr/0073-extractors-read-files-through-rb-model.md) (the
//!   extractors read the dropped files from memory)
//!
//! | Module | Holds |
//! | --- | --- |
//! | [`playground`] | `evaluate`, `explain` and `version`, in plain Rust: the native tests reach every line |
//! | [`cruise`] | `cruise_ts` and `cruise_py`: dropped files extracted from memory, then evaluated |
//!
//! The `wasm-bindgen` exports exist only on `wasm32`, where they turn a JavaScript `Map` (or a
//! plain object) of paths to source text into the files a [`cruise`] reads, and a
//! [`playground::PlaygroundError`] into the string the page shows. All five exports of
//! [plan 0004 § 1.6](../../../docs/plans/pending/0004-wave-4-reach.md#16-interfaces-and-contracts-this-wave-freezes)
//! are here.

pub mod cruise;
pub mod playground;

#[cfg(target_arch = "wasm32")]
mod exports {
    use std::collections::BTreeMap;

    use wasm_bindgen::JsCast as _;
    use wasm_bindgen::prelude::{JsValue, wasm_bindgen};

    use crate::cruise::{self, Extractor};
    use crate::playground::{self, Now};

    fn refused(error: &impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&error.to_string())
    }

    /// The page's files: a `Map` or a plain object of paths to source text.
    fn files(value: JsValue) -> Result<BTreeMap<String, String>, JsValue> {
        let entries = match value.dyn_into::<js_sys::Map>() {
            Ok(map) => js_sys::Array::from(&map.entries()),
            Err(value) if value.is_object() => {
                js_sys::Object::entries(&js_sys::Object::from(value))
            }
            Err(_) => {
                return Err(JsValue::from_str(
                    "the files are a Map (or an object) of paths to source text",
                ));
            }
        };
        let mut out = BTreeMap::new();
        for entry in entries.iter() {
            let pair = js_sys::Array::from(&entry);
            let (Some(path), Some(text)) = (pair.get(0).as_string(), pair.get(1).as_string())
            else {
                return Err(JsValue::from_str(
                    "every file is a path and its source text, both strings",
                ));
            };
            out.insert(path, text);
        }
        Ok(out)
    }

    /// The browser's clock as the command line reads its own: today in UTC, and an ISO 8601
    /// timestamp without the `Z` (`rb_report::ReportOptions::timestamp`).
    fn now() -> Now {
        let date = js_sys::Date::new_0();
        let today = chrono::NaiveDate::from_ymd_opt(
            i32::try_from(date.get_utc_full_year()).unwrap_or(1970),
            date.get_utc_month() + 1,
            date.get_utc_date(),
        )
        .unwrap_or_default();
        let iso = String::from(date.to_iso_string());
        Now {
            today,
            timestamp: iso.strip_suffix('Z').unwrap_or(&iso).to_owned(),
        }
    }

    /// The configuration's rules over a pasted cruise result, rendered by `output_type`.
    ///
    /// # Errors
    /// The reason, as text, when the configuration, the graph or the reporter fails.
    #[wasm_bindgen]
    pub fn evaluate(config: &str, cruise_json: &str, output_type: &str) -> Result<String, JsValue> {
        playground::evaluate(config, cruise_json, output_type, &now()).map_err(|e| refused(&e))
    }

    /// The dropped TypeScript and JavaScript files extracted, the configuration's rules over the
    /// graph, rendered by `output_type`.
    ///
    /// # Errors
    /// The reason, as text, when the files, the configuration, the extraction or the reporter
    /// fails.
    #[wasm_bindgen]
    pub fn cruise_ts(files: JsValue, config: &str, output_type: &str) -> Result<String, JsValue> {
        cruise::cruise(
            Extractor::TypeScript,
            &self::files(files)?,
            config,
            output_type,
            &now(),
        )
        .map_err(|e| refused(&e))
    }

    /// The dropped Python files extracted, the configuration's rules over the graph, rendered by
    /// `output_type`.
    ///
    /// # Errors
    /// As `cruise_ts`.
    #[wasm_bindgen]
    pub fn cruise_py(files: JsValue, config: &str, output_type: &str) -> Result<String, JsValue> {
        cruise::cruise(
            Extractor::Python,
            &self::files(files)?,
            config,
            output_type,
            &now(),
        )
        .map_err(|e| refused(&e))
    }

    /// A rule of the configuration explained over a pasted cruise result.
    ///
    /// # Errors
    /// The reason, as text, when the configuration or the graph fails or there is no such rule.
    #[wasm_bindgen]
    pub fn explain(config: &str, cruise_json: &str, rule: &str) -> Result<String, JsValue> {
        playground::explain(config, cruise_json, rule, &now()).map_err(|e| refused(&e))
    }

    /// The Rulebearing version the playground was built from.
    #[wasm_bindgen]
    pub fn version() -> String {
        playground::version().to_owned()
    }
}
