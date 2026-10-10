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
//! | Module | Holds |
//! | --- | --- |
//! | [`playground`] | what each export does, in plain Rust: the native tests reach every line |
//!
//! The `wasm-bindgen` exports exist only on `wasm32`, where they turn a [`playground`] error into
//! the string the page shows. This step exports `evaluate` and `version`. `cruise_ts`,
//! `cruise_py` and `explain` follow in later steps of the same plan, once the extractors read an
//! in-memory file tree and `explain` lives in a library crate.

pub mod playground;

#[cfg(target_arch = "wasm32")]
mod exports {
    use wasm_bindgen::prelude::{JsValue, wasm_bindgen};

    use crate::playground::{self, Now};

    /// The browser's clock: today's date and an ISO 8601 timestamp.
    fn now() -> Now {
        let date = js_sys::Date::new_0();
        let today = chrono::NaiveDate::from_ymd_opt(
            i32::try_from(date.get_full_year()).unwrap_or(1970),
            date.get_month() + 1,
            date.get_date(),
        )
        .unwrap_or_default();
        Now {
            today,
            timestamp: String::from(date.to_iso_string()),
        }
    }

    /// The configuration's rules over a pasted cruise result, rendered by `output_type`.
    ///
    /// # Errors
    /// The reason, as text, when the configuration, the graph or the reporter fails.
    #[wasm_bindgen]
    pub fn evaluate(config: &str, cruise_json: &str, output_type: &str) -> Result<String, JsValue> {
        playground::evaluate(config, cruise_json, output_type, &now())
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// The Rulebearing version the playground was built from.
    #[wasm_bindgen]
    pub fn version() -> String {
        playground::version().to_owned()
    }
}
